use anyhow::Result;
use clap::Parser;
use rand::Rng;
use sprite_video_renderer::data::{CoordinateMapper, INVALID_MAP_ID_FLAG};
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::collections::HashSet;

#[derive(Parser, Debug)]
#[command(author, version, about = "Render compact runs to video", long_about = None)]
struct Args {
    /// Input compact runs file (compressed or uncompressed)
    #[arg(long)]
    input: PathBuf,

    /// Path to sprite sheet image
    #[arg(long, default_value = "../../assets/characters_transparent.png")]
    sprite_sheet: PathBuf,

    /// Path to map_data.json
    #[arg(long, default_value = "../../assets/map_data.json")]
    map_data: PathBuf,

    /// Output video file path
    #[arg(long, default_value = "compact_runs_output.mov")]
    output: PathBuf,

    /// Frame rate
    #[arg(long, default_value = "60")]
    fps: u32,

    /// Canvas width
    #[arg(long, default_value = "8192")]
    width: u32,

    /// Canvas height
    #[arg(long, default_value = "8192")]
    height: u32,

    /// Interval between coordinate points in milliseconds (base interval)
    #[arg(long, default_value = "500")]
    interval_ms: u32,

    /// Animation speed multiplier (4 = 4x faster, uses 125ms between coords instead of 500ms)
    #[arg(long, default_value = "4")]
    speed_multiplier: u32,

    /// Starting step/coordinate index in the runs (default: 0)
    #[arg(long)]
    start_step: Option<usize>,

    /// Maximum number of frames to render (for testing)
    #[arg(long)]
    max_frames: Option<usize>,
}

/*
//use this to read old data
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct CompactCoord {
    x: u16,
    y: u16,
    map_id: u16,
}
*/

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct UltraCompactCoordMem {
    x: u8,
    y: u8,
    map_id: u8,
}

#[derive(Debug)]
struct CompactRun {
    sprite_id: u8,
    coords: Vec<UltraCompactCoordMem>,
}

#[derive(Debug, Clone)]
struct CompactRunMetadata {
    sprite_id: u8,
    coord_count: usize,
    file_offset: u64,
}

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("=== Rendering compact runs ===");
    log::info!("Input file: {:?}", args.input);
    log::info!("Output: {:?}", args.output);
    log::info!("Speed multiplier: {}x ({}ms between coords)", args.speed_multiplier, args.interval_ms / args.speed_multiplier);

    let start_step = args.start_step.unwrap_or(0);
    if start_step > 0 {
        log::info!("Starting from step/coord index: {}", start_step);
    }

    // Load coordinate mapper
    log::info!("Loading map data...");
    let coordinate_mapper = CoordinateMapper::load(&args.map_data)?;

    // Load run metadata from file
    log::info!("Loading compact run metadata...");
    let metadata = load_compact_runs_metadata(&args.input)?;
    log::info!("Loaded {} run metadata entries", metadata.len());

    if metadata.is_empty() {
        log::warn!("No runs to render!");
        return Ok(());
    }

    // Calculate max duration (with faster animation speed)
    let effective_interval_ms = (args.interval_ms / args.speed_multiplier) as f32;
    let max_coords = metadata.iter().map(|m| m.coord_count).max().unwrap();

    // Validate start_step
    if start_step >= max_coords {
        log::error!("start_step ({}) exceeds maximum coordinate count ({}) in runs", start_step, max_coords);
        return Ok(());
    }

    // Calculate time offset for start_step
    let start_time_offset_ms = start_step as f32 * effective_interval_ms;

    // Calculate remaining duration from start_step to end
    let remaining_coords = max_coords - start_step;
    let remaining_duration_ms = remaining_coords as f32 * effective_interval_ms;

    let mut total_frames = (remaining_duration_ms / 1000.0 * args.fps as f32).ceil() as usize;

    if let Some(max) = args.max_frames {
        if max < total_frames {
            total_frames = max;
            log::info!("Limiting to {} frames (instead of full animation from start_step)", max);
        }
    }

    log::info!("Animation: {:.2} seconds, {} frames @ {} fps (from step {} to end)",
               total_frames as f32 / args.fps as f32, total_frames, args.fps, start_step);
    log::info!("Total runs: {}", metadata.len());

    // Calculate chunk size (1/8 of max coords)
    let chunk_size = (max_coords + 7) / 8; // Round up
    let num_chunks = (max_coords + chunk_size - 1) / chunk_size;
    log::info!("Processing in {} chunks of ~{} coords each", num_chunks, chunk_size);

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
        metadata.len() + 1000,
    )?;

    // Create encoder
    log::info!("Starting video encoder...");
    let mut encoder = ProResEncoder::new(&args.output, args.width, args.height, args.fps)?;

    // Track last direction for each run (persists across chunks)
    let mut run_directions: Vec<sprite_video_renderer::data::Direction> =
        vec![sprite_video_renderer::data::Direction::Down; metadata.len()];

    // Generate random offsets [0, 1) for each run to desync their animations
    let mut rng = rand::thread_rng();
    let run_offsets: Vec<f32> = (0..metadata.len())
        .map(|_| rng.gen::<f32>())
        .collect();
    log::info!("Generated random offsets for {} runs", run_offsets.len());

    // Use sliding window to keep only necessary chunks in memory
    log::info!("Rendering {} frames with sliding chunk window...", total_frames);
    let start_time = std::time::Instant::now();

    let mut current_chunk_idx: Option<usize> = None;
    let mut loaded_runs: Vec<CompactRun> = Vec::new();
    let mut loaded_chunk_start = 0;
    let mut loaded_chunk_end = 0;

    for frame_number in 0..total_frames {
        // Add start_time_offset_ms to effectively start from start_step
        let time_ms = (frame_number as f32 * (1000.0 / args.fps as f32)) + start_time_offset_ms;

        // Determine which coord indices are accessed in this frame (approximately)
        // Use median coord index to determine which chunk to load
        let median_progress = (time_ms / effective_interval_ms) + 0.5; // Use 0.5 as median offset
        let median_coord_index = median_progress as usize;

        // Determine which chunk this falls into
        let needed_chunk_idx = (median_coord_index / chunk_size).min(num_chunks - 1);

        // Load chunk if it's not the current one
        // Add overlap of 2 coords on each side to handle random offsets
        if current_chunk_idx != Some(needed_chunk_idx) {
            loaded_chunk_start = (needed_chunk_idx * chunk_size).saturating_sub(2);
            loaded_chunk_end = (((needed_chunk_idx + 1) * chunk_size) + 2).min(max_coords);

            log::info!("Frame {}: Loading chunk {}/{} (coords [{}, {}) with overlap)",
                       frame_number, needed_chunk_idx + 1, num_chunks,
                       loaded_chunk_start, loaded_chunk_end);

            loaded_runs = load_chunk_coords(&args.input, &metadata, loaded_chunk_start, loaded_chunk_end)?;
            current_chunk_idx = Some(needed_chunk_idx);
        }

        // Calculate sprite instances for this frame
        let mut sprite_instances = Vec::new();

        for (run_idx, run) in loaded_runs.iter().enumerate() {
            // Calculate which coord index we're at (with random offset for desyncing)
            let progress = (time_ms / effective_interval_ms) + run_offsets[run_idx];
            let coord_index = progress as usize;

            let total_coords = metadata[run_idx].coord_count;
            if coord_index >= total_coords {
                continue; // This run has finished
            }

            // Check if coord_index is in currently loaded chunk
            if coord_index < loaded_chunk_start || coord_index >= loaded_chunk_end {
                log::warn!(
                    "coord index not loaded! this should not happen! coord index: {} loaded start: {} loaded end: {}",
                    coord_index, loaded_chunk_start, loaded_chunk_end
                );
                continue; // Not in loaded chunk
            }

            // Map to local coords array
            let local_coord_index = coord_index - loaded_chunk_start;
            if local_coord_index >= run.coords.len() {
                continue;
            }

            let next_index = (coord_index + 1).min(total_coords - 1);
            let local_next_index = if next_index < loaded_chunk_end {
                next_index - loaded_chunk_start
            } else {
                local_coord_index
            };
            let interpolation_t = progress.fract();

            let current_coord = &run.coords[local_coord_index];
            let next_coord = &run.coords[local_next_index];

            // Convert to i64 for coordinate mapper
            let current_coords = [current_coord.x as i64, current_coord.y as i64, current_coord.map_id as i64];
            let next_coords = [next_coord.x as i64, next_coord.y as i64, next_coord.map_id as i64];

            // Convert to pixel positions
            let current_pos = coordinate_mapper.convert_coords(&current_coords);
            let next_pos = coordinate_mapper.convert_coords(&next_coords);

            // map id doesn't exist! (probably transitioning between world)
            if current_pos == INVALID_MAP_ID_FLAG || next_pos == INVALID_MAP_ID_FLAG {
                continue;
            }

            // Check pixel distance - only interpolate if moving <= 3*16 pixels (3 tiles)
            let pixel_dx = (next_pos[0] - current_pos[0]).abs();
            let pixel_dy = (next_pos[1] - current_pos[1]).abs();
            let pixel_distance = pixel_dx + pixel_dy;

            let should_interpolate = pixel_distance <= 3.0 * 16.0;
            let interp_t = if should_interpolate { interpolation_t } else { 0.0 };

            // Interpolate position
            let position = [
                current_pos[0] + (next_pos[0] - current_pos[0]) * interp_t - 8.0,
                current_pos[1] + (next_pos[1] - current_pos[1]) * interp_t - 8.0,
            ];

            // Determine direction - only update if there's movement
            let dx = next_pos[0] - current_pos[0];
            let dy = next_pos[1] - current_pos[1];

            if dx.abs() > 0.1 || dy.abs() > 0.1 {
                // There's movement, calculate new direction
                let new_direction = if dx.abs() > dy.abs() {
                    if dx > 0.0 { sprite_video_renderer::data::Direction::Right }
                    else { sprite_video_renderer::data::Direction::Left }
                } else {
                    if dy > 0.0 { sprite_video_renderer::data::Direction::Down }
                    else { sprite_video_renderer::data::Direction::Up }
                };
                run_directions[run_idx] = new_direction;
            }

            // Get texture coordinates
            let are_we_biking_on_route_17 = current_coord.map_id == 28 || next_coord.map_id == 28;

            let sprite_index_capped = if are_we_biking_on_route_17 {
                1
            } else {
                run.sprite_id.min(54)
            };

            let tex_coords = texture_atlas.get_sprite_tex_coords(sprite_index_capped, run_directions[run_idx]);

            sprite_instances.push(SpriteInstance {
                position,
                tex_rect: tex_coords,
            });
        }

        // Debug logging for first frame
        if frame_number == 0 {
            log::info!("First frame: {} runs, {} sprites rendered",
                       metadata.len(), sprite_instances.len());

            if !sprite_instances.is_empty() {
                log::info!("Sample sprite positions (first 10):");
                for (i, instance) in sprite_instances.iter().take(10).enumerate() {
                    let in_bounds_x = instance.position[0] >= 0.0 && instance.position[0] < args.width as f32;
                    let in_bounds_y = instance.position[1] >= 0.0 && instance.position[1] < args.height as f32;
                    log::info!("  Sprite {}: pos=[{:.1}, {:.1}] in_bounds=({}, {})",
                               i, instance.position[0], instance.position[1], in_bounds_x, in_bounds_y);
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
        if frame_number % 60 == 0 || frame_number == total_frames - 1 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let fps_actual = (frame_number + 1) as f32 / elapsed;
            let progress = (frame_number + 1) as f32 / total_frames as f32 * 100.0;
            let eta = (total_frames - frame_number - 1) as f32 / fps_actual;

            log::info!(
                "Progress: {:.1}% ({}/{}) | {:.1} fps | ETA: {:.1}s | Sprites: {} | Chunk: {}/{}",
                progress,
                frame_number + 1,
                total_frames,
                fps_actual,
                eta,
                sprite_instances.len(),
                current_chunk_idx.map(|i| i + 1).unwrap_or(0),
                num_chunks
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

    Ok(())
}

fn load_compact_runs_metadata(path: &PathBuf) -> Result<Vec<CompactRunMetadata>> {
    let mut reader: Box<dyn Read> = if path.extension().and_then(|s| s.to_str()) == Some("zst") {
        // Decompress
        log::info!("Decompressing zstd file...");
        let file = File::open(path)?;
        Box::new(zstd::Decoder::new(file)?)
    } else {
        // Read uncompressed
        let file = File::open(path)?;
        Box::new(BufReader::new(file))
    };

    let mut metadata = Vec::new();
    let mut current_offset: u64 = 0;
    let mut all_sprite_ids = HashSet::new();

    loop {
        // Read sprite_id
        let mut sprite_id_buf = [0u8; 1];
        match reader.read_exact(&mut sprite_id_buf) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let sprite_id = sprite_id_buf[0];
        all_sprite_ids.insert(sprite_id);
        current_offset += 1;

        // Read coord_count
        let mut count_buf = [0u8; 2];
        reader.read_exact(&mut count_buf)?;
        let coord_count = u16::from_le_bytes(count_buf) as usize;
        current_offset += 2;

        // Store metadata with the offset where coords start
        let coords_offset = current_offset;
        metadata.push(CompactRunMetadata {
            sprite_id,
            coord_count,
            file_offset: coords_offset,
        });

        // Skip over the coordinate data
        let bytes_to_skip = coord_count * std::mem::size_of::<UltraCompactCoordMem>();
        let mut skip_buffer = vec![0u8; bytes_to_skip];
        reader.read_exact(&mut skip_buffer)?;
        current_offset += bytes_to_skip as u64;

        if metadata.len() % 100000 == 0 {
            log::info!("Loaded {} run metadata entries", metadata.len());
        }
    }

    Ok(metadata)
}

fn load_chunk_coords(
    path: &PathBuf,
    metadata: &[CompactRunMetadata],
    chunk_start: usize,
    chunk_end: usize,
) -> Result<Vec<CompactRun>> {
    let mut reader: Box<dyn Read> = if path.extension().and_then(|s| s.to_str()) == Some("zst") {
        // For compressed files, we need to read from the beginning
        let file = File::open(path)?;
        Box::new(zstd::Decoder::new(file)?)
    } else {
        let file = File::open(path)?;
        Box::new(BufReader::new(file))
    };

    let mut runs = Vec::with_capacity(metadata.len());
    let mut current_file_pos: u64 = 0;
    let mut buffer = vec![0u8; 1024 * 1024];

    for meta in metadata {
        // Skip to this run's data if needed
        if current_file_pos < meta.file_offset {
            let bytes_to_skip = meta.file_offset - current_file_pos;
            let mut skip_buf = vec![0u8; bytes_to_skip as usize];
            reader.read_exact(&mut skip_buf)?;
            current_file_pos = meta.file_offset;
        }

        // Calculate which coords to load for this run
        let run_chunk_start = chunk_start.min(meta.coord_count);
        let run_chunk_end = chunk_end.min(meta.coord_count);
        let coords_to_load = if run_chunk_start < run_chunk_end {
            run_chunk_end - run_chunk_start
        } else {
            0
        };

        // Skip coords before chunk_start
        let skip_before = run_chunk_start * std::mem::size_of::<UltraCompactCoordMem>();
        if skip_before > 0 {
            let mut skip_buf = vec![0u8; skip_before];
            reader.read_exact(&mut skip_buf)?;
            current_file_pos += skip_before as u64;
        }

        // Load coords in this chunk
        let mut coords = Vec::with_capacity(coords_to_load);
        if coords_to_load > 0 {
            let bytes_to_read = coords_to_load * std::mem::size_of::<UltraCompactCoordMem>();
            if buffer.len() < bytes_to_read {
                buffer.resize(bytes_to_read, 0);
            }
            reader.read_exact(&mut buffer[..bytes_to_read])?;
            current_file_pos += bytes_to_read as u64;

            for i in 0..coords_to_load {
                let offset = i * std::mem::size_of::<UltraCompactCoordMem>();
                let coord = unsafe {
                    std::ptr::read_unaligned(buffer[offset..].as_ptr() as *const UltraCompactCoordMem)
                };
                coords.push(coord);
            }
        }

        // Skip coords after chunk_end
        let skip_after = (meta.coord_count - run_chunk_end) * std::mem::size_of::<UltraCompactCoordMem>();
        if skip_after > 0 {
            let mut skip_buf = vec![0u8; skip_after];
            reader.read_exact(&mut skip_buf)?;
            current_file_pos += skip_after as u64;
        }

        runs.push(CompactRun {
            sprite_id: meta.sprite_id,
            coords,
        });
    }

    Ok(runs)
}

