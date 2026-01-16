mod animation;
mod data;
mod rendering;
mod video;

use animation::AnimationInterpolator;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use data::{CoordinateMapper, ParquetFilter, ParquetReader};
use regex::Regex;
use rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use std::path::PathBuf;
use video::ProResEncoder;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing parquet files
    #[arg(long)]
    parquet_dir: PathBuf,

    /// Specific parquet files to process (optional, defaults to all part_*.parquet files)
    #[arg(long)]
    parquet_files: Vec<String>,

    /// Path to sprite sheet image
    #[arg(long)]
    sprite_sheet: PathBuf,

    /// Path to map_data.json
    #[arg(long)]
    map_data: PathBuf,

    /// Output video file path
    #[arg(long)]
    output: PathBuf,

    /// User filter regex (optional)
    #[arg(long)]
    user_filter: Option<String>,

    /// Timestamp start filter (RFC3339 format)
    #[arg(long)]
    timestamp_start: Option<String>,

    /// Timestamp end filter (RFC3339 format)
    #[arg(long)]
    timestamp_end: Option<String>,

    /// Frame rate
    #[arg(long, default_value = "60")]
    fps: u32,

    /// Canvas width
    #[arg(long, default_value = "8192")]
    width: u32,

    /// Canvas height
    #[arg(long, default_value = "8192")]
    height: u32,

    /// Interval between coordinate points in milliseconds
    #[arg(long, default_value = "500")]
    interval_ms: u32,

    /// Maximum number of simultaneous sprites (for memory management)
    #[arg(long, default_value = "10000")]
    max_sprites: usize,
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("Starting sprite video renderer");
    log::info!("Output: {:?}", args.output);
    log::info!("Canvas: {}x{} @ {} fps", args.width, args.height, args.fps);

    // Build list of parquet files to process
    let parquet_files: Vec<PathBuf> = if args.parquet_files.is_empty() {
        // Find all part_*.parquet files
        std::fs::read_dir(&args.parquet_dir)
            .context("Failed to read parquet directory")?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        if name.to_string_lossy().starts_with("part_")
                            && name.to_string_lossy().ends_with(".parquet")
                        {
                            return Some(path);
                        }
                    }
                }
                None
            })
            .collect()
    } else {
        args.parquet_files
            .iter()
            .map(|name| args.parquet_dir.join(name))
            .collect()
    };

    log::info!("Processing {} parquet files", parquet_files.len());

    // Parse filters
    let user_regex = if let Some(pattern) = &args.user_filter {
        Some(Regex::new(pattern).context("Invalid user filter regex")?)
    } else {
        None
    };

    let timestamp_start = if let Some(ts_str) = &args.timestamp_start {
        Some(DateTime::parse_from_rfc3339(ts_str)
            .context("Invalid timestamp_start format")?
            .with_timezone(&Utc))
    } else {
        None
    };

    let timestamp_end = if let Some(ts_str) = &args.timestamp_end {
        Some(DateTime::parse_from_rfc3339(ts_str)
            .context("Invalid timestamp_end format")?
            .with_timezone(&Utc))
    } else {
        None
    };

    // Load coordinate mapper
    log::info!("Loading map data from {:?}", args.map_data);
    let coordinate_mapper = CoordinateMapper::load(&args.map_data)?;

    // Read parquet files
    log::info!("Reading parquet files...");
    let parquet_reader = ParquetReader::new(ParquetFilter {
        user_regex,
        timestamp_start,
        timestamp_end,
    });

    let frames = parquet_reader.read_files(&parquet_files)?;
    log::info!("Loaded {} frames", frames.len());

    if frames.is_empty() {
        log::warn!("No frames to render after filtering");
        return Ok(());
    }

    // Group into sequences
    log::info!("Grouping frames into sequences...");
    let sequences = ParquetReader::group_into_sequences(frames);
    log::info!("Created {} sprite sequences", sequences.len());

    // Create animation interpolator
    let interpolator = AnimationInterpolator::new(
        coordinate_mapper,
        args.interval_ms as f32,
        args.fps as f32,
    );

    let total_frames = interpolator.calculate_frame_count(&sequences);
    let duration_ms = interpolator.calculate_duration(&sequences);
    log::info!(
        "Animation duration: {:.2} seconds ({} frames)",
        duration_ms / 1000.0,
        total_frames
    );

    // Run async rendering
    pollster::block_on(render_video(
        args,
        sequences,
        interpolator,
        total_frames,
    ))?;

    log::info!("Done!");
    Ok(())
}

async fn render_video(
    args: Args,
    sequences: Vec<data::SpriteSequence>,
    interpolator: AnimationInterpolator,
    total_frames: usize,
) -> Result<()> {
    // Initialize GPU
    log::info!("Initializing GPU context...");
    let gpu = GpuContext::new(args.width, args.height).await?;

    // Load texture atlas
    log::info!("Loading sprite sheet from {:?}", args.sprite_sheet);
    let texture_atlas = TextureAtlas::load(&gpu.device, &gpu.queue, &args.sprite_sheet)?;

    // Create sprite renderer
    log::info!("Creating sprite renderer...");
    let sprite_renderer = SpriteRenderer::new(
        &gpu.device,
        &gpu.queue,
        &texture_atlas,
        args.width,
        args.height,
        args.max_sprites,
    )?;

    // Create video encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new(&args.output, args.width, args.height, args.fps)?;

    // Render each frame
    log::info!("Rendering {} frames...", total_frames);
    let start_time = std::time::Instant::now();

    for frame_number in 0..total_frames {
        let time_ms = interpolator.frame_to_time(frame_number);

        // Calculate sprite instances for this frame
        let mut sprite_instances = Vec::new();

        for sequence in &sequences {
            if let Some(state) = interpolator.get_animation_state(sequence, time_ms) {
                if let Some(sprite_data) = interpolator.interpolate_sprite(sequence, &state) {
                    // Get texture coordinates for this sprite
                    let tex_coords = texture_atlas.get_sprite_tex_coords(
                        sprite_data.sprite_id,
                        sprite_data.direction,
                    );

                    // Center the sprite (sprite is 16x16, position is top-left in shader)
                    // So subtract 8 pixels from both X and Y to center it
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
        sprite_renderer.render(
            &gpu.device,
            &gpu.queue,
            &gpu.render_texture_view,
            &sprite_instances,
        )?;

        // Read pixels from GPU
        let pixels = gpu.read_pixels().await?;

        // Write to encoder
        encoder.write_frame(&pixels)?;

        // Progress logging
        if frame_number % 60 == 0 || frame_number == total_frames - 1 {
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
        "Rendering complete! Total time: {:.2}s ({:.2} fps)",
        elapsed.as_secs_f32(),
        total_frames as f32 / elapsed.as_secs_f32()
    );

    // Finalize encoder
    encoder.finish()?;

    Ok(())
}
