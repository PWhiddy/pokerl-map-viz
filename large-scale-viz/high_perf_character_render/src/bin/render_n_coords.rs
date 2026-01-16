use anyhow::Result;
use clap::Parser;
use sprite_video_renderer::animation::AnimationInterpolator;
use sprite_video_renderer::data::{CoordinateMapper, ParquetFilter, ParquetReader};
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Render first N coordinates from a parquet file", long_about = None)]
struct Args {
    /// Path to parquet file
    #[arg(long)]
    parquet_file: PathBuf,

    /// Number of coordinates to render
    #[arg(long, default_value = "1000")]
    num_coords: usize,

    /// Path to sprite sheet image
    #[arg(long, default_value = "../../assets/characters_transparent.png")]
    sprite_sheet: PathBuf,

    /// Path to map_data.json
    #[arg(long, default_value = "../../assets/map_data.json")]
    map_data: PathBuf,

    /// Output video file path
    #[arg(long, default_value = "output.mov")]
    output: PathBuf,

    /// Frame rate
    #[arg(long, default_value = "30")]
    fps: u32,

    /// Canvas width
    #[arg(long, default_value = "1920")]
    width: u32,

    /// Canvas height
    #[arg(long, default_value = "1080")]
    height: u32,

    /// Interval between coordinate points in milliseconds
    #[arg(long, default_value = "500")]
    interval_ms: u32,
}

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("=== Rendering first {} coordinates from parquet ===", args.num_coords);
    log::info!("Parquet file: {:?}", args.parquet_file);
    log::info!("Output: {:?}", args.output);

    let width = args.width;
    let height = args.height;
    let fps = args.fps;
    let interval_ms = args.interval_ms as f32;

    // Load coordinate mapper
    log::info!("Loading map data...");
    let coordinate_mapper = CoordinateMapper::load(&args.map_data)?;

    // Read parquet file and take first N valid frames
    log::info!("Reading parquet file...");
    let reader = ParquetReader::new(ParquetFilter::default());
    let mut all_frames = reader.read_file(&args.parquet_file)?;

    log::info!("Total frames with valid user+env_id: {}", all_frames.len());

    // Take first N
    all_frames.truncate(args.num_coords);
    log::info!("Using first {} frames", args.num_coords);

    // Group into sequences
    log::info!("Grouping into sequences...");
    let mut sequences = ParquetReader::group_into_sequences(all_frames);
    log::info!("Created {} sprite sequences", sequences.len());

    // Sort by number of frames for logging
    sequences.sort_by_key(|s| std::cmp::Reverse(s.frames.len()));

    if !sequences.is_empty() {
        let longest = &sequences[0];
        log::info!("Longest sequence: user={}, env_id={}, frames={}",
                   longest.user, longest.env_id, longest.frames.len());

        // Log first few coordinates for debugging
        if longest.frames.len() > 0 {
            log::info!("First coord: {:?}, sprite_id={}",
                       longest.frames[0].coords, longest.sprite_id);
        }
        if longest.frames.len() > 1 {
            log::info!("Second coord: {:?}", longest.frames[1].coords);
        }
    }

    // Create animation interpolator
    let interpolator = AnimationInterpolator::new(coordinate_mapper, interval_ms, fps as f32);

    let total_frames = interpolator.calculate_frame_count(&sequences);
    let duration_sec = interpolator.calculate_duration(&sequences) / 1000.0;

    log::info!("Animation: {:.2} seconds, {} frames @ {} fps", duration_sec, total_frames, fps);
    log::info!("Max sprites on screen: {}", sequences.len());

    // Initialize GPU
    log::info!("Initializing GPU...");
    let gpu = GpuContext::new(width, height).await?;

    // Load sprite sheet
    log::info!("Loading sprite sheet...");
    let texture_atlas = TextureAtlas::load(&gpu.device, &gpu.queue, &args.sprite_sheet)?;

    // Create renderer
    log::info!("Creating renderer...");
    let renderer = SpriteRenderer::new(
        &gpu.device,
        &gpu.queue,
        &texture_atlas,
        width,
        height,
        sequences.len() + 100, // Max sprites
    )?;

    // Create encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new(&args.output, width, height, fps)?;

    // Render frames
    log::info!("Rendering {} frames...", total_frames);
    let start_time = std::time::Instant::now();

    for frame_number in 0..total_frames {
        let time_ms = interpolator.frame_to_time(frame_number);

        // Calculate sprite instances for this frame
        let mut sprite_instances = Vec::new();

        for sequence in &sequences {
            if let Some(state) = interpolator.get_animation_state(sequence, time_ms) {
                if let Some(sprite_data) = interpolator.interpolate_sprite(sequence, &state) {
                    // Get texture coordinates
                    let tex_coords = texture_atlas.get_sprite_tex_coords(
                        sprite_data.sprite_id,
                        sprite_data.direction,
                    );

                    // Center sprite (16x16, so offset by -8)
                    sprite_instances.push(SpriteInstance {
                        position: [
                            sprite_data.position[0] - 8.0,
                            sprite_data.position[1] - 8.0,
                        ],
                        tex_rect: tex_coords,
                    });
                }
            }
        }

        // Render frame
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &gpu.render_texture_view,
            &sprite_instances,
        )?;

        // Read pixels
        let pixels = gpu.read_pixels().await?;

        // Write to encoder
        encoder.write_frame(&pixels)?;

        // Progress logging
        if frame_number % 30 == 0 || frame_number == total_frames - 1 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let fps_actual = (frame_number + 1) as f32 / elapsed;
            let progress = (frame_number + 1) as f32 / total_frames as f32 * 100.0;
            let eta = (total_frames - frame_number - 1) as f32 / fps_actual;

            log::info!(
                "Progress: {:.1}% ({}/{}) | {:.1} fps | ETA: {:.1}s | Sprites: {}",
                progress,
                frame_number + 1,
                total_frames,
                fps_actual,
                eta,
                sprite_instances.len()
            );
        }
    }

    let elapsed = start_time.elapsed();
    log::info!(
        "Rendering complete! {:.2}s ({:.1} fps)",
        elapsed.as_secs_f32(),
        total_frames as f32 / elapsed.as_secs_f32()
    );

    // Finalize encoder
    log::info!("Finalizing video...");
    encoder.finish()?;

    log::info!("✓ Done! Created {:?}", args.output);
    log::info!("Video: {}x{} @ {} fps, {:.2} seconds", width, height, fps, duration_sec);

    Ok(())
}
