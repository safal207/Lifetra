use lifetra_causal::CausalState;
use lifetra_orient::OrientationVector;
use lifetra_reflect::ReflectionState;
use lifetra_resonance::ResonanceState;
use lifetra_synergy::SynergyState;
use lifetra_trajectory::TrajectoryState;

pub type Scalar = f32;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

impl EntityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub fn new(epoch_seconds: i64) -> Self {
        Self(epoch_seconds)
    }

    pub fn epoch_seconds(self) -> i64 {
        self.0
    }
}

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
            ReflectionState::new(0.6, 0.7, vec![], vec![]),
            ResonanceState::new(0.8, 0.7, 0.6),
            SynergyState::new(0.5, 0.4),
        );

        assert_eq!(entity.id.as_str(), "entity-1");
        assert_eq!(entity.orientation.toward_connection, 0.6);
    }

    #[test]
    fn timestamp_exposes_epoch_seconds() {
        let timestamp = Timestamp::new(123);

        assert_eq!(timestamp.epoch_seconds(), 123);
    }
}
