//! Public facade for the Lifetra workspace.
//!
//! This crate re-exports the core domain types for modeling living trajectories
//! of ideas and entities across causality, orientation, trajectory, reflection,
//! resonance, and synergy.

pub use lifetra_causal::{CausalLink, CausalState};
pub use lifetra_core::{EntityId, Scalar, Timestamp};
pub use lifetra_entity::EntityState;
pub use lifetra_orient::OrientationVector;
pub use lifetra_reflect::ReflectionState;
pub use lifetra_resonance::ResonanceState;
pub use lifetra_synergy::SynergyState;
pub use lifetra_trajectory::{LifecycleStage, StateTransition, TrajectoryState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexports_support_entity_state_composition() {
        let entity = EntityState::new(
            EntityId::new("seed"),
            CausalState::new(vec![CausalLink::new("origin", 0.8)], 0.72),
            OrientationVector::new(0.9, 0.6, 0.8, 0.7),
            TrajectoryState::new(LifecycleStage::Emerging, 0.5, 0.4),
            ReflectionState::default(),
            ResonanceState::new(0.8, 0.75, 0.7),
            SynergyState::new(0.85, 0.65),
        );

        assert_eq!(entity.id.as_str(), "seed");
        assert_eq!(entity.causality.links.len(), 1);
        assert_eq!(entity.orientation.toward_truth, 0.8);
    }
}
