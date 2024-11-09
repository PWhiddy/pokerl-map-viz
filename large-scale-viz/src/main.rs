use std::collections::HashMap;
use std::fs::File;
use std::io::{self};
use std::path::Path;

use chrono::{DateTime, NaiveDateTime, Utc};
use std::str::FromStr;

use csv::ReaderBuilder;
use indicatif::ProgressBar;
use serde::Deserialize;
use serde_json::Value;
use image::{ImageBuffer, Rgb};

#[derive(Debug, Deserialize)]
struct Record {
    timestamp: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct MapData {
    regions: Vec<Region>,
}

#[derive(Debug, Deserialize)]
struct Region {
    id: String,
    coordinates: [i64; 2],
}

const DIM: usize = 768;
const SAVE_INTERVAL_SECS: u64 = 12 * 60; // 12 minutes in seconds

fn main() {
    // Read the map data from map_data.json
    let file = File::open("../assets/map_data.json").expect("Failed to open map_data.json");
    let map_data: MapData = serde_json::from_reader(file).expect("Failed to parse map_data.json");

    // Create a hashmap for region coordinate lookups
    let mut region_map: HashMap<i64, [i64; 2]> = HashMap::new();
    for region in map_data.regions {
        region_map.insert(region.id.parse::<i64>().unwrap(), region.coordinates);
    }

    // Create a buffer reader from the standard input
    let stdin = io::stdin();
    let reader = stdin.lock();

    // Initialize counters
    let mut row_count: u64 = 0;
    let mut total_coords: u64 = 0;
    let mut failed_rows: u64 = 0;
    let mut img_count: u64 = 0;

    // Create a progress bar
    let progress_bar = ProgressBar::new_spinner();

    // Set initial state for progress bar
    progress_bar.set_message(format!("rows: {} coords: {} failed: {}", row_count, total_coords, failed_rows));

    // Create a CSV reader
    let mut csv_reader = ReaderBuilder::new().has_headers(true).from_reader(reader);

    // Create arrays for coordinate counts
    let mut coord_counts_full = vec![vec![0u64; DIM]; DIM];
    let mut coord_counts_medium = vec![vec![0u64; DIM]; DIM];
    let mut coord_counts_fast = vec![vec![0u64; DIM]; DIM];
    let mut coord_counts_extra_fast = vec![vec![0u64; DIM]; DIM];

    let mut last_save_time = None;

    // Iterate over each record in the CSV
    for result in csv_reader.deserialize() {
        row_count += 1;

        let record: Record = match result {
            Ok(record) => record,
            Err(_) => {
                failed_rows += 1;
                continue;
            }
        };


        let timestamp = DateTime::<Utc>::from_utc(
            NaiveDateTime::parse_from_str(&record.timestamp, "%Y-%m-%dT%H:%M:%S%.f")
                .expect("Invalid timestamp format"),
                    Utc,
        );

        if last_save_time.is_none() {
            last_save_time = Some(timestamp);
        }

        // Parse the JSON message
        if let Ok(json_value) = serde_json::from_str::<Value>(&record.message) {
            if let Some(coords_array) = json_value.get("coords").and_then(|v| v.as_array()) {
                for coords in coords_array {
                    if let Some(coords) = coords.as_array() {
                        if coords.len() == 3 {
                            let x = coords[0].as_i64().unwrap_or(-1);
                            let y = coords[1].as_i64().unwrap_or(-1);
                            let map_id = coords[2].as_i64().unwrap_or(6666);

                            if let Some(&offsets) = region_map.get(&map_id) {
                                let global_x = x + offsets[0];
                                let global_y = y + offsets[1];

                                if global_x >= 0 && global_x < DIM as i64 && global_y >= 0 && global_y < DIM as i64 {
                                    coord_counts_full[global_x as usize][global_y as usize] += 1;
                                    coord_counts_medium[global_x as usize][global_y as usize] += 1;
                                    coord_counts_fast[global_x as usize][global_y as usize] += 1;
                                    coord_counts_extra_fast[global_x as usize][global_y as usize] += 1;
                                    total_coords += 1;
                                } else {
                                    println!("bad coords {} {}", global_x, global_y);
                                    failed_rows += 1;
                                }
                            } else {
                                println!("bad map id {}", map_id);
                                failed_rows += 1;
                            }
                        } else {
                            println!("bad coords.len() {}", coords.len());
                            failed_rows += 1;
                        }
                    } else {
                        println!("no array within coords");
                        failed_rows += 1;
                    }
                }
            } else {
                failed_rows += 1;
            }
        } else {
            failed_rows += 1;
        }

        // Check if 12 minutes have passed since the last save
        if let Some(last_save) = last_save_time {
            let elapsed = timestamp.signed_duration_since(last_save).num_seconds();
            if elapsed >= SAVE_INTERVAL_SECS as i64 {
                save_map_as_image("full", u32::pow(2, 26), &coord_counts_full, row_count, img_count);
                save_map_as_image("medium", u32::pow(2, 22), &coord_counts_medium, row_count, img_count);
                save_map_as_image("fast", u32::pow(2, 18), &coord_counts_fast, row_count, img_count);
                save_map_as_image("extra_fast", u32::pow(2, 16), &coord_counts_extra_fast, row_count, img_count);
                img_count += 1; 
                last_save_time = Some(timestamp);

                for row in coord_counts_medium.iter_mut() {
                    for pix in row.iter_mut() {
                        *pix = ((*pix as f64) * 0.99) as u64;
                    }
                }
                for row in coord_counts_fast.iter_mut() {
                    for pix in row.iter_mut() {
                        *pix = ((*pix as f64) * 0.9) as u64;
                    }
                }
                for row in coord_counts_extra_fast.iter_mut() {
                    for pix in row.iter_mut() {
                        *pix = ((*pix as f64) * 0.5) as u64;
                    }
                }
            }
        }

        progress_bar.set_message(format!("rows: {} coords: {} failed: {} timestamp: {}", row_count, total_coords, failed_rows, timestamp));
    }

    println!("rows: {} coords: {} failed: {}", row_count, total_coords, failed_rows);
}

fn save_map_as_image(name: &str, max_count: u32, coord_counts: &Vec<Vec<u64>>, row_count: u64, img_count: u64) {
    let cur_max_pixel = coord_counts.iter().flatten().max().cloned().unwrap_or(1);
    println!("{} image: {} true max pixel: {} using max {} ", name, img_count, cur_max_pixel, max_count);

    let mut img: ImageBuffer<Rgb<f32>, Vec<f32>> = ImageBuffer::new(DIM as u32, DIM as u32);

    for (x, row) in coord_counts.iter().enumerate() {
        for (y, &count) in row.iter().enumerate() {
            let intensity = (f64::min((count as f64) / (max_count as f64), 1.0)) as f32;
            img.put_pixel(x as u32, y as u32, Rgb([intensity, intensity, intensity]));
        }
    }

    let filename = format!("images/coord_map_{name}_{}.exr", img_count);
    img.save_with_format(Path::new(&filename), image::ImageFormat::OpenExr)
        .expect("Failed to save image");
}
