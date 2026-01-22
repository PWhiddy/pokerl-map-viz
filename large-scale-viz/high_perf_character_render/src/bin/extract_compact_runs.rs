use anyhow::{Context, Result};
use clap::Parser;
use sprite_video_renderer::data::{ParquetFilter, CoordinateMapper, ParquetReader};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use chrono::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract compact runs from parquet files", long_about = None)]
struct Args {
    /// Directory containing parquet files
    #[arg(long)]
    parquet_dir: PathBuf,

    /// Output file for compact runs
    #[arg(long, default_value = "compact_runs.bin")]
    output: PathBuf,

    /// Progress file to track processed files
    #[arg(long, default_value = "compact_runs.progress")]
    progress_file: PathBuf,

    /// Minimum run duration in seconds
    /// 60 secs might be around 1200 steps.
    /// for longer run we should do at least 240 seconds
    #[arg(long, default_value = "60")]
    min_duration_secs: i64,

    /// Maximum coordinates per run
    // this gets converted to u16 so 2^16 is max safe value! 
    // 65528 <- safe value with 8 padding
    // lets try 32768
    #[arg(long, default_value = "2000")]
    max_coords_per_run: usize,

    #[arg(long)]
    pallet_start_only: bool,

}

/*
// original, bigger than needed
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
struct UltraCompactCoord {
    x: u8,
    y: u8,
    map_id: u8,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("=== Extracting compact runs from parquet files ===");
    log::info!("Parquet directory: {:?}", args.parquet_dir);
    log::info!("Output file: {:?}", args.output);
    log::info!("Max coords per run: {}", args.max_coords_per_run);

    // Find all parquet files
    let mut parquet_files: Vec<PathBuf> = std::fs::read_dir(&args.parquet_dir)
        .context("Failed to read parquet directory")?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() && path.extension()? == "parquet" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    parquet_files.sort();
    log::info!("Found {} parquet files", parquet_files.len());

    // Load progress - which files have been processed
    let mut processed_files = std::collections::HashSet::new();
    if args.progress_file.exists() {
        let file = File::open(&args.progress_file)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            processed_files.insert(line?);
        }
        log::info!("Loaded progress: {} files already processed", processed_files.len());
    }

    let coordinate_mapper = CoordinateMapper::load("../../assets/map_data.json").unwrap();

    // Open output file in append mode
    let mut output_file = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&args.output)?
    );

    // Open progress file in append mode
    let mut progress_writer = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&args.progress_file)?
    );

    let mut total_runs_extracted = 0;
    let starting_maps = vec![0i64, 37, 40, 38];
    let starting_and_adjacent_maps = vec![0i64, 37, 40, 39, 38, 12, 32];
    let gap_threshold = Duration::minutes(2);
    let min_duration = Duration::seconds(args.min_duration_secs);

    // Process each parquet file
    for (file_idx, parquet_path) in parquet_files.iter().enumerate() {
        let file_name = parquet_path.file_name().unwrap().to_string_lossy().to_string();

        // Skip if already processed
        if processed_files.contains(&file_name) {
            log::debug!("Skipping already processed: {}", file_name);
            continue;
        }

        log::info!("Processing [{}/{}]: {}", file_idx + 1, parquet_files.len(), file_name);

        // Read parquet file
        let reader = ParquetReader::new(ParquetFilter::default());
        let mut frames = match reader.read_file(parquet_path) {
            Ok(f) => f,
            Err(e) => {
                log::error!("Failed to read {}: {}", file_name, e);
                continue;
            }
        };

        if frames.is_empty() {
            log::warn!("No frames in {}", file_name);
            writeln!(progress_writer, "{}", file_name)?;
            progress_writer.flush()?;
            continue;
        }

        // Sort frames
        frames.sort_by(|a, b| {
            (&a.user, &a.env_id, a.timestamp, a.path_index)
                .cmp(&(&b.user, &b.env_id, b.timestamp, b.path_index))
        });

        // Extract runs
        let mut file_runs_count = 0;
        let mut i = 0;

        while i < frames.len() {
            let run_user = &frames[i].user;
            let run_env_id = &frames[i].env_id;
            let run_sprite_id = frames[i].sprite_id;

            let user_env_start = i;

            // Find all frames for this user+env_id
            while i < frames.len() && &frames[i].user == run_user && &frames[i].env_id == run_env_id {
                i += 1;
            }

            let user_env_end = i;

            // Split into runs
            let mut run_start = user_env_start;

            for j in (user_env_start + 1)..user_env_end {
                let time_gap = frames[j].timestamp - frames[j-1].timestamp;
                let curr_map = frames[j].coords[2];
                let prev_map = frames[j-1].coords[2];

                //////////////////////////
                let current_coord = frames[j].coords.map(|x| x as i64);
                let previous_coord = frames[j-1].coords.map(|x| x as i64);

                // Convert to pixel positions
                let current_global_pos = coordinate_mapper.convert_coords(&current_coord);
                let previous_global_pos = coordinate_mapper.convert_coords(&previous_coord);
                let dx = current_global_pos[0] - previous_global_pos[0];
                let dy = current_global_pos[1] - previous_global_pos[1];
                let global_step_delta = (dx*dx + dy*dy).sqrt();
                let step_count = j as i64 - run_start as i64;
                let exact_start_compatible = true;//step_count > 1 || (previous_coord[0] == 5 && previous_coord[1] == 3 && previous_coord[2] == 40);
                let early_big_jump_fail = args.pallet_start_only && (!exact_start_compatible || (step_count < 140 && global_step_delta > 30.0));
                ///////////////

                let should_split = time_gap >= gap_threshold || early_big_jump_fail
                    || (starting_maps.contains(&curr_map) && !starting_and_adjacent_maps.contains(&prev_map));

                if should_split {
                    let duration = frames[j-1].timestamp - frames[run_start].timestamp;
                    let pallet_start_ok = if args.pallet_start_only { starting_maps.contains(&frames[run_start].coords[2]) } else { true };
                    if duration >= min_duration && pallet_start_ok && !early_big_jump_fail {
                        // Write this run
                        write_compact_run(
                            &mut output_file,
                            run_sprite_id,
                            &frames[run_start..j],
                            args.max_coords_per_run,
                        )?;
                        file_runs_count += 1;
                        total_runs_extracted += 1;
                    }

                    run_start = j;
                }
            }

            // Final run
            if run_start < user_env_end {
                let duration = frames[user_env_end - 1].timestamp - frames[run_start].timestamp;
                let pallet_start_ok = if args.pallet_start_only { starting_maps.contains(&frames[run_start].coords[2]) } else { true };
                
                if duration >= min_duration && pallet_start_ok {
                    write_compact_run(
                        &mut output_file,
                        run_sprite_id,
                        &frames[run_start..user_env_end],
                        args.max_coords_per_run,
                    )?;
                    file_runs_count += 1;
                    total_runs_extracted += 1;
                }
            }
        }

        log::info!("  Extracted {} runs from {}", file_runs_count, file_name);

        // Mark file as processed
        writeln!(progress_writer, "{}", file_name)?;
        progress_writer.flush()?;

        // Periodic flush of output
        output_file.flush()?;
    }

    output_file.flush()?;
    progress_writer.flush()?;

    log::info!("=== Extraction complete ===");
    log::info!("Total runs extracted: {}", total_runs_extracted);
    log::info!("Output written to: {:?}", args.output);

    // Compress the output file
    log::info!("Compressing output...");
    compress_file(&args.output)?;
    log::info!("Compressed to: {:?}.zst", args.output);

    Ok(())
}

fn write_compact_run<W: Write>(
    writer: &mut W,
    sprite_id: u8,
    frames: &[sprite_video_renderer::data::SpriteFrame],
    max_coords: usize,
) -> Result<()> {
    let coord_count = frames.len().min(max_coords) as u16;

    // Write sprite_id
    writer.write_all(&[sprite_id])?;

    // Write coord_count
    writer.write_all(&coord_count.to_le_bytes())?;

    // Write coords
    for frame in frames.iter().take(max_coords) {

        // flag out invalid map id or coordinates
        let compact = match (
            u8::try_from(frame.coords[0]),
            u8::try_from(frame.coords[1]),
            u8::try_from(frame.coords[2]),
        ) {
            (Ok(x), Ok(y), Ok(map_id)) => UltraCompactCoord { x, y, map_id },
            _ => UltraCompactCoord {
                x: 0,
                y: 0,
                map_id: 255,
            },
        };

        let bytes = unsafe {
            std::slice::from_raw_parts(
                &compact as *const UltraCompactCoord as *const u8,
                std::mem::size_of::<UltraCompactCoord>(),
            )
        };

        writer.write_all(bytes)?;
    }

    Ok(())
}

fn compress_file(path: &PathBuf) -> Result<()> {
    let input = File::open(path)?;
    let mut reader = BufReader::new(input);

    let output_path = path.with_extension("bin.zst");
    let output = File::create(&output_path)?;
    let mut encoder = zstd::Encoder::new(output, 3)?; // Compression level 3 (fast)

    std::io::copy(&mut reader, &mut encoder)?;
    encoder.finish()?;

    log::info!("Original size: {} MB", path.metadata()?.len() / 1_000_000);
    log::info!("Compressed size: {} MB", output_path.metadata()?.len() / 1_000_000);

    Ok(())
}
