use lifetra_causal::CausalState;
use lifetra_core::{EntityId, Scalar};
use lifetra_orient::OrientationVector;
use lifetra_reflect::ReflectionState;
use lifetra_resonance::ResonanceState;
use lifetra_synergy::SynergyState;
use lifetra_trajectory::TrajectoryState;

/// Complete state of a living entity across Lifetra's semantic dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityState {
    pub id: EntityId,
    pub causality: CausalState,
    pub orientation: OrientationVector,
    pub trajectory: TrajectoryState,
    pub reflection: ReflectionState,
    pub resonance: ResonanceState,
    pub synergy: SynergyState,
}

impl EntityState {
    /// Creates a full entity state from the six semantic dimensions plus identity.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: EntityId,
        causality: CausalState,
        orientation: OrientationVector,
        trajectory: TrajectoryState,
        reflection: ReflectionState,
        resonance: ResonanceState,
        synergy: SynergyState,
    ) -> Self {
        Self {
            id,
            causality,
            orientation,
            trajectory,
            reflection,
            resonance,
            synergy,
        }
    }

    /// Builds a compact textual summary of the entity's current condition.
    pub fn summary(&self) -> String {
        let (axis, axis_score) = self.orientation.dominant_axis();
        let strongest_link = self
            .causality
            .strongest_link()
            .map(|link| format!("{} ({:.2})", link.source, link.influence))
            .unwrap_or_else(|| "none".to_string());
        let latest_transition = self
            .trajectory
            .latest_transition()
            .map(|transition| transition.label.as_str())
            .unwrap_or("none");

        format!(
            "Entity `{}` is in {} stage with dominant orientation {} ({:.2}), \
health {:.2}, strongest cause {}, latest transition {}, resonance {:.2}, synergy {:.2}.",
            self.id.as_str(),
            self.trajectory.stage,
            axis,
            axis_score,
            self.health_score(),
            strongest_link,
            latest_transition,
            self.resonance.average_alignment(),
            self.synergy.combined_score(),
        )
    }

    /// Returns a coarse normalized health score across the six semantic dimensions.
    pub fn health_score(&self) -> Scalar {
        let causal = self.causality.influence_balance;
        let orientation = self.orientation.average_intensity();
        let trajectory = (self.trajectory.momentum + self.trajectory.stability) / 2.0;
        let reflection = self.reflection.reflection_score();
        let resonance = self.resonance.average_alignment();
        let synergy = self.synergy.combined_score();

        ((causal + orientation + trajectory + reflection + resonance + synergy) / 6.0)
            .clamp(0.0, 1.0)
    }

    /// Returns true when the entity clears the threshold for health, average
    /// resonance, and reflection quality while also carrying no explicit
    /// contradictions.
    pub fn is_coherent(&self, threshold: Scalar) -> bool {
        debug_assert!((0.0..=1.0).contains(&threshold));

        self.health_score() >= threshold
            && self.resonance.average_alignment() >= threshold
            && self.reflection.reflection_score() >= threshold
            && !self.reflection.has_contradictions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifetra_causal::CausalLink;
    use lifetra_trajectory::LifecycleStage;

    #[test]
    fn constructs_entity_state_from_domain_dimensions() {
        let entity = EntityState::new(
            EntityId::new("entity-1"),
            CausalState::new(vec![CausalLink::new("seed", 1.0)], 1.0),
            OrientationVector::new(0.7, 0.5, 0.8, 0.6),
            TrajectoryState::new(LifecycleStage::Emerging, 0.4, 0.5),
            ReflectionState::default(),
            ResonanceState::default(),
            SynergyState::default(),
        );

        assert_eq!(entity.id.as_str(), "entity-1");
        assert_eq!(entity.orientation.toward_connection, 0.6);
    }

    #[test]
    fn summarizes_and_scores_entity_state() {
        let entity = EntityState::new(
            EntityId::new("entity-2"),
            CausalState::new(
                vec![
                    CausalLink::new("research", 0.8),
                    CausalLink::new("dialogue", 0.7),
                ],
                0.78,
            ),
            OrientationVector::new(0.85, 0.5, 0.92, 0.81),
            TrajectoryState::new(LifecycleStage::Evolving, 0.76, 0.64),
            ReflectionState::new(0.79, 0.82, vec![], vec![]),
            ResonanceState::new(0.84, 0.79, 0.77),
            SynergyState::new(0.88, 0.74),
        );

        let summary = entity.summary();

        assert!(summary.contains("entity-2"));
        assert!(summary.contains("truth"));
        assert!(entity.health_score() > 0.7);
        assert!(entity.is_coherent(0.7));
    }

    #[test]
    fn coherence_fails_when_contradictions_remain() {
        let entity = EntityState::new(
            EntityId::new("entity-3"),
            CausalState::new(vec![CausalLink::new("conflict", 0.7)], 0.7),
            OrientationVector::new(0.8, 0.72, 0.75, 0.7),
            TrajectoryState::new(LifecycleStage::Stabilizing, 0.74, 0.71),
            ReflectionState::new(0.8, 0.78, vec![], vec!["goal mismatch".into()]),
            ResonanceState::new(0.8, 0.79, 0.76),
            SynergyState::new(0.77, 0.74),
        );

        assert!(!entity.is_coherent(0.65));
    }
}
