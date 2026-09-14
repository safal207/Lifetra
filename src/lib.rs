//! Public facade for the Lifetra workspace.
//!
//! This crate re-exports the core domain types for modeling living trajectories
//! of ideas and entities across causality, orientation, trajectory, reflection,
//! resonance, synergy, bounded trajectory beads, proof-carrying bead chains,
//! proof-backed orientation deltas, and bounded correction policies.

mod correction_policy;
mod orientation_delta;

pub use correction_policy::{
    CorrectionBlock, CorrectionDecision, CorrectionMemory, CorrectionPolicy, OrientationAdjustment,
};
pub use lifetra_bead::{
    AggregationBlock, BeadAggregate, BeadChain, BeadCommit, BeadId, BeadScale, ChainBlock,
    CommitBlock, EvidenceRef, EvidenceStatus, ProofCarry, SectorEdge, SectorGraph, SectorKind,
    SectorNode, TrajectoryBead,
};
pub use lifetra_causal::{CausalLink, CausalState};
pub use lifetra_core::{EntityId, Scalar, Timestamp};
pub use lifetra_entity::EntityState;
pub use lifetra_orient::{OrientationAxis, OrientationVector};
pub use lifetra_reflect::ReflectionState;
pub use lifetra_resonance::ResonanceState;
pub use lifetra_synergy::SynergyState;
pub use lifetra_trajectory::{LifecycleStage, StateTransition, TrajectoryState};
pub use orientation_delta::{OrientationBlock, OrientationDelta, ProvenOrientation};

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

    #[test]
    fn facade_exposes_evidence_gated_beads() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:test"),
            BeadScale::Event,
            Timestamp::new(1),
            Timestamp::new(2),
        )
        .with_evidence(EvidenceRef::new(
            "proof:1",
            EvidenceStatus::Supported,
            "verified externally",
        ));

        assert_eq!(bead.supported_evidence_count(), 1);
    }

    #[test]
    fn facade_exposes_proof_carrying_chains() {
        let first = TrajectoryBead::new(
            BeadId::new("b1"),
            BeadScale::Minute,
            Timestamp::new(0),
            Timestamp::new(60),
        )
        .with_evidence(EvidenceRef::new(
            "proof:1",
            EvidenceStatus::Supported,
            "verified",
        ));
        let commit = first
            .prove_transition("verified", "first step")
            .expect("proof-backed bead should commit");
        let second = TrajectoryBead::new(
            BeadId::new("b2"),
            BeadScale::Minute,
            Timestamp::new(60),
            Timestamp::new(120),
        );

        let mut chain = BeadChain::new();
        chain.append(first).expect("first bead should append");
        chain
            .append_with_commit(second, &commit)
            .expect("proof should carry into second bead");

        assert!(chain.proof_continuity_is_intact());
    }

    #[test]
    fn facade_exposes_proof_backed_orientation_delta() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:orientation"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:move",
            EvidenceStatus::Supported,
            "verified movement",
        ));
        let commit = bead
            .prove_transition("move", "verified")
            .expect("bead should commit");
        let proven =
            ProvenOrientation::from_commit(OrientationVector::new(0.7, 0.6, 0.9, 0.5), &commit)
                .expect("commit should back movement");
        let delta = OrientationDelta::between(OrientationVector::new(0.8, 0.6, 0.9, 0.5), proven);

        assert!(delta.alignment_score() > 0.97);
    }

    #[test]
    fn facade_exposes_bounded_correction_policy() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:correction"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:correction",
            EvidenceStatus::Supported,
            "verified movement",
        ));
        let commit = bead
            .prove_transition("move", "verified")
            .expect("bead should commit");
        let proven =
            ProvenOrientation::from_commit(OrientationVector::new(0.4, 0.5, 0.8, 0.5), &commit)
                .expect("commit should back movement");
        let delta = OrientationDelta::between(OrientationVector::new(0.8, 0.5, 0.8, 0.5), proven);
        let policy = CorrectionPolicy::new(0.5, 0.1, 0.05, 0.1).expect("valid policy");
        let decision = policy
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce correction");

        assert!((decision.adjustment.growth - 0.1).abs() < 0.000_1);
        assert!((decision.next_orientation.toward_growth - 0.9).abs() < 0.000_1);
    }
}
