use anyhow::Result;
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting test render (10 seconds @ 30 fps)");

    let width = 640;
    let height = 480;
    let fps = 30;
    let duration_secs = 1;  // Just 1 second for quick test
    let total_frames = fps * duration_secs;

    log::info!("Test: {}x{} @ {} fps, {} frames", width, height, fps, total_frames);

    // Initialize GPU
    log::info!("Initializing GPU context...");
    let gpu = GpuContext::new(width, height).await?;

    // Load sprite sheet
    log::info!("Loading sprite sheet...");
    let texture_atlas = TextureAtlas::load(
        &gpu.device,
        &gpu.queue,
        "../../assets/characters_transparent.png",
    )?;

    // Create renderer
    log::info!("Creating renderer...");
    let renderer = SpriteRenderer::new(
        &gpu.device,
        &gpu.queue,
        &texture_atlas,
        width,
        height,
        100,
    )?;

    // Create encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new("test_output.mov", width, height, fps)?;

    // Render frames
    log::info!("Rendering {} frames...", total_frames);
    let start = std::time::Instant::now();

    for frame_num in 0..total_frames {
        // Create test sprite instances - 5 sprites moving in a circle
        let mut sprites = Vec::new();
        for i in 0..5 {
            let angle = (frame_num as f32 / fps as f32) * 2.0 * std::f32::consts::PI + (i as f32 * 1.256);
            let radius = 150.0;
            let center_x = width as f32 / 2.0;
            let center_y = height as f32 / 2.0;

            let x = center_x + angle.cos() * radius;
            let y = center_y + angle.sin() * radius;

            // Get texture coords for sprite
            let sprite_id = i as u8;
            let direction = sprite_video_renderer::data::Direction::Down;
            let tex_coords = texture_atlas.get_sprite_tex_coords(sprite_id, direction);

            sprites.push(SpriteInstance {
                position: [x, y],
                tex_rect: tex_coords,
            });
        }

        // Render frame
        renderer.render(&gpu.device, &gpu.queue, &gpu.render_texture_view, &sprites)?;

        // Read pixels
        let pixels = gpu.read_pixels().await?;

        // Write to encoder
        encoder.write_frame(&pixels)?;

        log::info!("Frame {}/{}", frame_num + 1, total_frames);
    }

    let elapsed = start.elapsed();
    log::info!("Rendered {} frames in {:.2}s ({:.1} fps)",
               total_frames, elapsed.as_secs_f32(),
               total_frames as f32 / elapsed.as_secs_f32());

    log::info!("Finalizing video...");
    encoder.finish()?;

    log::info!("Done! Created test_output.mov");
    Ok(())
}
