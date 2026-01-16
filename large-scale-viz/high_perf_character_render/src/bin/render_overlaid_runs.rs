use anyhow::Result;
use chrono::{Duration, TimeZone, Utc};
use clap::Parser;
use sprite_video_renderer::animation::AnimationInterpolator;
use sprite_video_renderer::data::{CoordinateMapper, ParquetFilter, ParquetReader, SpriteFrame, SpriteSequence};
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Render all runs from parquet overlaid", long_about = None)]
struct Args {
    /// Path to parquet file
    #[arg(long)]
    parquet_file: PathBuf,

    /// Path to sprite sheet image
    #[arg(long, default_value = "../../assets/characters_transparent.png")]
    sprite_sheet: PathBuf,

    /// Path to map_data.json
    #[arg(long, default_value = "../../assets/map_data.json")]
    map_data: PathBuf,

    /// Output video file path
    #[arg(long, default_value = "overlaid_runs.mov")]
    output: PathBuf,

    /// Frame rate
    #[arg(long, default_value = "30")]
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

    /// Minimum run duration in seconds
    #[arg(long, default_value = "60")]
    min_duration_secs: i64,

    /// Maximum number of frames to render (for testing)
    #[arg(long)]
    max_frames: Option<usize>,
}

#[derive(Debug)]
struct NormalizedRun {
    user: String,
    env_id: String,
    sprite_id: u8,
    color: String,
    frames: Vec<SpriteFrame>,
    duration_ms: f32,
}

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("=== Rendering overlaid runs from parquet ===");
    log::info!("Parquet file: {:?}", args.parquet_file);
    log::info!("Output: {:?}", args.output);
    log::info!("Min run duration: {}s", args.min_duration_secs);

    // Load coordinate mapper
    log::info!("Loading map data...");
    let coordinate_mapper = CoordinateMapper::load(&args.map_data)?;

    // Read parquet file
    log::info!("Reading parquet file...");
    let reader = ParquetReader::new(ParquetFilter::default());
    let frames = reader.read_file(&args.parquet_file)?;

    log::info!("Total frames read: {}", frames.len());

    // Group frames by user+env_id
    log::info!("Grouping by user+env_id...");
    let mut user_env_frames: HashMap<String, Vec<SpriteFrame>> = HashMap::new();

    for frame in frames {
        let key = format!("{}-{}", frame.user, frame.env_id);
        user_env_frames.entry(key).or_insert_with(Vec::new).push(frame);
    }

    log::info!("Found {} unique user+env_id pairs", user_env_frames.len());

    // Split into runs and normalize
    log::info!("Splitting into runs and normalizing...");
    let mut normalized_runs = Vec::new();
    let gap_threshold = Duration::minutes(2);
    let min_duration = Duration::seconds(args.min_duration_secs);

    for mut frames in user_env_frames.into_values() {
        // Sort by timestamp
        frames.sort_by_key(|f| (f.timestamp, f.path_index));

        let user = frames[0].user.clone();
        let env_id = frames[0].env_id.clone();
        let sprite_id = frames[0].sprite_id;
        let color = frames[0].color.clone();

        let mut run_start_idx = 0;

        for i in 1..frames.len() {
            let time_gap = frames[i].timestamp - frames[i-1].timestamp;

            // If gap is >= 2 minutes, end the current run
            if time_gap >= gap_threshold {
                let run_frames = &frames[run_start_idx..i];
                let duration = run_frames[run_frames.len()-1].timestamp - run_frames[0].timestamp;

                // Filter by minimum duration
                if duration >= min_duration {
                    let normalized = normalize_run(
                        user.clone(),
                        env_id.clone(),
                        sprite_id,
                        color.clone(),
                        run_frames
                    );
                    normalized_runs.push(normalized);
                }

                run_start_idx = i;
            }
        }

        // Process final run
        let run_frames = &frames[run_start_idx..];
        let duration = run_frames[run_frames.len()-1].timestamp - run_frames[0].timestamp;

        if duration >= min_duration {
            let normalized = normalize_run(
                user.clone(),
                env_id.clone(),
                sprite_id,
                color.clone(),
                run_frames
            );
            normalized_runs.push(normalized);
        }
    }

    log::info!("Total runs after filtering: {}", normalized_runs.len());

    if normalized_runs.is_empty() {
        log::warn!("No runs to render after filtering!");
        return Ok(());
    }

    // Log sample run details
    if !normalized_runs.is_empty() {
        let sample = &normalized_runs[0];
        log::info!("Sample run: user={}, env_id={}, frames={}, duration={:.1}s",
                   sample.user, sample.env_id, sample.frames.len(), sample.duration_ms / 1000.0);
        if !sample.frames.is_empty() {
            log::info!("  First frame coords: {:?}", sample.frames[0].coords);
        }
    }

    // Find max duration
    let max_duration_ms = normalized_runs
        .iter()
        .map(|r| r.duration_ms)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();

    log::info!("Max run duration: {:.2}s", max_duration_ms / 1000.0);

    // Convert normalized runs to SpriteSequences
    let sequences: Vec<SpriteSequence> = normalized_runs
        .into_iter()
        .map(|run| SpriteSequence {
            user: run.user,
            env_id: run.env_id,
            sprite_id: run.sprite_id,
            color: run.color,
            frames: run.frames,
        })
        .collect();

    // Create animation interpolator
    let interpolator = AnimationInterpolator::new(
        coordinate_mapper,
        args.interval_ms as f32,
        args.fps as f32,
    );

    let mut total_frames = (max_duration_ms / 1000.0 * args.fps as f32).ceil() as usize;

    // Limit frames if max_frames is specified
    if let Some(max) = args.max_frames {
        if max < total_frames {
            total_frames = max;
            log::info!("Limiting to {} frames (instead of full animation)", max);
        }
    }

    log::info!("Animation: {:.2} seconds, {} frames @ {} fps",
               total_frames as f32 / args.fps as f32, total_frames, args.fps);
    log::info!("Total sprites (runs): {}", sequences.len());

    // Initialize GPU
    log::info!("Initializing GPU...");
    let gpu = GpuContext::new(args.width, args.height).await?;

    // Load sprite sheet
    log::info!("Loading sprite sheet...");
    let texture_atlas = TextureAtlas::load(&gpu.device, &gpu.queue, &args.sprite_sheet)?;

    // Create renderer
    log::info!("Creating renderer...");
    let renderer = SpriteRenderer::new(
        &gpu.device,
        &gpu.queue,
        &texture_atlas,
        args.width,
        args.height,
        sequences.len() + 1000, // Max sprites with buffer
    )?;

    // Create encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new(&args.output, args.width, args.height, args.fps)?;

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

        // Debug logging for first frame
        if frame_number == 0 {
            log::info!("First frame: {} sequences, {} sprites rendered",
                       sequences.len(), sprite_instances.len());

            if !sprite_instances.is_empty() {
                log::info!("Sample sprite positions (first 10):");
                for (i, instance) in sprite_instances.iter().take(10).enumerate() {
                    let in_bounds_x = instance.position[0] >= 0.0 && instance.position[0] < args.width as f32;
                    let in_bounds_y = instance.position[1] >= 0.0 && instance.position[1] < args.height as f32;
                    log::info!("  Sprite {}: pos=[{:.1}, {:.1}] in_bounds=({}, {})",
                               i, instance.position[0], instance.position[1], in_bounds_x, in_bounds_y);
                }
            } else {
                log::warn!("No sprites rendered in first frame!");
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
    log::info!("Video: {}x{} @ {} fps, {:.2} seconds",
               args.width, args.height, args.fps, max_duration_ms / 1000.0);

    Ok(())
}

/// Normalize a run so that all timestamps start from a base time of 0
fn normalize_run(
    user: String,
    env_id: String,
    sprite_id: u8,
    color: String,
    frames: &[SpriteFrame],
) -> NormalizedRun {
    if frames.is_empty() {
        return NormalizedRun {
            user,
            env_id,
            sprite_id,
            color,
            frames: Vec::new(),
            duration_ms: 0.0,
        };
    }

    let base_time = frames[0].timestamp;
    let end_time = frames[frames.len() - 1].timestamp;
    let duration_ms = (end_time - base_time).num_milliseconds() as f32;

    // Create normalized frames with timestamps relative to base_time
    let normalized_frames: Vec<SpriteFrame> = frames
        .iter()
        .map(|f| {
            let offset_ms = (f.timestamp - base_time).num_milliseconds();
            // Create a new timestamp starting from Unix epoch + offset
            let normalized_timestamp = Utc.timestamp_opt(0, 0).unwrap()
                + Duration::milliseconds(offset_ms);

            SpriteFrame {
                timestamp: normalized_timestamp,
                user: f.user.clone(),
                env_id: f.env_id.clone(),
                sprite_id: f.sprite_id,
                color: f.color.clone(),
                extra: f.extra.clone(),
                coords: f.coords,
                path_index: f.path_index,
            }
        })
        .collect();

    NormalizedRun {
        user,
        env_id,
        sprite_id,
        color,
        frames: normalized_frames,
        duration_ms,
    }
}
