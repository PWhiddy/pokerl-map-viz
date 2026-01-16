use crate::data::Direction;
use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::Path;
use wgpu;

pub struct TextureAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
}

impl TextureAtlas {
    /// Load sprite sheet texture from file
    pub fn load<P: AsRef<Path>>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: P,
    ) -> Result<Self> {
        let img = image::open(path.as_ref())
            .context("Failed to open sprite sheet")?
            .to_rgba8();

        let dimensions = img.dimensions();

        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Sprite Texture Atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Use nearest-neighbor filtering for pixel-perfect sprites
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        log::info!(
            "Loaded sprite atlas: {}x{} from {:?}",
            dimensions.0,
            dimensions.1,
            path.as_ref()
        );

        Ok(Self {
            texture,
            view,
            sampler,
            width: dimensions.0,
            height: dimensions.1,
        })
    }

    /// Get texture coordinates for a sprite
    /// Formula from JS: sx = 9 + 17 * x, sy = 34 + 17 * y, width = 16, height = 16
    pub fn get_sprite_tex_coords(&self, sprite_id: u8, direction: Direction) -> [f32; 4] {
        let x = direction.column_index();
        let y = sprite_id as usize;

        let sx = 9.0 + 17.0 * x as f32;
        let sy = 34.0 + 17.0 * y as f32;
        let sprite_width = 16.0;
        let sprite_height = 16.0;

        // Normalize to 0-1 range for texture coordinates
        let u_min = sx / self.width as f32;
        let v_min = sy / self.height as f32;
        let u_max = (sx + sprite_width) / self.width as f32;
        let v_max = (sy + sprite_height) / self.height as f32;

        [u_min, v_min, u_max, v_max]
    }
}
