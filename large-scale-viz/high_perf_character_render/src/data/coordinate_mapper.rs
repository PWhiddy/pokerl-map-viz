use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn deserialize_id<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<i64>().map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRegion {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: i64,
    pub coordinates: [f32; 2],
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MapData {
    regions: Vec<MapRegion>,
}

pub struct CoordinateMapper {
    regions: HashMap<i64, MapRegion>,
}

pub const INVALID_MAP_ID_FLAG: [f32; 2] = [117117.0, 117117.0];

impl CoordinateMapper {
    /// Load map data from JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .context("Failed to read map_data.json")?;

        let map_data: MapData = serde_json::from_str(&content)
            .context("Failed to parse map_data.json")?;

        let regions: HashMap<i64, MapRegion> = map_data
            .regions
            .into_iter()
            .map(|r| (r.id, r))
            .collect();

        log::info!("Loaded {} map regions", regions.len());

        Ok(Self { regions })
    }

    /// Convert game coordinates to pixel position
    /// Formula from JS: [coords[0] + mapX - 217.5, coords[1] + mapY - 221.5] * 16
    /// The JS uses a centered coordinate system (0,0 at center of 6976x7104 map)
    /// We need to offset to top-left coordinate system for rendering to 8192x8192 canvas
    pub fn convert_coords(&self, coords: &[i64; 3]) -> [f32; 2] {
        let map_region_id = coords[2];

        if let Some(region) = self.regions.get(&map_region_id) {
            // Get position in JS coordinate system (centered)
            let x_centered = (coords[0] as f32 + region.coordinates[0] - 217.5) * 16.0;
            let y_centered = (coords[1] as f32 + region.coordinates[1] - 221.5) * 16.0;

            // Offset to top-left coordinate system
            // Map is 6976x7104, canvas is 8192x8192
            // Add half map size + centering offset = 6976/2 + (8192-6976)/2 = 3488 + 608 = 4096
            let x = x_centered + 4096.0;
            let y = y_centered + 4096.0;

            [x, y]
        } else {
            log::warn!("No map coordinate location for id: {}", map_region_id);
            INVALID_MAP_ID_FLAG // invalid for example 255 is seen sometimes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_conversion() {
        let mut regions = HashMap::new();
        regions.insert(1, MapRegion {
            id: 1,
            coordinates: [500.0, 600.0],
            name: Some("Test Region".to_string()),
        });

        let mapper = CoordinateMapper { regions };

        // Test coordinate: [10, 20, 1]
        // Expected: (10 + 500 - 217.5) * 16, (20 + 600 - 221.5) * 16
        // = (292.5 * 16, 398.5 * 16) = (4680.0, 6376.0)
        let result = mapper.convert_coords(&[10, 20, 1]);
        assert!((result[0] - 4680.0).abs() < 0.01);
        assert!((result[1] - 6376.0).abs() < 0.01);
    }
}
