#[derive(Debug, Clone, PartialEq)]
pub struct ResonanceState {
    pub self_alignment: f32,
    pub world_alignment: f32,
    pub temporal_alignment: f32,
}

impl ResonanceState {
    pub fn new(self_alignment: f32, world_alignment: f32, temporal_alignment: f32) -> Self {
        Self {
            self_alignment,
            world_alignment,
            temporal_alignment,
        }
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
}
