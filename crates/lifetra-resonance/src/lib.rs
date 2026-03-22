/// Alignment across the entity's inner state, environment, and temporal context.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResonanceState {
    pub self_alignment: f32,
    pub world_alignment: f32,
    pub temporal_alignment: f32,
}

impl ResonanceState {
    /// Creates a resonance state from three complementary alignment scores.
    pub fn new(self_alignment: f32, world_alignment: f32, temporal_alignment: f32) -> Self {
        Self {
            self_alignment,
            world_alignment,
            temporal_alignment,
        }
    }

    /// Returns true when all alignment dimensions meet or exceed the given threshold.
    pub fn is_aligned(&self, threshold: f32) -> bool {
        self.self_alignment >= threshold
            && self.world_alignment >= threshold
            && self.temporal_alignment >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_alignment_across_dimensions() {
        let state = ResonanceState::new(0.8, 0.7, 0.6);

        assert!(state.self_alignment >= state.temporal_alignment);
    }

    #[test]
    fn evaluates_threshold_alignment() {
        let state = ResonanceState::new(0.85, 0.8, 0.75);

        assert!(state.is_aligned(0.7));
        assert!(!state.is_aligned(0.8));
    }
}
