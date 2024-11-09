use std::collections::HashSet;
use std::fs::File;
use std::io;

use csv::ReaderBuilder;
use indicatif::ProgressBar;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Record {
    timestamp: String,
    message: String,
}

const DISPLAY_INTERVAL: u64 = 20_000;

fn main() {
    // Create a buffer reader from the standard input
    let stdin = io::stdin();
    let reader = stdin.lock();

    // Initialize counters
    let mut row_count: u64 = 0;
    let mut failed_rows: u64 = 0;
    
    // Create a HashSet to store unique usernames
    let mut unique_usernames: HashSet<String> = HashSet::new();

    // Create a progress bar
    let progress_bar = ProgressBar::new_spinner();

    // Set initial state for progress bar
    progress_bar.set_message(format!(
        "rows: {} unique users: {} failed: {}", 
        row_count, 
        unique_usernames.len(), 
        failed_rows
    ));

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
                                unique_usernames.insert(username.to_string());
                            } else {
                                failed_rows += 1;
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
        let message = format!(
            "rows: {} unique users: {} failed: {}", 
            row_count, 
            unique_usernames.len(), 
            failed_rows
        );
        
        if row_count % 10_000_000 == 0 {
            println!("{}", message.clone());
        }
        progress_bar.set_message(message.clone());

        // Display current list of usernames periodically
        if row_count % DISPLAY_INTERVAL == 0 {
            println!("\nCurrent unique usernames ({}):", unique_usernames.len());
            let mut sorted_usernames: Vec<&String> = unique_usernames.iter().collect();
            sorted_usernames.sort();
            for username in sorted_usernames {
                println!("  - {}", username);
            }
            println!("\n{}", message);
        }
    }

    // Display final statistics and username list
    println!("\nFinal Statistics:");
    println!("Total rows processed: {}", row_count);
    println!("Total unique users: {}", unique_usernames.len());
    println!("Failed rows: {}", failed_rows);
    
    println!("\nFinal list of unique usernames:");
    let mut sorted_usernames: Vec<&String> = unique_usernames.iter().collect();
    sorted_usernames.sort();
    for username in sorted_usernames {
        println!("  - {}", username);
    }
}
