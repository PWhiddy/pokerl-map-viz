use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

/// Represents a position in the game world as (x, y, map_id)
pub type Position = (u8, u8, u8);

/// HashMap for O(1) warp lookup: (map_id, x, y) -> (new_map_id, new_x, new_y)
static WARP_MAP: Lazy<HashMap<Position, Position>> = Lazy::new(|| {
    load_warps().expect("Failed to load warp data")
});

static WARP_V2: Lazy<HashMap<String, String>> = Lazy::new(|| {
    load_warps_v2().expect("failed to load new v2 warp data")
});

fn load_warps_v2() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let json_data = include_str!("../../extract_warps/transitions_weak.json");
    let value: Value = serde_json::from_str(json_data)?;

    let obj = value
        .as_object()
        .ok_or("expected top-level JSON object for transitions")?;

    let mut map = HashMap::with_capacity(obj.len());

    for (key, val) in obj {
        let target = val
            .as_str()
            .ok_or("transition value must be a string")?;

        map.insert(key.clone(), target.to_string());
    }

    Ok(map)
}

pub fn valid_coordinate_pair_v2(transition: String) -> bool {
    if let Some(_type) = WARP_V2.get(&transition) {
        return true;
    } else {
        return false;
    }
}

/// Loads warp data from the JSON file and builds the lookup HashMap
fn load_warps() -> Result<HashMap<Position, Position>, Box<dyn std::error::Error>> {
    let json_data = include_str!("../3d_red_warps.json");
    let warps: Value = serde_json::from_str(json_data)?;

    let mut warp_map = HashMap::new();

    // Parse the 3D array: [map_id][warp_num][warp_data]
    if let Some(maps) = warps.as_array() {
        for (map_id, map_warps) in maps.iter().enumerate() {
            if let Some(warp_list) = map_warps.as_array() {
                for warp_data in warp_list {
                    if let Some(data) = warp_data.as_array() {
                        // Parse [cur_y, cur_x, new_map_id, new_y, new_x]
                        if data.len() == 5 {
                            let cur_y = data[0].as_u64().unwrap_or(0) as u8;
                            let cur_x = data[1].as_u64().unwrap_or(0) as u8;
                            let new_map_id = data[2].as_u64().unwrap_or(0) as u8;
                            let new_y = data[3].as_u64().unwrap_or(0) as u8;
                            let new_x = data[4].as_u64().unwrap_or(0) as u8;

                            // Skip all-zero entries (invalid warps)
                            if cur_y == 0 && cur_x == 0 && new_map_id == 0 && new_y == 0 && new_x == 0 {
                                continue;
                            }

                            // Store as: (map_id, x, y) -> (new_map_id, new_x, new_y)
                            let from_pos = (cur_x, cur_y, map_id as u8);
                            let to_pos = (new_x, new_y, new_map_id);
                            warp_map.insert(from_pos, to_pos);
                            warp_map.insert(to_pos, from_pos);
                        }
                    }
                }
            }
        }
    }
    //println!("valid warp pairs: {:?}", warp_map);

    Ok(warp_map)
}

/// Validates if there is a warp transition from position `a` to position `b`
///
/// # Arguments
/// * `a` - Source position as [x, y, map_id]
/// * `b` - Destination position as [x, y, map_id]
///
/// # Returns
/// `true` if there is a valid warp from `a` to `b`, `false` otherwise
pub fn valid_coordinate_pair(a: [u8; 3], b: [u8; 3]) -> bool {
    let from_pos = (a[0], a[1], a[2]);

    if let Some(&to_pos) = WARP_MAP.get(&from_pos) {
        // Check if the warp destination matches position b
        to_pos.0 == b[0] && to_pos.1 == b[1] && to_pos.2 == b[2]
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warp_map_loaded() {
        // Ensure the warp map is loaded and contains data
        assert!(!WARP_MAP.is_empty(), "Warp map should not be empty");
        println!("Loaded {} warps", WARP_MAP.len());
    }

    #[test]
    fn test_valid_warp() {
        // Based on the JSON data: map 0, position (5, 5) -> map 37, position (2, 7)
        let from = [5, 5, 0]; // [x, y, map_id]
        let to = [2, 7, 37];  // [x, y, map_id]

        assert!(valid_coordinate_pair(from, to),
                "Should be a valid warp from {:?} to {:?}", from, to);
    }

    #[test]
    fn test_invalid_warp() {
        // Non-existent warp
        let from = [0, 0, 0];
        let to = [1, 1, 1];

        assert!(!valid_coordinate_pair(from, to),
                "Should not be a valid warp from {:?} to {:?}", from, to);
    }

    #[test]
    fn test_wrong_destination() {
        // Valid source but wrong destination
        let from = [5, 5, 0]; // [x, y, map_id]
        let to = [9, 9, 99];  // Wrong destination

        assert!(!valid_coordinate_pair(from, to),
                "Should not be valid with wrong destination");
    }
}
