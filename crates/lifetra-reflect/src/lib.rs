use lifetra_core::Scalar;

/// Self-observation state, including confidence, coherence, blind spots, and contradictions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReflectionState {
    pub self_confidence: Scalar,
    pub perceived_coherence: Scalar,
    pub blind_spots: Vec<String>,
    pub contradictions: Vec<String>,
}

impl ReflectionState {
    /// Creates a reflection state from confidence, coherence, and observed limitations.
    pub fn new(
        self_confidence: Scalar,
        perceived_coherence: Scalar,
        blind_spots: Vec<String>,
        contradictions: Vec<String>,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&self_confidence));
        debug_assert!((0.0..=1.0).contains(&perceived_coherence));

        Self {
            self_confidence,
            perceived_coherence,
            blind_spots,
            contradictions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_internal_observation() {
        let state = ReflectionState::new(0.6, 0.7, vec!["bias".into()], vec!["tension".into()]);

        assert_eq!(state.blind_spots, vec![String::from("bias")]);
        assert_eq!(state.contradictions.len(), 1);
    }
}
