use crate::data::SpriteInstance as DataSpriteInstance;
use crate::rendering::pipeline::{SpritePipeline, SpriteInstance, Vertex, QUAD_INDICES, QUAD_VERTICES};
use crate::rendering::texture_atlas::TextureAtlas;
use anyhow::Result;
use wgpu;

pub struct SpriteRenderer {
    pipeline: SpritePipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    max_sprites: usize,
}

impl SpriteRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_atlas: &TextureAtlas,
        canvas_width: u32,
        canvas_height: u32,
        max_sprites: usize,
    ) -> Result<Self> {
        let pipeline = SpritePipeline::new(
            device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            canvas_width,
            canvas_height,
        )?;

        // Create vertex buffer
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * QUAD_VERTICES.len()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(QUAD_VERTICES));

        // Create index buffer
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Index Buffer"),
            size: (std::mem::size_of::<u16>() * QUAD_INDICES.len()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(QUAD_INDICES));

        // Create instance buffer (large enough for max_sprites)
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Instance Buffer"),
            size: (std::mem::size_of::<SpriteInstance>() * max_sprites) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group
        let bind_group = pipeline.create_bind_group(
            device,
            queue,
            &texture_atlas.view,
            &texture_atlas.sampler,
            canvas_width,
            canvas_height,
        );

        Ok(Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            bind_group,
            max_sprites,
        })
    }

    /// Render a batch of sprites
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        sprites: &[SpriteInstance],
    ) -> Result<()> {
        if sprites.is_empty() {
            // Still need to clear the render target
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Clear Encoder"),
            });

            {
                let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Clear Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }

            queue.submit(Some(encoder.finish()));
            return Ok(());
        }

        let sprite_count = sprites.len().min(self.max_sprites);

        // Update instance buffer
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&sprites[..sprite_count]),
        );

        // Create command encoder
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Sprite Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Sprite Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.pipeline.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..QUAD_INDICES.len() as u32, 0, 0..sprite_count as u32);
        }

        queue.submit(Some(encoder.finish()));

        Ok(())
    }
}
