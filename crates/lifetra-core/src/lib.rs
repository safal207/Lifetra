use lifetra_causal::CausalState;
use lifetra_orient::OrientationVector;
use lifetra_reflect::ReflectionState;
use lifetra_resonance::ResonanceState;
use lifetra_synergy::SynergyState;
use lifetra_trajectory::TrajectoryState;

/// Shared scalar type for lightweight domain measurements.
pub type Scalar = f32;

/// Stable identifier for a modeled entity or idea.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

impl EntityId {
    /// Creates a new entity identifier from any string-like value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Timestamp represented as seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates a timestamp from epoch seconds.
    pub fn new(epoch_seconds: i64) -> Self {
        Self(epoch_seconds)
    }

    /// Returns the timestamp as epoch seconds.
    pub fn epoch_seconds(self) -> i64 {
        self.0
    }
}

/// Complete state of a living entity across Lifetra's core dimensions.
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
    fn timestamp_exposes_epoch_seconds() {
        let timestamp = Timestamp::new(123);

        assert_eq!(timestamp.epoch_seconds(), 123);
    }
}
