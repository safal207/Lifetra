//! Public facade for the Lifetra workspace.
//!
//! This crate re-exports the core domain types for modeling living trajectories
//! of ideas and entities across causality, orientation, trajectory, reflection,
//! resonance, synergy, bounded trajectory beads, proof-carrying bead chains,
//! proof-backed orientation deltas, bounded correction policies, decision authority,
//! identity-preserving execution receipts, reconciliation-gated retry authority,
//! append-only physical attempt ledgers, fsync-backed durable recovery journals,
//! provider-neutral reconciliation adapters, and recovery-lease fencing.

mod attempt_ledger;
mod correction_policy;
mod durable_journal;
mod execution_receipt;
mod orientation_delta;
mod provider_reconciliation;
mod reconciliation;
mod recovery_lease;
mod safety_authority;

pub use attempt_ledger::{
    AttemptBlock, AttemptDispatchReceipt, AttemptExternalReceipt, AttemptId, AttemptLedger,
    AttemptReconciliationReceipt, AttemptRecord, AttemptStatus,
};
pub use correction_policy::{
    CorrectionBlock, CorrectionDecision, CorrectionMemory, CorrectionPolicy, OrientationAdjustment,
};
pub use durable_journal::{
    DurableJournal, JournalError, PreparedAttempt, PreparedReconciliationReceipt, RecoveredRuntime,
    RecoveryDirective,
};
pub use execution_receipt::{
    ActionId, AuthorityTicket, DispatchReceipt, ExecutionBlock, ExecutionOutcome, ExecutionStatus,
    ExecutionTrace, ExternalExecutionReceipt,
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
pub use provider_reconciliation::{
    ProviderObservation, ProviderReconciler, ProviderReconciliationAdapter,
    ProviderReconciliationError, ProviderReconciliationPhase, ProviderReconciliationQuery,
    ProviderReconciliationResult,
};
pub use reconciliation::{
    IdempotencyBinding, ReconciliationOutcome, ReconciliationReceipt, RetryAuthority, RetryBlock,
    RetryContext, RetryDecision, RetryPolicy, RetryReason, RetryVerdict,
};
pub use recovery_lease::{
    FencedAttemptPermit, FencedDurableJournal, FencedJournalError, FencingToken,
    InMemoryLeaseError, InMemoryRecoveryLeaseStore, LeaseConfigBlock, LeaseVersion, RecoveryLease,
    RecoveryLeaseBlock, RecoveryLeaseManager, RecoveryLeaseStore, RecoveryWorkerId,
};
pub use safety_authority::{
    ApprovalState, AuthorityContext, AuthorityDecision, AuthorityReason, AuthorityVerdict,
    AutonomyLevel, DecisionAuthority, ExecutionMode, SafetyConfigBlock, SafetyEnvelope,
};

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

    #[test]
    fn facade_exposes_decision_authority_gate() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:authority"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:authority",
            EvidenceStatus::Supported,
            "verified movement",
        ));
        let commit = bead
            .prove_transition("move", "verified")
            .expect("bead should commit");
        let proven =
            ProvenOrientation::from_commit(OrientationVector::new(0.6, 0.5, 0.8, 0.5), &commit)
                .expect("commit should back movement");
        let delta = OrientationDelta::between(OrientationVector::new(0.8, 0.5, 0.8, 0.5), proven);
        let correction = CorrectionPolicy::new(0.5, 0.1, 0.05, 0.1)
            .expect("valid correction policy")
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce correction");
        let envelope =
            SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.10, 0.25)
                .expect("valid safety envelope");
        let authority = DecisionAuthority::new(envelope);
        let gate = authority.evaluate(&correction, AuthorityContext::default());

        assert_eq!(
            gate.verdict,
            AuthorityVerdict::Allow(ExecutionMode::Automatic)
        );
    }

    #[test]
    fn facade_exposes_authority_ticket_and_execution_receipts() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:execution"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:execution",
            EvidenceStatus::Supported,
            "verified movement",
        ));
        let commit = bead
            .prove_transition("move", "verified")
            .expect("bead should commit");
        let proven =
            ProvenOrientation::from_commit(OrientationVector::new(0.55, 0.5, 0.8, 0.5), &commit)
                .expect("commit should back movement");
        let delta = OrientationDelta::between(OrientationVector::new(0.60, 0.5, 0.8, 0.5), proven);
        let correction = CorrectionPolicy::new(0.5, 0.01, 0.0, 0.05)
            .expect("valid correction policy")
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce correction");
        let envelope =
            SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.10, 0.25)
                .expect("valid safety envelope");
        let authority =
            DecisionAuthority::new(envelope).evaluate(&correction, AuthorityContext::default());
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:execution").expect("valid action id"),
            &authority,
            Timestamp::new(2),
        )
        .expect("allowed authority should issue ticket");
        let mut trace = ExecutionTrace::new(ticket);

        trace
            .record_dispatch(Timestamp::new(3), "proof:dispatch")
            .expect("dispatch should record");
        assert_eq!(trace.status(), ExecutionStatus::DispatchedEffectUnknown);

        trace
            .record_external_outcome(
                Timestamp::new(4),
                ExecutionOutcome::Succeeded,
                "proof:external",
            )
            .expect("external outcome should record");
        assert_eq!(
            trace.status(),
            ExecutionStatus::EffectConfirmed(ExecutionOutcome::Succeeded)
        );
    }

    #[test]
    fn facade_exposes_reconciliation_gated_retry_authority() {
        let ticket = AuthorityTicket {
            action_id: ActionId::new("action:retry:facade").expect("valid action id"),
            source_bead: BeadId::new("bead:retry:facade"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:facade".into()],
            issued_at: Timestamp::new(10),
        };
        let binding = IdempotencyBinding::new(&ticket, "idem:retry:facade").expect("valid key");
        let mut trace = ExecutionTrace::new(ticket);
        trace
            .record_dispatch(Timestamp::new(11), "proof:dispatch:facade")
            .expect("dispatch should record");

        let authority = RetryAuthority::new(RetryPolicy::new(1, false, true));
        let before = authority
            .evaluate(&trace, &binding, None, RetryContext::default())
            .expect("valid evaluation");
        assert_eq!(before.verdict, RetryVerdict::ReconcileFirst);

        let receipt = ReconciliationReceipt::new(
            trace.ticket.action_id.clone(),
            Timestamp::new(12),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:reconcile:facade",
        )
        .expect("valid reconciliation receipt");
        let after = authority
            .evaluate(&trace, &binding, Some(&receipt), RetryContext::default())
            .expect("valid evaluation");
        assert_eq!(
            after.verdict,
            RetryVerdict::RedispatchAllowed { ordinal: 1 }
        );
    }

    #[test]
    fn facade_exposes_append_only_attempt_ledger() {
        let ticket = AuthorityTicket {
            action_id: ActionId::new("action:attempt-ledger:facade").expect("valid action id"),
            source_bead: BeadId::new("bead:attempt-ledger:facade"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:attempt-ledger".into()],
            issued_at: Timestamp::new(10),
        };
        let binding =
            IdempotencyBinding::new(&ticket, "idem:attempt-ledger:facade").expect("valid key");
        let mut ledger = AttemptLedger::new(ticket, binding).expect("valid attempt ledger");
        let retry_authority = RetryAuthority::new(RetryPolicy::new(1, false, true));

        let (trace, reconciliation, context) = ledger.retry_inputs().expect("initial retry inputs");
        let initial = retry_authority
            .evaluate(&trace, &ledger.binding, reconciliation.as_ref(), context)
            .expect("initial dispatch decision");
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:attempt:0")
            .expect("initial attempt should record");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:attempt:0",
            )
            .expect("attempt reconciliation should record");

        let (trace, reconciliation, context) = ledger.retry_inputs().expect("retry inputs");
        let retry = retry_authority
            .evaluate(&trace, &ledger.binding, reconciliation.as_ref(), context)
            .expect("redispatch decision");
        let second_id = ledger
            .record_dispatch(&retry, Timestamp::new(13), "proof:dispatch:attempt:1")
            .expect("second attempt should record")
            .id
            .clone();

        assert_eq!(ledger.attempts().len(), 2);
        assert_eq!(second_id.ordinal, 1);
        assert_eq!(second_id.action_id, ledger.ticket.action_id);
        assert_eq!(ledger.retry_context().redispatches_used, 1);
        assert_eq!(ledger.binding.key, "idem:attempt-ledger:facade");
    }
}
