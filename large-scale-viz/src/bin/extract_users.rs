use std::collections::HashMap; // Changed from HashSet
use std::fs::File;
use std::io::{self, Write, BufWriter}; // Added Write and BufWriter
use std::error::Error; // Added for error handling

use csv::{ReaderBuilder, Writer}; // Added Writer
use indicatif::ProgressBar;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Record {
    timestamp: String,
    message: String,
}

// Using a struct for writing might be cleaner if more fields are added later,
// but for just two fields, manual writing is straightforward.
// #[derive(Serialize)] // Need Serialize for this approach
// struct UserCoordOutput<'a> {
//     username: &'a str,
//     coord_count: u64,
// }

const DISPLAY_INTERVAL: u64 = 20_000;
const OUTPUT_FILENAME: &str = "user_coords.csv";

// Change main to return a Result for easier error handling with `?`
fn main() -> Result<(), Box<dyn Error>> {
    // Create a buffer reader from the standard input
    let stdin = io::stdin();
    let reader = stdin.lock();

    // Initialize counters
    let mut row_count: u64 = 0;
    let mut failed_rows: u64 = 0;

    // Use a HashMap to store username -> coordinate count
    let mut user_coord_counts: HashMap<String, u64> = HashMap::new();

    // Create a progress bar
    let progress_bar = ProgressBar::new_spinner();

    // Set initial state for progress bar
    let update_progress = |pb: &ProgressBar, rc: u64, uc: usize, fr: u64| {
        let message = format!(
            "rows: {} unique users: {} failed: {}",
            rc, uc, fr
        );
        if rc % 10_000_000 == 0 && rc > 0 {
             // Print intermediate summary less intrusively
             eprintln!("\nProgress: {}", message); // Use eprintln to not interfere with stdout if needed
        }
        pb.set_message(message);
    };

    update_progress(&progress_bar, row_count, user_coord_counts.len(), failed_rows);


    // Create a CSV reader
    let mut csv_reader = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(reader);

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
                        if let Some(metadata) = json_value.get("metadata") {
                            if let Some(username) = metadata.get("user").and_then(|u| u.as_str()) {
                                // Increment the count for this user
                                let count = user_coord_counts.entry(username.to_string()).or_insert(0);
                                *count += 1;
                            } else {
                                failed_rows += 1; // Missing or invalid user field
                            }
                        } else {
                            failed_rows += 1; // Missing metadata field
                        }
                    }
                    Err(_) => {
                        failed_rows += 1; // Failed JSON parsing
                    }
                }
            }
            Err(_) => {
                failed_rows += 1; // Failed CSV record deserialization
            }
        }

        // Update progress bar message periodically to avoid overhead
        if row_count % 1000 == 0 { // Update message less frequently
           update_progress(&progress_bar, row_count, user_coord_counts.len(), failed_rows);
           progress_bar.tick(); // Ensure spinner updates visually
        }


        // Display current list of usernames and counts periodically
        if row_count % DISPLAY_INTERVAL == 0 && row_count > 0 {
             // Use eprintln to avoid interfering with potential stdout usage later
            eprintln!("\nCurrent unique users ({}) and counts:", user_coord_counts.len());
            let mut sorted_users: Vec<_> = user_coord_counts.iter().collect();
            // Sort by username for consistent display
            sorted_users.sort_by_key(|(k, _)| *k);
            for (username, count) in sorted_users {
                eprintln!("  - {}: {}", username, count);
            }
            eprintln!("\nProgress: rows: {} unique users: {} failed: {}",
                row_count,
                user_coord_counts.len(),
                failed_rows);
        }
    }

    progress_bar.finish_with_message(format!(
        "Processing finished. rows: {} unique users: {} failed: {}",
        row_count,
        user_coord_counts.len(),
        failed_rows
    ));

    // Display final statistics
    println!("\n--- Final Statistics ---");
    println!("Total rows processed: {}", row_count);
    println!("Total unique users found: {}", user_coord_counts.len());
    println!("Total failed rows/parses: {}", failed_rows);

    // --- Write results to CSV file ---
    println!("\nWriting user coordinate counts to {}...", OUTPUT_FILENAME);
    { // Scope for file and writer
        let file = File::create(OUTPUT_FILENAME)?;
        let mut wtr = Writer::from_writer(BufWriter::new(file)); // Use BufWriter for efficiency

        // Write header
        wtr.write_record(&["username", "coord_count"])?;

        // Sort users by username before writing for consistent output
        let mut sorted_users: Vec<_> = user_coord_counts.into_iter().collect();
        sorted_users.sort_by(|a, b| a.0.cmp(&b.0)); // Sort by username (the String key)

        // Write data rows
        for (username, count) in sorted_users {
             // Convert count to string for writing
            wtr.write_record(&[&username, &count.to_string()])?;
            // Alternative using Serialize if struct UserCoordOutput is defined and derived:
            // wtr.serialize(UserCoordOutput { username: &username, coord_count: count })?;
        }

        // Flush the writer to ensure all data is written to the file
        wtr.flush()?;
    } // File and writer are dropped and closed here

    println!("Successfully wrote user counts to {}.", OUTPUT_FILENAME);

    // Indicate successful completion
    Ok(())
}
