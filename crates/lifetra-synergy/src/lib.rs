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
}
