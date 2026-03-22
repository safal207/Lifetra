/// Emergent collaborative capacity of an entity in relation to others.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SynergyState {
    pub cooperative_potential: f32,
    pub emergent_value: f32,
}

impl SynergyState {
    /// Creates a synergy state from collaborative potential and realized emergence.
    pub fn new(cooperative_potential: f32, emergent_value: f32) -> Self {
        Self {
            cooperative_potential,
            emergent_value,
        }
    }

    /// Returns true when collaboration is both promising and already generative.
    pub fn is_productive(&self, threshold: f32) -> bool {
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
