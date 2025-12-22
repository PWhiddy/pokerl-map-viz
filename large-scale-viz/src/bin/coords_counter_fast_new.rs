use std::io::{self, Read};
use rayon::prelude::*;
use serde::Deserialize;
use std::cell::RefCell;
use std::borrow::Cow;
use indicatif::{ProgressBar, ProgressStyle, HumanCount};

#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[derive(Deserialize, Debug)]
struct Row<'a> {
    #[serde(borrow)] 
    metadata: Metadata<'a>,
    coords: Vec<[i32; 3]>, 
}

#[derive(Deserialize, Debug)]
struct Metadata<'a> {
    #[serde(borrow)]
    user: Cow<'a, str>,
    #[serde(borrow)]
    color: Cow<'a, str>,
    #[serde(borrow)]
    extra: Cow<'a, str>,
}

fn main() -> io::Result<()> {
    // 1. Setup Progress Bar with a wide template
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner()
        // Template: [Time] Bytes (Speed) | Custom Message
        .template("{spinner:.green} [{elapsed_precise}] {bytes} ({bytes_per_sec}) | {msg}")
        .unwrap());
    
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    let cap = 1024 * 1024;
    let mut buf = vec![0u8; cap];
    let mut left_over = Vec::with_capacity(4096);
    
    let mut total_coords: u64 = 0;
    let mut magikarp_count: u64 = 0; 
    let mut loop_idx: u64 = 0;

    loop {
        let start_fill = left_over.len();
        if buf.len() < start_fill + cap {
            buf.resize(start_fill + cap, 0);
        }
        buf[0..start_fill].copy_from_slice(&left_over);

        let n = handle.read(&mut buf[start_fill..])?;
        if n == 0 {
            if !left_over.is_empty() {
                let (c, m) = process_chunk(&left_over);
                total_coords += c;
                magikarp_count += m;
            }
            break;
        }

        // --- UPDATE PROGRESS BAR ---
        pb.inc(n as u64);
        
        // Update the text message every 128 chunks (approx 30 times/sec at 3.5GB/s)
        // This prevents terminal flickering and locking overhead.
        if loop_idx % 512 == 0 {
            pb.set_message(format!(
                "Coords: {} | Magikarps: {}", 
                HumanCount(total_coords), 
                HumanCount(magikarp_count)
            ));
        }
        loop_idx += 1;

        let valid_data = &buf[0..start_fill + n];
        
        let split_idx = match memchr::memrchr(b'\n', valid_data) {
            Some(idx) => idx + 1,
            None => {
                left_over.extend_from_slice(valid_data);
                continue;
            }
        };

        let (chunk, rest) = valid_data.split_at(split_idx);
        left_over.clear();
        left_over.extend_from_slice(rest);

        let (batch_coords, batch_magikarp) = process_chunk(chunk);
        
        total_coords += batch_coords;
        magikarp_count += batch_magikarp;
    }

    pb.finish_with_message(format!(
        "Done! Total Coords: {} | Total Magikarps: {}", 
        HumanCount(total_coords), 
        HumanCount(magikarp_count)
    ));

    Ok(())
}

fn process_chunk(chunk: &[u8]) -> (u64, u64) {
    chunk
        .par_split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| parse_row(line))
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

thread_local! {
    static SCRATCH: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(8192));
}

fn parse_row(line: &[u8]) -> (u64, u64) {
    let comma_pos = match memchr::memchr(b',', line) {
        Some(p) => p,
        None => return (0, 0),
    };

    if line.len() < comma_pos + 3 { return (0, 0); }
    
    let raw_json_slice = &line[comma_pos + 2 .. line.len() - 1];

    SCRATCH.with(|cell| {
        let mut scratch = cell.borrow_mut();
        scratch.clear();

        let mut i = 0;
        let len = raw_json_slice.len();
        while i < len {
            let b = raw_json_slice[i];
            scratch.push(b);
            if b == b'"' { i += 1; }
            i += 1;
        }

        match serde_json::from_slice::<Row>(&scratch) {
            Ok(row) => {
                let is_magikarp = if row.metadata.extra.contains("Magikarp") { 1 } else { 0 };
                (row.coords.len() as u64, is_magikarp)
            },
            Err(_) => (0, 0),
        }
    })
}
