use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down,
    Up,
    Left,
    Right,
}

impl Direction {
    /// Get the column index in the sprite sheet for this direction
    pub fn column_index(&self) -> usize {
        match self {
            Direction::Down => 1,
            Direction::Up => 4,
            Direction::Left => 6,
            Direction::Right => 8,
        }
    }

    pub fn column_index_short(&self) -> usize {
        match self {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::Left => 2,
            Direction::Right => 3,
        }
    }

    /// Determine direction based on movement delta
    pub fn from_movement(dx: f32, dy: f32) -> Self {
        if dx.abs() > dy.abs() {
            if dx > 0.0 {
                Direction::Right
            } else {
                Direction::Left
            }
        } else {
            if dy > 0.0 {
                Direction::Down
            } else {
                Direction::Up
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpriteFrame {
    pub timestamp: DateTime<Utc>,
    pub user: String,
    pub env_id: String,
    pub sprite_id: u8,
    pub color: String,
    pub extra: String,
    pub coords: [i64; 3], // [x, y, map_region_id]
    pub path_index: usize, // Order within the path for this timestamp
}

#[derive(Debug, Clone)]
pub struct SpriteSequence {
    pub user: String,
    pub env_id: String,
    pub sprite_id: u8,
    pub color: String,
    pub frames: Vec<SpriteFrame>,
}

impl SpriteSequence {
    pub fn cache_key(&self) -> String {
        format!("{}-{}", self.user, self.env_id)
    }
}

#[derive(Debug, Clone)]
pub struct SpriteInstance {
    pub position: [f32; 2],      // Screen position in pixels
    pub sprite_id: u8,            // Which character (0-49)
    pub direction: Direction,     // Which direction sprite to use
}

#[derive(Debug, Clone, Copy)]
pub struct AnimationState {
    pub current_frame_index: usize,
    pub next_frame_index: usize,
    pub interpolation_t: f32,      // 0.0 to 1.0
}
