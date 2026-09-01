//! Build one full-history 768x768 OpenEXR coordinate heatmap per canonical user.
//!
//! The input is read directly from parquet in bounded record batches.  Only the
//! `user` and `coords` columns are decoded.  Every retained canonical user owns
//! one dense u64 count map; files are processed in parallel, while updates to a
//! particular user's map are grouped and locked once per record batch.

use anyhow::{bail, Context, Result};
use arrow::array::{Array, DictionaryArray, Int64Array, ListArray, StringArray};
use arrow::datatypes::{Int16Type, Int32Type, Int8Type};
use clap::{Parser, ValueEnum};
use image::{ImageBuffer, ImageFormat, Rgb};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const DIM: usize = 768;
const PIXELS: usize = DIM * DIM;
const FULL_MAP_MAX_COUNT: f64 = (1_u64 << 27) as f64;
// The temporary filename is ".{stem}.exr.tmp" (nine extra bytes).  Keeping
// the stem at 246 bytes or less respects the common 255-byte component limit.
const MAX_OUTPUT_STEM_BYTES: usize = 246;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FilenamePolicy {
    /// Reject a canonical username that is not safe as one literal filename.
    Strict,
    /// Encode NTFS-forbidden names and shorten oversized names deterministically.
    Percent,
}

#[derive(Debug, Parser)]
#[command(about = "Render one aggregate coordinate heatmap per canonical user")]
struct Args {
    /// Directory containing part_*.parquet input files.
    #[arg(
        long,
        default_value = "/home/pwhiddy/messages_1_15_2026_parquet_zstd_v2_env_id/data1"
    )]
    input: PathBuf,

    /// Destination directory for {canonical_username}.exr and manifest.json.
    #[arg(long, default_value = "images_users")]
    output: PathBuf,

    /// Map-region offsets used by the existing full-map renderer.
    #[arg(long, default_value = "../assets/map_data.json")]
    map_data: PathBuf,

    /// Notebook containing consolidate_user_data() and its manual_map.
    #[arg(long, default_value = "explore_parquet_data.ipynb")]
    notebook: PathBuf,

    /// Required because at least one known retained username contains '/'.
    #[arg(long, value_enum)]
    filename_policy: FilenamePolicy,

    /// Number of parquet files decoded concurrently.
    #[arg(long, default_value_t = 4)]
    threads: usize,

    /// Maximum rows decoded in one Arrow record batch, per worker.
    #[arg(long, default_value_t = 2048)]
    batch_size: usize,

    /// Process only the first N parquet files (for validation/benchmarking).
    #[arg(long)]
    max_files: Option<usize>,

    /// Process only the first N record batches in each selected file.
    #[arg(long)]
    max_batches_per_file: Option<usize>,

    /// Replace existing EXR/manifest files in the output directory.
    #[arg(long)]
    overwrite: bool,
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

#[derive(Debug, Deserialize)]
struct Notebook {
    cells: Vec<NotebookCell>,
}

#[derive(Debug, Deserialize)]
struct NotebookCell {
    source: Vec<String>,
}

#[derive(Debug)]
struct Canonicalizer {
    manual_map: HashMap<String, String>,
}

impl Canonicalizer {
    fn from_notebook(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| {
            format!(
                "failed to open canonicalization notebook {}",
                path.display()
            )
        })?;
        let notebook: Notebook = serde_json::from_reader(file)
            .with_context(|| format!("failed to parse notebook JSON {}", path.display()))?;

        let mapping_line = Regex::new(r"^\s*'((?:\\.|[^'])*)'\s*:\s*'((?:\\.|[^'])*)'\s*,?")?;
        let mut manual_map = HashMap::new();
        let mut found_block = false;

        for cell in notebook.cells {
            let mut in_manual_map = false;
            for line in cell.source {
                if line.contains("manual_map = {") {
                    in_manual_map = true;
                    found_block = true;
                    continue;
                }
                if !in_manual_map {
                    continue;
                }
                if line.trim() == "}" {
                    in_manual_map = false;
                    continue;
                }
                if let Some(captures) = mapping_line.captures(&line) {
                    let original = decode_python_single_quoted(&captures[1])?;
                    let canonical = decode_python_single_quoted(&captures[2])?;
                    if let Some(previous) = manual_map.insert(original.clone(), canonical.clone()) {
                        bail!(
                            "duplicate manual_map key {original:?}: both {previous:?} and {canonical:?}"
                        );
                    }
                }
            }
        }

        if !found_block || manual_map.is_empty() {
            bail!(
                "could not find a populated manual_map in {}",
                path.display()
            );
        }

        Ok(Self { manual_map })
    }

    /// Match the notebook exactly: drop nulls before this call, then exclude a
    /// username if the case-sensitive substring "tranny" occurs anywhere.
    fn canonical_name<'a>(&'a self, original: &'a str) -> Option<&'a str> {
        if original.contains("tranny") {
            return None;
        }
        Some(
            self.manual_map
                .get(original)
                .map(String::as_str)
                .unwrap_or(original),
        )
    }
}

fn decode_python_single_quoted(value: &str) -> Result<String> {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars
            .next()
            .context("unterminated escape in notebook manual_map string")?;
        match escaped {
            '\\' => output.push('\\'),
            '\'' => output.push('\''),
            '"' => output.push('"'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            other => bail!("unsupported Python string escape \\{other} in manual_map"),
        }
    }
    Ok(output)
}

#[derive(Debug)]
struct UserCounts {
    id: usize,
    canonical_name: String,
    pixels: Mutex<Box<[u64]>>,
    total_valid_coords: AtomicU64,
    variation_totals: Mutex<HashMap<String, u64>>,
}

impl UserCounts {
    fn new(id: usize, canonical_name: String) -> Self {
        Self {
            id,
            canonical_name,
            pixels: Mutex::new(vec![0_u64; PIXELS].into_boxed_slice()),
            total_valid_coords: AtomicU64::new(0),
            variation_totals: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedUser {
    original_name: Arc<str>,
    counts: Arc<UserCounts>,
}

#[derive(Debug)]
struct UserRegistry {
    canonicalizer: Canonicalizer,
    filename_policy: FilenamePolicy,
    users: Mutex<HashMap<String, Arc<UserCounts>>>,
}

impl UserRegistry {
    fn new(canonicalizer: Canonicalizer, filename_policy: FilenamePolicy) -> Self {
        Self {
            canonicalizer,
            filename_policy,
            users: Mutex::new(HashMap::new()),
        }
    }

    fn resolve(&self, original_name: &str) -> Result<Option<ResolvedUser>> {
        let Some(canonical_name) = self.canonicalizer.canonical_name(original_name) else {
            return Ok(None);
        };
        // Fail when a bad canonical filename is first observed, rather than
        // after an expensive full-dataset pass.
        output_stem(canonical_name, self.filename_policy)?;

        let mut users = self.users.lock().expect("user registry mutex poisoned");
        let next_id = users.len();
        let counts = users
            .entry(canonical_name.to_owned())
            .or_insert_with(|| Arc::new(UserCounts::new(next_id, canonical_name.to_owned())))
            .clone();

        Ok(Some(ResolvedUser {
            original_name: Arc::from(original_name),
            counts,
        }))
    }

    fn snapshot(&self) -> Vec<Arc<UserCounts>> {
        let users = self.users.lock().expect("user registry mutex poisoned");
        users.values().cloned().collect()
    }
}

#[derive(Debug, Default)]
struct Stats {
    files: AtomicU64,
    rows: AtomicU64,
    retained_rows: AtomicU64,
    excluded_rows: AtomicU64,
    valid_coords: AtomicU64,
    invalid_coords: AtomicU64,
}

#[derive(Clone, Copy, Debug)]
struct RegionOffset {
    x: i64,
    y: i64,
}

#[derive(Debug)]
struct RegionOffsets {
    min_id: i64,
    values: Vec<Option<RegionOffset>>,
}

impl RegionOffsets {
    fn from_json(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("failed to open map data {}", path.display()))?;
        let map_data: MapData = serde_json::from_reader(file)
            .with_context(|| format!("failed to parse map data {}", path.display()))?;
        if map_data.regions.is_empty() {
            bail!("map data contains no regions");
        }

        let parsed: Vec<(i64, RegionOffset)> = map_data
            .regions
            .into_iter()
            .map(|region| {
                let id = region
                    .id
                    .parse::<i64>()
                    .with_context(|| format!("invalid map region id {:?}", region.id))?;
                Ok((
                    id,
                    RegionOffset {
                        x: region.coordinates[0],
                        y: region.coordinates[1],
                    },
                ))
            })
            .collect::<Result<_>>()?;

        let min_id = parsed.iter().map(|(id, _)| *id).min().unwrap();
        let max_id = parsed.iter().map(|(id, _)| *id).max().unwrap();
        let mut values = vec![None; (max_id - min_id + 1) as usize];
        for (id, offset) in parsed {
            values[(id - min_id) as usize] = Some(offset);
        }
        Ok(Self { min_id, values })
    }

    #[inline(always)]
    fn get(&self, id: i64) -> Option<RegionOffset> {
        let index = id.checked_sub(self.min_id)? as usize;
        self.values.get(index).copied().flatten()
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
struct GroupKey {
    canonical_id: usize,
    original_name: Arc<str>,
}

struct RowGroup {
    user: ResolvedUser,
    rows: Vec<usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.threads == 0 {
        bail!("--threads must be at least 1");
    }
    if args.batch_size == 0 {
        bail!("--batch-size must be at least 1");
    }

    prepare_output_directory(&args.output, args.overwrite)?;

    let canonicalizer = Canonicalizer::from_notebook(&args.notebook)?;
    eprintln!(
        "Loaded {} manually curated username mappings from {}",
        canonicalizer.manual_map.len(),
        args.notebook.display()
    );
    let registry = Arc::new(UserRegistry::new(canonicalizer, args.filename_policy));
    let region_offsets = Arc::new(RegionOffsets::from_json(&args.map_data)?);
    let stats = Arc::new(Stats::default());

    let mut parquet_files = parquet_files(&args.input)?;
    if let Some(max_files) = args.max_files {
        parquet_files.truncate(max_files);
    }
    if parquet_files.is_empty() {
        bail!("no parquet files selected from {}", args.input.display());
    }

    eprintln!(
        "Processing {} parquet files with {} workers and batch size {}",
        parquet_files.len(),
        args.threads,
        args.batch_size
    );
    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .thread_name(|index| format!("user-heatmap-{index}"))
        .build()?;

    pool.install(|| {
        parquet_files.par_iter().try_for_each(|path| {
            process_file(
                path,
                args.batch_size,
                args.max_batches_per_file,
                &registry,
                &region_offsets,
                &stats,
            )?;

            let completed = stats.files.fetch_add(1, Ordering::Relaxed) + 1;
            if completed == 1 || completed % 10 == 0 || completed as usize == parquet_files.len() {
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                eprintln!(
                    "files {completed}/{} | rows {} | valid coords {} | invalid {} | users {} | {:.2} files/s",
                    parquet_files.len(),
                    stats.rows.load(Ordering::Relaxed),
                    stats.valid_coords.load(Ordering::Relaxed),
                    stats.invalid_coords.load(Ordering::Relaxed),
                    registry.snapshot().len(),
                    completed as f64 / elapsed,
                );
            }
            Ok::<(), anyhow::Error>(())
        })
    })?;

    let mut users = registry.snapshot();
    users.retain(|user| user.total_valid_coords.load(Ordering::Relaxed) != 0);
    users.sort_by(|a, b| a.canonical_name.cmp(&b.canonical_name));
    let user_total: u64 = users
        .iter()
        .map(|user| user.total_valid_coords.load(Ordering::Relaxed))
        .sum();
    let stats_total = stats.valid_coords.load(Ordering::Relaxed);
    if user_total != stats_total {
        bail!(
            "internal count mismatch: user maps contain {user_total} valid coordinates but global stats contain {stats_total}"
        );
    }
    validate_output_names(&users, args.filename_policy)?;

    eprintln!(
        "Rendering {} canonical users to {}",
        users.len(),
        args.output.display()
    );
    let manifest_entries: Vec<ManifestUser> = pool.install(|| {
        users
            .par_iter()
            .map(|user| render_user(user, &args.output, args.filename_policy, args.overwrite))
            .collect::<Result<Vec<_>>>()
    })?;

    let manifest = Manifest {
        input: args.input.display().to_string(),
        map_data: args.map_data.display().to_string(),
        canonicalization_notebook: args.notebook.display().to_string(),
        width: DIM,
        height: DIM,
        normalization_max_count: FULL_MAP_MAX_COUNT as u64,
        files_processed: stats.files.load(Ordering::Relaxed),
        rows_processed: stats.rows.load(Ordering::Relaxed),
        retained_rows: stats.retained_rows.load(Ordering::Relaxed),
        excluded_rows: stats.excluded_rows.load(Ordering::Relaxed),
        valid_coords: stats.valid_coords.load(Ordering::Relaxed),
        invalid_coords: stats.invalid_coords.load(Ordering::Relaxed),
        users: manifest_entries,
    };
    write_manifest(&args.output, &manifest, args.overwrite)?;

    eprintln!(
        "Done in {:.1}s: {} EXRs, {} valid coordinates, peak count maps approximately {:.2} GiB",
        started.elapsed().as_secs_f64(),
        manifest.users.len(),
        manifest.valid_coords,
        manifest.users.len() as f64 * PIXELS as f64 * 8.0 / (1024.0_f64.powi(3)),
    );
    Ok(())
}

fn prepare_output_directory(output: &Path, overwrite: bool) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("failed to create output directory {}", output.display()))?;
    if !overwrite {
        let mut conflicts = Vec::new();
        for entry in fs::read_dir(output)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("exr")
                || path.file_name().and_then(|value| value.to_str()) == Some("manifest.json")
            {
                conflicts.push(path);
                if conflicts.len() == 3 {
                    break;
                }
            }
        }
        if !conflicts.is_empty() {
            bail!(
                "output directory {} already contains generated files (for example {}); pass --overwrite to replace them",
                output.display(),
                conflicts[0].display()
            );
        }
    }
    Ok(())
}

fn parquet_files(input: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(input)
        .with_context(|| format!("failed to read input directory {}", input.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("parquet"))
        .collect();
    files.sort();
    Ok(files)
}

fn process_file(
    path: &Path,
    batch_size: usize,
    max_batches: Option<usize>,
    registry: &UserRegistry,
    region_offsets: &RegionOffsets,
    stats: &Stats,
) -> Result<()> {
    let file = File::open(path)
        .with_context(|| format!("failed to open parquet file {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .with_context(|| format!("failed to inspect parquet file {}", path.display()))?;
    let user_index = builder.schema().index_of("user")?;
    let coords_index = builder.schema().index_of("coords")?;
    let projection = ProjectionMask::roots(builder.parquet_schema(), [user_index, coords_index]);
    let reader = builder
        .with_projection(projection)
        .with_batch_size(batch_size)
        .build()
        .with_context(|| format!("failed to build parquet reader for {}", path.display()))?;

    for (batch_index, batch_result) in reader.enumerate() {
        if max_batches.is_some_and(|limit| batch_index >= limit) {
            break;
        }
        let batch = batch_result.with_context(|| {
            format!(
                "failed to decode batch {batch_index} from {}",
                path.display()
            )
        })?;
        process_batch(&batch, registry, region_offsets, stats).with_context(|| {
            format!(
                "failed to process batch {batch_index} from {}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn process_batch(
    batch: &arrow::record_batch::RecordBatch,
    registry: &UserRegistry,
    region_offsets: &RegionOffsets,
    stats: &Stats,
) -> Result<()> {
    let user_column = batch
        .column_by_name("user")
        .context("missing projected user column")?;
    let coords = batch
        .column_by_name("coords")
        .context("missing projected coords column")?
        .as_any()
        .downcast_ref::<ListArray>()
        .context("coords is not List<List<Int64>>")?;
    let inner_coords = coords
        .values()
        .as_any()
        .downcast_ref::<ListArray>()
        .context("coords inner values are not List<Int64>")?;
    let coord_values = inner_coords
        .values()
        .as_any()
        .downcast_ref::<Int64Array>()
        .context("coordinate values are not Int64")?;

    let mut groups: HashMap<GroupKey, RowGroup> = HashMap::new();
    let mut excluded_rows = 0_u64;

    macro_rules! group_dictionary_rows {
        ($dict:expr) => {{
            let dict = $dict;
            let values = dict
                .values()
                .as_any()
                .downcast_ref::<StringArray>()
                .context("user dictionary values are not strings")?;
            let resolved: Vec<Option<ResolvedUser>> = (0..values.len())
                .map(|index| registry.resolve(values.value(index)))
                .collect::<Result<_>>()?;
            let keys = dict.keys();
            for row in 0..batch.num_rows() {
                if dict.is_null(row) {
                    excluded_rows += 1;
                    continue;
                }
                let key = keys.value(row) as usize;
                if let Some(user) = resolved.get(key).and_then(Option::as_ref) {
                    add_group_row(&mut groups, user, row);
                } else {
                    excluded_rows += 1;
                }
            }
        }};
    }

    if let Some(dict) = user_column
        .as_any()
        .downcast_ref::<DictionaryArray<Int8Type>>()
    {
        group_dictionary_rows!(dict);
    } else if let Some(dict) = user_column
        .as_any()
        .downcast_ref::<DictionaryArray<Int16Type>>()
    {
        group_dictionary_rows!(dict);
    } else if let Some(dict) = user_column
        .as_any()
        .downcast_ref::<DictionaryArray<Int32Type>>()
    {
        group_dictionary_rows!(dict);
    } else if let Some(strings) = user_column.as_any().downcast_ref::<StringArray>() {
        let mut resolved: HashMap<String, Option<ResolvedUser>> = HashMap::new();
        for row in 0..batch.num_rows() {
            if strings.is_null(row) {
                excluded_rows += 1;
                continue;
            }
            let original = strings.value(row);
            if !resolved.contains_key(original) {
                resolved.insert(original.to_owned(), registry.resolve(original)?);
            }
            if let Some(user) = resolved.get(original).and_then(Option::as_ref) {
                add_group_row(&mut groups, user, row);
            } else {
                excluded_rows += 1;
            }
        }
    } else {
        bail!("unsupported user column type {:?}", user_column.data_type());
    }

    let retained_rows = batch.num_rows() as u64 - excluded_rows;
    let outer_offsets = coords.value_offsets();
    let inner_offsets = inner_coords.value_offsets();
    let values = coord_values.values();
    let values_have_nulls = coord_values.null_count() != 0;
    let mut batch_valid = 0_u64;
    let mut batch_invalid = 0_u64;

    for (_, group) in groups {
        let mut pixels = group
            .user
            .counts
            .pixels
            .lock()
            .expect("user pixel mutex poisoned");
        let mut variation_valid = 0_u64;

        for row in group.rows {
            if coords.is_null(row) {
                continue;
            }
            let first_coord = outer_offsets[row] as usize;
            let end_coord = outer_offsets[row + 1] as usize;
            for coord_index in first_coord..end_coord {
                if inner_coords.is_null(coord_index) {
                    batch_invalid += 1;
                    continue;
                }
                let value_start = inner_offsets[coord_index] as usize;
                let value_end = inner_offsets[coord_index + 1] as usize;
                if value_end - value_start != 3 {
                    batch_invalid += 1;
                    continue;
                }
                if values_have_nulls
                    && (!coord_values.is_valid(value_start)
                        || !coord_values.is_valid(value_start + 1)
                        || !coord_values.is_valid(value_start + 2))
                {
                    batch_invalid += 1;
                    continue;
                }

                let local_x = values[value_start];
                let local_y = values[value_start + 1];
                let map_id = values[value_start + 2];
                let Some(offset) = region_offsets.get(map_id) else {
                    batch_invalid += 1;
                    continue;
                };
                let global_x = local_x + offset.x;
                let global_y = local_y + offset.y;
                if !(0..DIM as i64).contains(&global_x) || !(0..DIM as i64).contains(&global_y) {
                    batch_invalid += 1;
                    continue;
                }

                // Match main_coords_based.rs: the first dimension is x and
                // the second is y, then put_pixel(x, y).
                pixels[global_x as usize * DIM + global_y as usize] += 1;
                variation_valid += 1;
            }
        }

        drop(pixels);
        batch_valid += variation_valid;
        group
            .user
            .counts
            .total_valid_coords
            .fetch_add(variation_valid, Ordering::Relaxed);
        let mut variations = group
            .user
            .counts
            .variation_totals
            .lock()
            .expect("variation totals mutex poisoned");
        *variations
            .entry(group.user.original_name.to_string())
            .or_default() += variation_valid;
    }

    stats
        .rows
        .fetch_add(batch.num_rows() as u64, Ordering::Relaxed);
    stats
        .retained_rows
        .fetch_add(retained_rows, Ordering::Relaxed);
    stats
        .excluded_rows
        .fetch_add(excluded_rows, Ordering::Relaxed);
    stats.valid_coords.fetch_add(batch_valid, Ordering::Relaxed);
    stats
        .invalid_coords
        .fetch_add(batch_invalid, Ordering::Relaxed);
    Ok(())
}

fn add_group_row(groups: &mut HashMap<GroupKey, RowGroup>, user: &ResolvedUser, row: usize) {
    let key = GroupKey {
        canonical_id: user.counts.id,
        original_name: Arc::clone(&user.original_name),
    };
    groups
        .entry(key)
        .or_insert_with(|| RowGroup {
            user: user.clone(),
            rows: Vec::new(),
        })
        .rows
        .push(row);
}

fn validate_output_names(users: &[Arc<UserCounts>], policy: FilenamePolicy) -> Result<()> {
    let mut used: HashMap<String, String> = HashMap::new();
    for user in users {
        let encoded = output_stem(&user.canonical_name, policy)?;
        if let Some(previous) = used.insert(encoded.clone(), user.canonical_name.clone()) {
            bail!(
                "canonical usernames {previous:?} and {:?} collide at output filename {encoded:?}",
                user.canonical_name
            );
        }
    }
    Ok(())
}

fn output_stem(canonical_name: &str, policy: FilenamePolicy) -> Result<String> {
    if canonical_name.is_empty() && matches!(policy, FilenamePolicy::Strict) {
        bail!("empty canonical username cannot be used as an output filename");
    }
    match policy {
        FilenamePolicy::Strict => {
            if canonical_name == "."
                || canonical_name == ".."
                || canonical_name.chars().any(|ch| {
                    matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                        || ch.is_control()
                })
                || canonical_name.ends_with(' ')
                || canonical_name.ends_with('.')
            {
                bail!(
                    "canonical username {canonical_name:?} is not one portable literal filename; rerun with --filename-policy percent or curate an additional notebook mapping"
                );
            }
            Ok(canonical_name.to_owned())
        }
        FilenamePolicy::Percent => {
            if canonical_name.is_empty() {
                return Ok("%EMPTY".to_owned());
            }
            let mut output = String::with_capacity(canonical_name.len());
            let last_index = canonical_name.chars().count() - 1;
            for (index, ch) in canonical_name.chars().enumerate() {
                if ch == '%'
                    || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                    || ch.is_control()
                    || (index == last_index && matches!(ch, ' ' | '.'))
                {
                    push_percent_encoded(&mut output, ch);
                } else {
                    output.push(ch);
                }
            }

            // Windows/NTFS reserves these device basenames even when an
            // extension (such as .exr) is present. Encode the first character
            // to make the filename unambiguously ordinary.
            if is_windows_reserved_name(canonical_name) {
                let first = output.chars().next().unwrap();
                let first_len = first.len_utf8();
                let mut encoded_first = String::new();
                push_percent_encoded(&mut encoded_first, first);
                output.replace_range(..first_len, &encoded_first);
            }

            // Extremely long names receive a deterministic hash suffix. The
            // manifest remains the source of truth for the full canonical
            // username, and collision validation still runs before export.
            if output.len() > MAX_OUTPUT_STEM_BYTES {
                let suffix = format!("%~{:016X}", fnv1a64(canonical_name.as_bytes()));
                let prefix_limit = MAX_OUTPUT_STEM_BYTES - suffix.len();
                while output.len() > prefix_limit {
                    output.pop();
                }
                output.push_str(&suffix);
            }
            Ok(output)
        }
    }
}

fn push_percent_encoded(output: &mut String, ch: char) {
    let mut encoded = [0_u8; 4];
    for &byte in ch.encode_utf8(&mut encoded).as_bytes() {
        output.push('%');
        output.push(
            char::from_digit((byte >> 4) as u32, 16)
                .unwrap()
                .to_ascii_uppercase(),
        );
        output.push(
            char::from_digit((byte & 0x0f) as u32, 16)
                .unwrap()
                .to_ascii_uppercase(),
        );
    }
}

fn is_windows_reserved_name(name: &str) -> bool {
    let basename = name
        .trim_end_matches(|ch| ch == ' ' || ch == '.')
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || basename
            .strip_prefix("COM")
            .or_else(|| basename.strip_prefix("LPT"))
            .is_some_and(|number| number.len() == 1 && matches!(number.as_bytes()[0], b'1'..=b'9'))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Serialize)]
struct Manifest {
    input: String,
    map_data: String,
    canonicalization_notebook: String,
    width: usize,
    height: usize,
    normalization_max_count: u64,
    files_processed: u64,
    rows_processed: u64,
    retained_rows: u64,
    excluded_rows: u64,
    valid_coords: u64,
    invalid_coords: u64,
    users: Vec<ManifestUser>,
}

#[derive(Debug, Serialize)]
struct ManifestUser {
    canonical_username: String,
    filename: String,
    total_valid_coords: u64,
    max_pixel_count: u64,
    variations: BTreeMap<String, u64>,
}

fn render_user(
    user: &UserCounts,
    output: &Path,
    filename_policy: FilenamePolicy,
    overwrite: bool,
) -> Result<ManifestUser> {
    let stem = output_stem(&user.canonical_name, filename_policy)?;
    let filename = format!("{stem}.exr");
    let destination = output.join(&filename);
    if destination.exists() && !overwrite {
        bail!("refusing to replace existing {}", destination.display());
    }
    let temporary = output.join(format!(".{stem}.exr.tmp"));

    let pixels = user.pixels.lock().expect("user pixel mutex poisoned");
    let max_pixel_count = pixels.iter().copied().max().unwrap_or(0);
    let mut image: ImageBuffer<Rgb<f32>, Vec<f32>> = ImageBuffer::new(DIM as u32, DIM as u32);
    for x in 0..DIM {
        for y in 0..DIM {
            let intensity = ((pixels[x * DIM + y] as f64 / FULL_MAP_MAX_COUNT).min(1.0)) as f32;
            image.put_pixel(x as u32, y as u32, Rgb([intensity, intensity, intensity]));
        }
    }
    drop(pixels);

    image
        .save_with_format(&temporary, ImageFormat::OpenExr)
        .with_context(|| format!("failed to write temporary EXR {}", temporary.display()))?;
    fs::rename(&temporary, &destination).with_context(|| {
        format!(
            "failed to move completed EXR {} to {}",
            temporary.display(),
            destination.display()
        )
    })?;

    let variations = user
        .variation_totals
        .lock()
        .expect("variation totals mutex poisoned")
        .iter()
        .map(|(name, count)| (name.clone(), *count))
        .collect();
    Ok(ManifestUser {
        canonical_username: user.canonical_name.clone(),
        filename,
        total_valid_coords: user.total_valid_coords.load(Ordering::Relaxed),
        max_pixel_count,
        variations,
    })
}

fn write_manifest(output: &Path, manifest: &Manifest, overwrite: bool) -> Result<()> {
    let destination = output.join("manifest.json");
    if destination.exists() && !overwrite {
        bail!("refusing to replace existing {}", destination.display());
    }
    let temporary = output.join(".manifest.json.tmp");
    let file = File::create(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    serde_json::to_writer_pretty(file, manifest)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, &destination).with_context(|| {
        format!(
            "failed to move completed manifest {} to {}",
            temporary.display(),
            destination.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_notebook_newline_escape() {
        assert_eq!(
            decode_python_single_quoted(r"leanke@pufferbox3\nboey_obs_wrapper").unwrap(),
            "leanke@pufferbox3\nboey_obs_wrapper"
        );
    }

    #[test]
    fn percent_filename_is_reversible_and_safe() {
        assert_eq!(
            output_stem("TREBOR_AGENTS/n", FilenamePolicy::Percent).unwrap(),
            "TREBOR_AGENTS%2Fn"
        );
        assert_eq!(
            output_stem("already%encoded", FilenamePolicy::Percent).unwrap(),
            "already%25encoded"
        );
        assert_eq!(output_stem("👀", FilenamePolicy::Percent).unwrap(), "👀");
        assert_eq!(
            output_stem("localtesty |BET|", FilenamePolicy::Percent).unwrap(),
            "localtesty %7CBET%7C"
        );
        assert_eq!(
            output_stem("trailing. ", FilenamePolicy::Percent).unwrap(),
            "trailing.%20"
        );
        assert_eq!(
            output_stem("CON", FilenamePolicy::Percent).unwrap(),
            "%43ON"
        );
        assert!(
            output_stem(&"a".repeat(400), FilenamePolicy::Percent)
                .unwrap()
                .len()
                <= MAX_OUTPUT_STEM_BYTES
        );
    }

    #[test]
    fn strict_filename_rejects_slash() {
        assert!(output_stem("TREBOR_AGENTS/n", FilenamePolicy::Strict).is_err());
    }
}
