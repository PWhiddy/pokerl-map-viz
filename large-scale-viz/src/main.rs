use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::Path;

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

const SAVE_INTERVAL: u64 = 20_000;

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

    // Create a progress bar
    let progress_bar = ProgressBar::new_spinner();

    // Set initial state for progress bar
    progress_bar.set_message(format!("rows: {} coords: {} failed: {}", row_count, total_coords, failed_rows));

    // Create a CSV reader
    let mut csv_reader = ReaderBuilder::new().has_headers(true).from_reader(reader);

    // Create an array for coordinate counts
    let mut coord_counts_full = vec![vec![0u64; DIM]; DIM];
    let mut coord_counts_medium = vec![vec![0u64; DIM]; DIM];
    let mut coord_counts_fast = vec![vec![0u64; DIM]; DIM];

    // Iterate over each record in the CSV
    for result in csv_reader.deserialize() {
        // Increment the row counter
        row_count += 1;

        // Handle the result
        match result {
            Ok(record) => {
                let record: Record = record;

                // Parse the JSON message
                match serde_json::from_str::<Value>(&record.message) {
                    Ok(json_value) => {
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
                    }
                    Err(_) => {
                        failed_rows += 1;
                    }
                }
            }
            Err(_) => {
                failed_rows += 1;
            }
        }

        // Update progress bar messages
        let message = format!("rows: {} coords: {} failed: {}", row_count, total_coords, failed_rows);
        if row_count % 10000000 == 0 {
            println!("{}", message);
        }
        progress_bar.set_message(message);

        // Save the map every 1M coordinate rows
        if row_count % SAVE_INTERVAL == 0 {
            save_map_as_image("full", u32::pow(2, 24), &coord_counts_full, row_count);
            save_map_as_image("medium", u32::pow(2, 20), &coord_counts_medium, row_count);
            save_map_as_image("fast", u32::pow(2, 16), &coord_counts_fast, row_count);
            // coord_counts = vec![vec![0u64; DIM]; DIM];
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
        }
    }

    // Save the final map

    //save_map_as_image(&coord_counts_full, row_count);


    println!("rows: {} coords: {} failed: {}", row_count, total_coords, failed_rows);
}


fn save_map_as_image(name: &str, max_count: u32, coord_counts: &Vec<Vec<u64>>, row_count: u64) {
    let cur_max_pixel = coord_counts.iter().flatten().max().cloned().unwrap_or(1);
    println!("{} image: {} true max pixel: {} using max {} ", name, row_count / SAVE_INTERVAL, cur_max_pixel, max_count);

    let mut img: ImageBuffer<Rgb<f32>, Vec<f32>> = ImageBuffer::new(DIM as u32, DIM as u32);

    for (x, row) in coord_counts.iter().enumerate() {
        for (y, &count) in row.iter().enumerate() {
            // Scale count to fit in u16 range
            let intensity = (f64::min((count as f64) / (max_count as f64), 1.0)) as f32;// * u16::MAX as f64) as u16;
            img.put_pixel(x as u32, y as u32, Rgb([intensity, intensity, intensity]));
        }
    }

    let filename = format!("images/coord_map_{name}_{}.exr", row_count / SAVE_INTERVAL);
    img.save_with_format(Path::new(&filename), image::ImageFormat::OpenExr)
        .expect("Failed to save image");
}
