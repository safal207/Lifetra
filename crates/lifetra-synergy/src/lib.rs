#[derive(Debug, Clone, PartialEq)]
pub struct SynergyState {
    pub cooperative_potential: f32,
    pub emergent_value: f32,
}

impl SynergyState {
    pub fn new(cooperative_potential: f32, emergent_value: f32) -> Self {
        Self {
            cooperative_potential,
            emergent_value,
        }
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
}
