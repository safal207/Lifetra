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

    /// Returns true when any contradiction has been recorded.
    pub fn has_contradictions(&self) -> bool {
        !self.contradictions.is_empty()
    }

    /// Returns a normalized reflection score with small penalties for blind spots and contradictions.
    pub fn reflection_score(&self) -> Scalar {
        let base = (self.self_confidence + self.perceived_coherence) / 2.0;
        let blind_spot_penalty = (self.blind_spots.len() as Scalar * 0.03).min(0.2);
        let contradiction_penalty = (self.contradictions.len() as Scalar * 0.07).min(0.3);

        (base - blind_spot_penalty - contradiction_penalty).clamp(0.0, 1.0)
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

    #[test]
    fn detects_contradictions_and_scores_reflection() {
        let state = ReflectionState::new(
            0.8,
            0.75,
            vec!["unknown market".into()],
            vec!["speed conflicts with rigor".into()],
        );

        assert!(state.has_contradictions());
        assert!(state.reflection_score() < 0.775);
    }
}
