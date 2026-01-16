use crate::data::{AnimationState, CoordinateMapper, Direction, SpriteSequence, SpriteInstance};

pub struct AnimationInterpolator {
    coordinate_mapper: CoordinateMapper,
    interval_ms: f32,
    fps: f32,
}

impl AnimationInterpolator {
    pub fn new(coordinate_mapper: CoordinateMapper, interval_ms: f32, fps: f32) -> Self {
        Self {
            coordinate_mapper,
            interval_ms,
            fps,
        }
    }

    /// Calculate animation state for a sprite sequence at a given time
    pub fn get_animation_state(&self, sequence: &SpriteSequence, time_ms: f32) -> Option<AnimationState> {
        if sequence.frames.is_empty() {
            return None;
        }

        // Calculate which point we're at based on fixed interval
        let point_index_f = time_ms / self.interval_ms;
        let current_frame_index = point_index_f.floor() as usize;

        if current_frame_index >= sequence.frames.len() {
            return None; // Animation finished
        }

        let next_frame_index = (current_frame_index + 1).min(sequence.frames.len() - 1);
        let interpolation_t = point_index_f.fract();

        Some(AnimationState {
            current_frame_index,
            next_frame_index,
            interpolation_t,
        })
    }

    /// Get interpolated position and direction for a sprite at a given animation state
    pub fn interpolate_sprite(
        &self,
        sequence: &SpriteSequence,
        state: &AnimationState,
    ) -> Option<SpriteInstance> {
        if state.current_frame_index >= sequence.frames.len() {
            return None;
        }

        let current_frame = &sequence.frames[state.current_frame_index];
        let next_frame = &sequence.frames[state.next_frame_index];

        // Convert coordinates to pixel positions FIRST
        let current_pos = self.coordinate_mapper.convert_coords(&current_frame.coords);
        let next_pos = self.coordinate_mapper.convert_coords(&next_frame.coords);

        // Check pixel distance - only interpolate if moving <= 16 pixels (1 tile)
        let pixel_dx = (next_pos[0] - current_pos[0]).abs();
        let pixel_dy = (next_pos[1] - current_pos[1]).abs();
        let pixel_distance = pixel_dx.max(pixel_dy);

        // Only interpolate if moving contiguously (1 tile = 16 pixels)
        let should_interpolate = pixel_distance <= 16.0;

        // If jumping > 16 pixels, don't interpolate - just show at current position
        let interpolation_t = if should_interpolate {
            state.interpolation_t
        } else {
            0.0
        };

        // Linear interpolation (or no interpolation if jumping)
        let position = [
            current_pos[0] + (next_pos[0] - current_pos[0]) * interpolation_t,
            current_pos[1] + (next_pos[1] - current_pos[1]) * interpolation_t,
        ];

        // Determine direction based on movement
        let dx = next_pos[0] - current_pos[0];
        let dy = next_pos[1] - current_pos[1];
        let direction = Direction::from_movement(dx, dy);

        Some(SpriteInstance {
            position,
            sprite_id: sequence.sprite_id,
            direction,
        })
    }

    /// Calculate total animation duration in milliseconds
    pub fn calculate_duration(&self, sequences: &[SpriteSequence]) -> f32 {
        sequences
            .iter()
            .map(|seq| seq.frames.len() as f32 * self.interval_ms)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }

    /// Get frame time in milliseconds for a given frame number
    pub fn frame_to_time(&self, frame_number: usize) -> f32 {
        frame_number as f32 * (1000.0 / self.fps)
    }

    /// Calculate total number of frames for the animation
    pub fn calculate_frame_count(&self, sequences: &[SpriteSequence]) -> usize {
        let duration_ms = self.calculate_duration(sequences);
        (duration_ms * self.fps / 1000.0).ceil() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{SpriteFrame, SpriteSequence};
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_animation_state_calculation() {
        let mapper = CoordinateMapper {
            regions: HashMap::new(),
        };
        let interpolator = AnimationInterpolator::new(mapper, 500.0, 60.0);

        let sequence = SpriteSequence {
            user: "test".to_string(),
            env_id: "1".to_string(),
            sprite_id: 0,
            color: "#000000".to_string(),
            frames: vec![
                SpriteFrame {
                    timestamp: Utc::now(),
                    user: "test".to_string(),
                    env_id: "1".to_string(),
                    sprite_id: 0,
                    color: "#000000".to_string(),
                    extra: String::new(),
                    coords: [0, 0, 1],
                },
                SpriteFrame {
                    timestamp: Utc::now(),
                    user: "test".to_string(),
                    env_id: "1".to_string(),
                    sprite_id: 0,
                    color: "#000000".to_string(),
                    extra: String::new(),
                    coords: [10, 0, 1],
                },
            ],
        };

        // At time 0, should be at first frame
        let state = interpolator.get_animation_state(&sequence, 0.0).unwrap();
        assert_eq!(state.current_frame_index, 0);
        assert_eq!(state.next_frame_index, 1);
        assert!((state.interpolation_t - 0.0).abs() < 0.01);

        // At time 250ms (halfway between frames), should be interpolating
        let state = interpolator.get_animation_state(&sequence, 250.0).unwrap();
        assert_eq!(state.current_frame_index, 0);
        assert_eq!(state.next_frame_index, 1);
        assert!((state.interpolation_t - 0.5).abs() < 0.01);

        // At time 500ms, should be at second frame
        let state = interpolator.get_animation_state(&sequence, 500.0).unwrap();
        assert_eq!(state.current_frame_index, 1);
        assert_eq!(state.next_frame_index, 1);
    }
}
