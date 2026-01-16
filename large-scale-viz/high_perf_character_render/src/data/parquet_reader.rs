use crate::data::sprite_data::{SpriteFrame, SpriteSequence};
use anyhow::{Context, Result};
use arrow::array::{
    Array, ArrayRef, DictionaryArray, Float64Array, Int64Array, Int8Array, ListArray,
    StringArray, TimestampNanosecondArray,
};
use arrow::datatypes::Int8Type;
use chrono::{DateTime, TimeZone, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct ParquetFilter {
    pub user_regex: Option<Regex>,
    pub timestamp_start: Option<DateTime<Utc>>,
    pub timestamp_end: Option<DateTime<Utc>>,
}

impl Default for ParquetFilter {
    fn default() -> Self {
        Self {
            user_regex: None,
            timestamp_start: None,
            timestamp_end: None,
        }
    }
}

pub struct ParquetReader {
    filter: ParquetFilter,
}

impl ParquetReader {
    pub fn new(filter: ParquetFilter) -> Self {
        Self { filter }
    }

    /// Read sprite frames from a single parquet file
    pub fn read_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<SpriteFrame>> {
        let file = File::open(path.as_ref())
            .context(format!("Failed to open parquet file: {:?}", path.as_ref()))?;

        let builder = ParquetRecordBatchReaderBuilder::try_new(file)
            .context("Failed to create parquet reader")?;

        let reader = builder.build()?;

        let mut frames = Vec::new();

        for batch_result in reader {
            let batch = batch_result?;

            // Extract columns
            let timestamp_col = batch
                .column_by_name("timestamp")
                .context("Missing timestamp column")?
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .context("Invalid timestamp column type")?;

            let user_col = batch
                .column_by_name("user")
                .context("Missing user column")?;
            let user_dict = user_col
                .as_any()
                .downcast_ref::<DictionaryArray<Int8Type>>()
                .context("Invalid user column type")?;
            let user_values = user_dict
                .values()
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid user values type")?;

            let env_id_col = batch
                .column_by_name("env_id")
                .context("Missing env_id column")?;
            let env_id_dict = env_id_col
                .as_any()
                .downcast_ref::<DictionaryArray<arrow::datatypes::Int16Type>>()
                .context("Invalid env_id column type")?;
            let env_id_values = env_id_dict
                .values()
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid env_id values type")?;

            let sprite_id_col = batch
                .column_by_name("sprite_id")
                .context("Missing sprite_id column")?;

            // sprite_id can be either Float64Array or Dictionary<Int8, Float64>
            let sprite_id_dict = sprite_id_col
                .as_any()
                .downcast_ref::<DictionaryArray<Int8Type>>()
                .context("Invalid sprite_id column type")?;
            let sprite_id_values = sprite_id_dict
                .values()
                .as_any()
                .downcast_ref::<Float64Array>()
                .context("Invalid sprite_id values type")?;

            let color_col = batch
                .column_by_name("color")
                .context("Missing color column")?;
            let color_dict = color_col
                .as_any()
                .downcast_ref::<DictionaryArray<Int8Type>>()
                .context("Invalid color column type")?;
            let color_values = color_dict
                .values()
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid color values type")?;

            let extra_col = batch
                .column_by_name("extra")
                .context("Missing extra column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid extra column type")?;

            let coords_col = batch
                .column_by_name("coords")
                .context("Missing coords column")?
                .as_any()
                .downcast_ref::<ListArray>()
                .context("Invalid coords column type")?;

            // Process each row
            for i in 0..batch.num_rows() {
                // Extract timestamp
                if timestamp_col.is_null(i) {
                    continue;
                }
                let timestamp_nanos = timestamp_col.value(i);
                let timestamp = Utc.timestamp_nanos(timestamp_nanos);

                // Apply timestamp filter
                if let Some(start) = self.filter.timestamp_start {
                    if timestamp < start {
                        continue;
                    }
                }
                if let Some(end) = self.filter.timestamp_end {
                    if timestamp > end {
                        continue;
                    }
                }

                // Extract user - skip row if null
                if user_dict.is_null(i) {
                    continue;
                }
                let user_key = user_dict.key(i).context("Invalid user key")?;
                let user = user_values.value(user_key as usize).to_string();

                // Apply user filter
                if let Some(regex) = &self.filter.user_regex {
                    if !regex.is_match(&user) {
                        continue;
                    }
                }

                // Extract env_id - skip row if null
                if env_id_dict.is_null(i) {
                    continue;
                }
                let env_id_key = env_id_dict.key(i).context("Invalid env_id key")?;
                let env_id = env_id_values.value(env_id_key as usize).to_string();

                // Extract sprite_id - match JS logic exactly:
                // Default to 0, and only use value if > 0 and < 50
                let sprite_id = if sprite_id_dict.is_null(i) {
                    0
                } else {
                    let key = sprite_id_dict.key(i).context("Invalid sprite_id key")?;
                    let sprite_id_raw = sprite_id_values.value(key as usize) as i32;
                    if sprite_id_raw > 0 && sprite_id_raw < 50 {
                        sprite_id_raw as u8
                    } else {
                        0
                    }
                };

                // Extract color (default to black if null)
                let color = if color_dict.is_null(i) {
                    "#000000".to_string()
                } else {
                    let color_key = color_dict.key(i).context("Invalid color key")?;
                    color_values.value(color_key as usize).to_string()
                };

                // Extract extra
                let extra = if extra_col.is_null(i) {
                    String::new()
                } else {
                    extra_col.value(i).to_string()
                };

                // Extract coords - nested list structure
                // Each row has a LIST of coordinates (a path)
                if coords_col.is_null(i) {
                    continue;
                }

                let coords_list = coords_col.value(i);
                let inner_list = coords_list
                    .as_any()
                    .downcast_ref::<ListArray>()
                    .context("Invalid inner coords list")?;

                if inner_list.len() == 0 {
                    continue;
                }

                // Iterate through ALL coordinates in the path
                for coord_idx in 0..inner_list.len() {
                    let coord_list = inner_list.value(coord_idx);
                    let coord_values = coord_list
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .context("Invalid coord values")?;

                    if coord_values.len() < 3 {
                        continue;
                    }

                    let coords = [
                        coord_values.value(0),
                        coord_values.value(1),
                        coord_values.value(2),
                    ];

                    // Each coordinate in the path gets the same timestamp/user/env_id
                    // path_index preserves the order within this path
                    frames.push(SpriteFrame {
                        timestamp,
                        user: user.clone(),
                        env_id: env_id.clone(),
                        sprite_id,
                        color: color.clone(),
                        extra: extra.clone(),
                        coords,
                        path_index: coord_idx,
                    });
                }
            }
        }

        log::info!("Read {} frames from {:?}", frames.len(), path.as_ref());
        Ok(frames)
    }

    /// Read multiple parquet files from a directory
    pub fn read_files<P: AsRef<Path>>(&self, files: &[P]) -> Result<Vec<SpriteFrame>> {
        let mut all_frames = Vec::new();

        for file_path in files {
            let frames = self.read_file(file_path)?;
            all_frames.extend(frames);
        }

        log::info!("Total frames read: {}", all_frames.len());
        Ok(all_frames)
    }

    /// Group frames into sequences by (user, env_id)
    pub fn group_into_sequences(frames: Vec<SpriteFrame>) -> Vec<SpriteSequence> {
        let mut sequences: HashMap<String, SpriteSequence> = HashMap::new();

        for frame in frames {
            let key = format!("{}-{}", frame.user, frame.env_id);

            sequences
                .entry(key.clone())
                .or_insert_with(|| SpriteSequence {
                    user: frame.user.clone(),
                    env_id: frame.env_id.clone(),
                    sprite_id: frame.sprite_id,
                    color: frame.color.clone(),
                    frames: Vec::new(),
                })
                .frames
                .push(frame);
        }

        // Sort frames within each sequence by timestamp, then path_index
        let mut result: Vec<SpriteSequence> = sequences.into_values().collect();
        for seq in &mut result {
            seq.frames.sort_by_key(|f| (f.timestamp, f.path_index));
        }

        log::info!("Grouped into {} sprite sequences", result.len());
        result
    }
}
