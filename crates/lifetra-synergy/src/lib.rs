use lifetra_core::Scalar;

/// Emergent collaborative capacity of an entity in relation to others.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SynergyState {
    pub cooperative_potential: Scalar,
    pub emergent_value: Scalar,
}

impl SynergyState {
    /// Creates a synergy state from collaborative potential and realized emergence.
    pub fn new(cooperative_potential: Scalar, emergent_value: Scalar) -> Self {
        debug_assert!((0.0..=1.0).contains(&cooperative_potential));
        debug_assert!((0.0..=1.0).contains(&emergent_value));

        Self {
            cooperative_potential,
            emergent_value,
        }
    }

    /// Returns true when collaboration is both promising and already generative.
    ///
    /// The threshold is expected to be in the `0.0..=1.0` range.
    pub fn is_productive(&self, threshold: Scalar) -> bool {
        debug_assert!((0.0..=1.0).contains(&threshold));

        self.cooperative_potential >= threshold && self.emergent_value >= threshold
    }

    /// Returns the average of collaborative potential and realized emergence.
    pub fn combined_score(&self) -> Scalar {
        (self.cooperative_potential + self.emergent_value) / 2.0
    }

    /// Returns the unrealized gap between potential and emergence.
    pub fn synergy_gap(&self) -> Scalar {
        (self.cooperative_potential - self.emergent_value).abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_cooperative_emergence() {
        let state = SynergyState::new(0.9, 0.8);

        assert!(state.cooperative_potential > state.emergent_value - 0.2);
    }

    #[test]
    fn detects_productive_collaboration() {
        let state = SynergyState::new(0.9, 0.82);

        assert!(state.is_productive(0.8));
        assert!(!state.is_productive(0.85));
    }

    #[test]
    fn computes_combined_score_and_gap() {
        let state = SynergyState::new(0.9, 0.7);

        assert!((state.combined_score() - 0.8).abs() < 0.000_1);
        assert!((state.synergy_gap() - 0.2).abs() < 0.000_1);
    }
}
