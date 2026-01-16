use anyhow::Result;
use clap::Parser;
use sprite_video_renderer::data::{ParquetFilter, ParquetReader, SpriteFrame};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Analyze user+env_id runs in parquet file", long_about = None)]
struct Args {
    /// Path to parquet file
    #[arg(long)]
    parquet_file: PathBuf,
}

#[derive(Debug)]
struct Run {
    start_timestamp: chrono::DateTime<chrono::Utc>,
    end_timestamp: chrono::DateTime<chrono::Utc>,
    coord_count: usize,
}

#[derive(Debug)]
struct UserEnvStats {
    user: String,
    env_id: String,
    runs: Vec<Run>,
    total_coords: usize,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    log::info!("=== Analyzing runs in parquet file ===");
    log::info!("Parquet file: {:?}", args.parquet_file);

    // Read all frames from parquet
    log::info!("Reading parquet file...");
    let reader = ParquetReader::new(ParquetFilter::default());
    let frames = reader.read_file(&args.parquet_file)?;

    log::info!("Total frames read: {}", frames.len());

    // Group by user+env_id directly and build run splits in one pass
    log::info!("Grouping and detecting runs...");
    let mut user_env_data: HashMap<String, Vec<SpriteFrame>> = HashMap::new();

    for frame in frames {
        let key = format!("{}-{}", frame.user, frame.env_id);
        user_env_data.entry(key).or_insert_with(Vec::new).push(frame);
    }

    log::info!("Found {} unique user+env_id pairs", user_env_data.len());

    // Process each user+env_id to identify runs with reset detection
    let mut stats: Vec<UserEnvStats> = Vec::new();
    let reset_maps = vec![0i64, 37, 40];

    for (_key, mut frames_list) in user_env_data {
        if frames_list.is_empty() {
            continue;
        }

        // Sort by timestamp
        frames_list.sort_by_key(|f| (f.timestamp, f.path_index));

        let user = frames_list[0].user.clone();
        let env_id = frames_list[0].env_id.clone();
        let total_coords = frames_list.len();

        // Detect runs with 2-minute gaps and reset events
        let runs = detect_runs_with_resets(&frames_list, &reset_maps);

        stats.push(UserEnvStats {
            user,
            env_id,
            runs,
            total_coords,
        });
    }

    // Filter out runs < 60 seconds
    let min_duration = chrono::Duration::seconds(60);
    let mut total_runs_before = 0;
    for stat in &mut stats {
        total_runs_before += stat.runs.len();
        stat.runs.retain(|run| {
            (run.end_timestamp - run.start_timestamp) >= min_duration
        });
    }

    log::info!("Runs before 60s filter: {}", total_runs_before);
    log::info!("Runs after 60s filter: {}", stats.iter().map(|s| s.runs.len()).sum::<usize>());

    // Remove user+env_id pairs with no runs left after filtering
    stats.retain(|s| !s.runs.is_empty());

    // Recalculate total coords after filtering
    for stat in &mut stats {
        stat.total_coords = stat.runs.iter().map(|r| r.coord_count).sum();
    }

    // Sort by total coords descending for easier reading
    stats.sort_by_key(|s| std::cmp::Reverse(s.total_coords));

    // Print summary
    println!("\n=== SUMMARY (after filtering runs < 60s) ===");
    println!("Total unique user+env_id pairs: {}", stats.len());
    println!("Total runs across all pairs: {}", stats.iter().map(|s| s.runs.len()).sum::<usize>());
    println!();

    // Print detailed stats (limit to first 50 to avoid huge output)
    println!("=== DETAILED BREAKDOWN (top 50) ===\n");

    for stat in stats.iter().take(50) {
        println!("User: {}, Env ID: {}", stat.user, stat.env_id);
        println!("  Total coords: {}", stat.total_coords);
        println!("  Number of runs: {}", stat.runs.len());

        for (i, run) in stat.runs.iter().enumerate() {
            let duration = run.end_timestamp - run.start_timestamp;
            println!("    Run {}: {} coords, duration: {:.1}s ({}  to  {})",
                     i + 1,
                     run.coord_count,
                     duration.num_milliseconds() as f64 / 1000.0,
                     run.start_timestamp.format("%Y-%m-%d %H:%M:%S"),
                     run.end_timestamp.format("%Y-%m-%d %H:%M:%S"));
        }
        println!();
    }

    Ok(())
}

/// Detect runs based on 2-minute gaps and reset events
/// This processes frames in a single pass - O(n)
fn detect_runs_with_resets(
    frames: &[SpriteFrame],
    reset_maps: &[i64],
) -> Vec<Run> {
    if frames.is_empty() {
        return Vec::new();
    }

    let mut runs = Vec::new();
    let gap_threshold = chrono::Duration::minutes(2);

    let mut run_start_idx = 0;

    for i in 1..frames.len() {
        let time_gap = frames[i].timestamp - frames[i-1].timestamp;
        let curr_map = frames[i].coords[2];
        let prev_map = frames[i-1].coords[2];

        let mut should_split = false;

        // Split on 2-minute gaps
        if time_gap >= gap_threshold {
            should_split = true;
        }

        // Split when jumping TO a reset map (0, 37, 40) from a different map
        if reset_maps.contains(&curr_map) && !reset_maps.contains(&prev_map) {
            should_split = true;
        }

        if should_split {
            let run = Run {
                start_timestamp: frames[run_start_idx].timestamp,
                end_timestamp: frames[i-1].timestamp,
                coord_count: i - run_start_idx,
            };
            runs.push(run);
            run_start_idx = i;
        }
    }

    // Add the final run
    let final_run = Run {
        start_timestamp: frames[run_start_idx].timestamp,
        end_timestamp: frames[frames.len() - 1].timestamp,
        coord_count: frames.len() - run_start_idx,
    };
    runs.push(final_run);

    runs
}
