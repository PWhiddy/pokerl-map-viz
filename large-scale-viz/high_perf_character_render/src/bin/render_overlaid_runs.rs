use anyhow::Result;
use chrono::Duration;
use clap::Parser;
use sprite_video_renderer::data::{CoordinateMapper, ParquetFilter, ParquetReader};
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;
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

#[derive(Debug, Clone, Copy)]
struct CompactFrame {
    coords: [u8; 3],
}

#[derive(Debug)]
struct RunData {
    sprite_id: u8,
    frames: Vec<CompactFrame>,
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
    let mut frames = reader.read_file(&args.parquet_file)?;

    log::info!("Total frames read: {}", frames.len());

    // Sort all frames by user+env_id for grouping
    log::info!("Sorting frames...");
    frames.sort_by(|a, b| {
        (&a.user, &a.env_id, a.timestamp, a.path_index)
            .cmp(&(&b.user, &b.env_id, b.timestamp, b.path_index))
    });

    // Detect runs by scanning through sorted frames and extract compact data
    log::info!("Detecting runs with reset detection...");
    let mut runs = Vec::new();
    let gap_threshold = Duration::minutes(2);
    let min_duration = Duration::seconds(args.min_duration_secs);
    let reset_maps = vec![0u8, 37, 40];

    let mut i = 0;
    while i < frames.len() {
        let run_user = &frames[i].user;
        let run_env_id = &frames[i].env_id;
        let run_sprite_id = frames[i].sprite_id;

        let mut run_start_idx = i;

        // Find all frames for this user+env_id
        while i < frames.len() && &frames[i].user == run_user && &frames[i].env_id == run_env_id {
            i += 1;
        }

        let user_env_end_idx = i;

        // Now split this user+env_id into runs
        let mut run_current_idx = run_start_idx;

        for j in (run_start_idx + 1)..user_env_end_idx {
            let time_gap = frames[j].timestamp - frames[j-1].timestamp;
            let curr_map = frames[j].coords[2];
            let prev_map = frames[j-1].coords[2];

            let mut should_split = false;

            // Split on 2-minute gaps
            if time_gap >= gap_threshold {
                should_split = true;
            }

            // Split when jumping TO a reset map
            if reset_maps.contains(&curr_map) && !reset_maps.contains(&prev_map) {
                should_split = true;
            }

            if should_split {
                let duration = frames[j-1].timestamp - frames[run_current_idx].timestamp;

                // Filter by minimum duration
                if duration >= min_duration {
                    let duration_ms = duration.num_milliseconds() as f32;

                    // Extract only the coords we need (discard all strings)
                    let compact_frames: Vec<CompactFrame> = frames[run_current_idx..j]
                        .iter()
                        .map(|f| CompactFrame { coords: f.coords })
                        .collect();

                    runs.push(RunData {
                        sprite_id: run_sprite_id,
                        frames: compact_frames,
                        duration_ms,
                    });
                }

                run_current_idx = j;
            }
        }

        // Process final run for this user+env_id
        if run_current_idx < user_env_end_idx {
            let duration = frames[user_env_end_idx - 1].timestamp - frames[run_current_idx].timestamp;

            if duration >= min_duration {
                let duration_ms = duration.num_milliseconds() as f32;

                // Extract only the coords we need
                let compact_frames: Vec<CompactFrame> = frames[run_current_idx..user_env_end_idx]
                    .iter()
                    .map(|f| CompactFrame { coords: f.coords })
                    .collect();

                runs.push(RunData {
                    sprite_id: run_sprite_id,
                    frames: compact_frames,
                    duration_ms,
                });
            }
        }
    }

    // Drop the huge frames vector immediately
    drop(frames);

    log::info!("Total runs after filtering: {}", runs.len());

    if runs.is_empty() {
        log::warn!("No runs to render after filtering!");
        return Ok(());
    }

    // Log sample run details
    if !runs.is_empty() {
        let sample = &runs[0];
        log::info!("Sample run: frames={}, duration={:.1}s",
                   sample.frames.len(), sample.duration_ms / 1000.0);
        log::info!("  First frame coords: {:?}", sample.frames[0].coords);
    }

    // Find max duration
    let max_duration_ms = runs
        .iter()
        .map(|r| r.duration_ms)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();

    log::info!("Max run duration: {:.2}s", max_duration_ms / 1000.0);

    // Store coordinate mapper for later use
    let coordinate_mapper = coordinate_mapper;

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
    log::info!("Total sprites (runs): {}", runs.len());

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
        runs.len() + 1000, // Max sprites with buffer
    )?;

    // Create encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new(&args.output, args.width, args.height, args.fps)?;

    // Render frames
    log::info!("Rendering {} frames...", total_frames);
    let start_time = std::time::Instant::now();

    for frame_number in 0..total_frames {
        let time_ms = frame_number as f32 * (1000.0 / args.fps as f32);

        // Calculate sprite instances for this frame
        let mut sprite_instances = Vec::new();

        for run in &runs {
            // Calculate which frame within this run we should be at
            let run_frame_index = (time_ms / args.interval_ms as f32) as usize;

            if run_frame_index >= run.frames.len() {
                continue; // This run has finished
            }

            let next_frame_index = (run_frame_index + 1).min(run.frames.len() - 1);
            let interpolation_t = (time_ms / args.interval_ms as f32).fract();

            let current_frame = &run.frames[run_frame_index];
            let next_frame = &run.frames[next_frame_index];

            // Convert coordinates to pixel positions FIRST
            let current_pos = coordinate_mapper.convert_coords(&current_frame.coords);
            let next_pos = coordinate_mapper.convert_coords(&next_frame.coords);

            // Check pixel distance - only interpolate if moving <= 16 pixels (1 tile)
            let pixel_dx = (next_pos[0] - current_pos[0]).abs();
            let pixel_dy = (next_pos[1] - current_pos[1]).abs();
            let pixel_distance = pixel_dx.max(pixel_dy);

            let should_interpolate = pixel_distance <= 16.0;
            let interp_t = if should_interpolate { interpolation_t } else { 0.0 };

            // Interpolate position
            let position = [
                current_pos[0] + (next_pos[0] - current_pos[0]) * interp_t - 8.0,
                current_pos[1] + (next_pos[1] - current_pos[1]) * interp_t - 8.0,
            ];

            // Determine direction
            let dx = next_pos[0] - current_pos[0];
            let dy = next_pos[1] - current_pos[1];
            let direction = if dx.abs() > dy.abs() {
                if dx > 0.0 { sprite_video_renderer::data::Direction::Right }
                else { sprite_video_renderer::data::Direction::Left }
            } else {
                if dy > 0.0 { sprite_video_renderer::data::Direction::Down }
                else { sprite_video_renderer::data::Direction::Up }
            };

            // Get texture coordinates
            let tex_coords = texture_atlas.get_sprite_tex_coords(run.sprite_id, direction);

            sprite_instances.push(SpriteInstance {
                position,
                tex_rect: tex_coords,
            });
        }

        // Debug logging for first frame
        if frame_number == 0 {
            log::info!("First frame: {} runs, {} sprites rendered",
                       runs.len(), sprite_instances.len());

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
