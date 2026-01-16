use anyhow::Result;
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let width = 640;
    let height = 480;
    let fps = 30;

    log::info!("Testing alpha channel rendering...");

    // Initialize GPU
    let gpu = GpuContext::new(width, height).await?;
    let texture_atlas = TextureAtlas::load(
        &gpu.device,
        &gpu.queue,
        "../../assets/characters_transparent.png",
    )?;
    let renderer = SpriteRenderer::new(&gpu.device, &gpu.queue, &texture_atlas, width, height, 10)?;
    let mut encoder = ProResEncoder::new("test_alpha.mov", width, height, fps)?;

    // Render 30 frames - one sprite
    // Sprite is 16x16, so to center it we need to offset by -8,-8 from center
    let sprite_pos = [
        width as f32 / 2.0 - 8.0,   // Center X minus half sprite width
        height as f32 / 2.0 - 8.0,   // Center Y minus half sprite height
    ];

    log::info!("Sprite position: {:?}, canvas: {}x{}", sprite_pos, width, height);

    for _frame_num in 0..30 {
        let tex_coords = texture_atlas.get_sprite_tex_coords(0, sprite_video_renderer::data::Direction::Down);
        if _frame_num == 0 {
            log::info!("Texture coords: {:?}", tex_coords);
        }

        let sprites = vec![SpriteInstance {
            position: sprite_pos,
            tex_rect: tex_coords,
        }];

        renderer.render(&gpu.device, &gpu.queue, &gpu.render_texture_view, &sprites)?;
        let pixels = gpu.read_pixels().await?;

        // Check alpha values in a corner (should be 0 - transparent)
        let corner_offset = 0; // Top-left pixel
        let r = pixels[corner_offset];
        let g = pixels[corner_offset + 1];
        let b = pixels[corner_offset + 2];
        let a = pixels[corner_offset + 3];

        if _frame_num == 0 {
            log::info!("Corner pixel RGBA: ({}, {}, {}, {}) - should be (0,0,0,0) for transparency", r, g, b, a);
        }

        // Check center (should have alpha > 0 from sprite)
        let center_y = height / 2;
        let center_x = width / 2;
        let center_offset = ((center_y * width + center_x) * 4) as usize;
        let center_a = pixels[center_offset + 3];

        if _frame_num == 0 {
            log::info!("Center pixel alpha: {} - should be > 0 for sprite", center_a);
        }

        encoder.write_frame(&pixels)?;
    }

    encoder.finish()?;
    log::info!("Created test_alpha.mov");
    Ok(())
}
