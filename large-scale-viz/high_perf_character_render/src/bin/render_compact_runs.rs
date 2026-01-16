use anyhow::{Context, Result};
use clap::Parser;
use sprite_video_renderer::data::CoordinateMapper;
use sprite_video_renderer::rendering::{GpuContext, SpriteInstance, SpriteRenderer, TextureAtlas};
use sprite_video_renderer::video::ProResEncoder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

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

    /// Maximum number of frames to render (for testing)
    #[arg(long)]
    max_frames: Option<usize>,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct CompactCoord {
    x: u16,
    y: u16,
    map_id: u16,
}

#[derive(Debug)]
struct CompactRun {
    sprite_id: u8,
    coords: Vec<CompactCoord>,
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

    // Load coordinate mapper
    log::info!("Loading map data...");
    let coordinate_mapper = CoordinateMapper::load(&args.map_data)?;

    // Load runs from file
    log::info!("Loading compact runs...");
    let runs = load_compact_runs(&args.input)?;
    log::info!("Loaded {} runs", runs.len());

    if runs.is_empty() {
        log::warn!("No runs to render!");
        return Ok(());
    }

    // Calculate max duration (with faster animation speed)
    let effective_interval_ms = (args.interval_ms / args.speed_multiplier) as f32;
    let max_coords = runs.iter().map(|r| r.coords.len()).max().unwrap();
    let max_duration_ms = max_coords as f32 * effective_interval_ms;

    let mut total_frames = (max_duration_ms / 1000.0 * args.fps as f32).ceil() as usize;

    if let Some(max) = args.max_frames {
        if max < total_frames {
            total_frames = max;
            log::info!("Limiting to {} frames (instead of full animation)", max);
        }
    }

    log::info!("Animation: {:.2} seconds, {} frames @ {} fps",
               total_frames as f32 / args.fps as f32, total_frames, args.fps);
    log::info!("Total runs: {}", runs.len());

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
        runs.len() + 1000,
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
            // Calculate which coord index we're at (using all coords, just faster)
            let coord_index = (time_ms / effective_interval_ms) as usize;

            if coord_index >= run.coords.len() {
                continue; // This run has finished
            }

            let next_index = (coord_index + 1).min(run.coords.len() - 1);
            let interpolation_t = (time_ms / effective_interval_ms).fract();

            let current_coord = &run.coords[coord_index];
            let next_coord = &run.coords[next_index];

            // Convert to i64 for coordinate mapper
            let current_coords = [current_coord.x as i64, current_coord.y as i64, current_coord.map_id as i64];
            let next_coords = [next_coord.x as i64, next_coord.y as i64, next_coord.map_id as i64];

            // Convert to pixel positions
            let current_pos = coordinate_mapper.convert_coords(&current_coords);
            let next_pos = coordinate_mapper.convert_coords(&next_coords);

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

    Ok(())
}

fn load_compact_runs(path: &PathBuf) -> Result<Vec<CompactRun>> {
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

    let mut runs = Vec::new();
    let mut buffer = vec![0u8; 1024 * 1024]; // 1MB buffer for reading

    loop {
        // Read sprite_id
        let mut sprite_id_buf = [0u8; 1];
        match reader.read_exact(&mut sprite_id_buf) {
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let sprite_id = sprite_id_buf[0];

        // Read coord_count
        let mut count_buf = [0u8; 2];
        reader.read_exact(&mut count_buf)?;
        let coord_count = u16::from_le_bytes(count_buf) as usize;

        // Read coords
        let bytes_to_read = coord_count * std::mem::size_of::<CompactCoord>();
        if buffer.len() < bytes_to_read {
            buffer.resize(bytes_to_read, 0);
        }

        reader.read_exact(&mut buffer[..bytes_to_read])?;

        let mut coords = Vec::with_capacity(coord_count);
        for i in 0..coord_count {
            let offset = i * std::mem::size_of::<CompactCoord>();
            let coord = unsafe {
                std::ptr::read_unaligned(buffer[offset..].as_ptr() as *const CompactCoord)
            };
            coords.push(coord);
        }

        runs.push(CompactRun { sprite_id, coords });

        if runs.len() % 100000 == 0 {
            log::info!("Loaded {} runs...", runs.len());
        }
    }

    Ok(runs)
}
