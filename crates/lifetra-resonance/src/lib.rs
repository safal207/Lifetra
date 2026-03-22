use lifetra_core::Scalar;

/// Alignment across the entity's inner state, environment, and temporal context.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResonanceState {
    pub self_alignment: Scalar,
    pub world_alignment: Scalar,
    pub temporal_alignment: Scalar,
}

impl ResonanceState {
    /// Creates a resonance state from three complementary alignment scores.
    pub fn new(
        self_alignment: Scalar,
        world_alignment: Scalar,
        temporal_alignment: Scalar,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&self_alignment));
        debug_assert!((0.0..=1.0).contains(&world_alignment));
        debug_assert!((0.0..=1.0).contains(&temporal_alignment));

        Self {
            self_alignment,
            world_alignment,
            temporal_alignment,
        }
    }

    /// Returns true when all alignment dimensions meet or exceed the given normalized threshold.
    ///
    /// The threshold is expected to be in the `0.0..=1.0` range.
    pub fn is_aligned(&self, threshold: Scalar) -> bool {
        debug_assert!((0.0..=1.0).contains(&threshold));

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
        // Fails at `0.8` because temporal alignment remains below the threshold.
        assert!(!state.is_aligned(0.8));
    }
}
