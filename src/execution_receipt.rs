use lifetra_bead::BeadId;
use lifetra_core::Timestamp;

use crate::{AuthorityDecision, AuthorityVerdict, ExecutionMode};

/// Stable identity for one authorized external action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);

impl ActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ExecutionBlock> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ExecutionBlock::EmptyActionId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Permission receipt issued only from an allowed authority decision.
///
/// A ticket authorizes an action identity. It is not proof that dispatch or
/// any external effect has occurred.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorityTicket {
    pub action_id: ActionId,
    pub source_bead: BeadId,
    pub execution_mode: ExecutionMode,
    pub authority_proof_refs: Vec<String>,
    pub issued_at: Timestamp,
}

/// Evidence that an authorized action was actually dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchReceipt {
    pub action_id: ActionId,
    pub dispatched_at: Timestamp,
    pub proof_ref: String,
}

/// Externally observed result for the dispatched action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Succeeded,
    Failed,
}

/// Evidence that the external effect reached a known outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalExecutionReceipt {
    pub action_id: ActionId,
    pub observed_at: Timestamp,
    pub outcome: ExecutionOutcome,
    pub proof_ref: String,
}

/// Explicit execution state. Dispatch without an external outcome remains
/// unknown instead of being collapsed into success or failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    AuthorizedNotDispatched,
    DispatchedEffectUnknown,
    EffectConfirmed(ExecutionOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionBlock {
    EmptyActionId,
    AuthorityDidNotAllow,
    EmptyDispatchProof,
    EmptyExternalProof,
    DispatchAlreadyRecorded,
    ExternalOutcomeAlreadyRecorded,
    DispatchNotRecorded,
    TemporalOrderViolation,
}

/// Identity-preserving execution trace from authority through external effect.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionTrace {
    pub ticket: AuthorityTicket,
    pub dispatch: Option<DispatchReceipt>,
    pub external: Option<ExternalExecutionReceipt>,
}

impl AuthorityTicket {
    pub fn issue(
        action_id: ActionId,
        authority: &AuthorityDecision,
        issued_at: Timestamp,
    ) -> Result<Self, ExecutionBlock> {
        let execution_mode = match authority.verdict {
            AuthorityVerdict::Allow(mode) => mode,
            AuthorityVerdict::RequireApproval | AuthorityVerdict::Block => {
                return Err(ExecutionBlock::AuthorityDidNotAllow)
            }
        };

        Ok(Self {
            action_id,
            source_bead: authority.source_bead.clone(),
            execution_mode,
            authority_proof_refs: authority.proof_refs.clone(),
            issued_at,
        })
    }
}

impl ExecutionTrace {
    pub fn new(ticket: AuthorityTicket) -> Self {
        Self {
            ticket,
            dispatch: None,
            external: None,
        }
    }

    pub fn status(&self) -> ExecutionStatus {
        match (&self.dispatch, &self.external) {
            (_, Some(receipt)) => ExecutionStatus::EffectConfirmed(receipt.outcome),
            (Some(_), None) => ExecutionStatus::DispatchedEffectUnknown,
            (None, None) => ExecutionStatus::AuthorizedNotDispatched,
        }
    }

    /// Records proof that the authorized action crossed the dispatch boundary.
    pub fn record_dispatch(
        &mut self,
        dispatched_at: Timestamp,
        proof_ref: impl Into<String>,
    ) -> Result<&DispatchReceipt, ExecutionBlock> {
        if self.dispatch.is_some() {
            return Err(ExecutionBlock::DispatchAlreadyRecorded);
        }
        if dispatched_at < self.ticket.issued_at {
            return Err(ExecutionBlock::TemporalOrderViolation);
        }

        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(ExecutionBlock::EmptyDispatchProof);
        }

        self.dispatch = Some(DispatchReceipt {
            action_id: self.ticket.action_id.clone(),
            dispatched_at,
            proof_ref,
        });
        Ok(self.dispatch.as_ref().expect("dispatch was just recorded"))
    }

    /// Records an externally verifiable outcome after dispatch.
    pub fn record_external_outcome(
        &mut self,
        observed_at: Timestamp,
        outcome: ExecutionOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<&ExternalExecutionReceipt, ExecutionBlock> {
        if self.external.is_some() {
            return Err(ExecutionBlock::ExternalOutcomeAlreadyRecorded);
        }

        let dispatch = self
            .dispatch
            .as_ref()
            .ok_or(ExecutionBlock::DispatchNotRecorded)?;
        if observed_at < dispatch.dispatched_at {
            return Err(ExecutionBlock::TemporalOrderViolation);
        }

        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(ExecutionBlock::EmptyExternalProof);
        }

        self.external = Some(ExternalExecutionReceipt {
            action_id: self.ticket.action_id.clone(),
            observed_at,
            outcome,
            proof_ref,
        });
        Ok(self
            .external
            .as_ref()
            .expect("external outcome was just recorded"))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ApprovalState, AuthorityContext, AutonomyLevel, BeadId, BeadScale, CorrectionMemory,
        CorrectionPolicy, DecisionAuthority, EvidenceRef, EvidenceStatus, OrientationDelta,
        OrientationVector, ProvenOrientation, SafetyEnvelope, Timestamp, TrajectoryBead,
    };

    use super::*;

    fn allowed_authority() -> AuthorityDecision {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:execution:1"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:movement:1",
            EvidenceStatus::Supported,
            "external movement proof",
        ));
        let commit = bead
            .prove_transition("moved", "verified movement")
            .expect("proof-backed bead should commit");
        let proven =
            ProvenOrientation::from_commit(OrientationVector::new(0.55, 0.5, 0.5, 0.5), &commit)
                .expect("commit carries proof");
        let delta = OrientationDelta::between(OrientationVector::new(0.60, 0.5, 0.5, 0.5), proven);
        let correction = CorrectionPolicy::new(0.5, 0.01, 0.0, 0.05)
            .expect("valid correction policy")
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce correction");
        let envelope =
            SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.10, 0.25)
                .expect("valid safety envelope");

        DecisionAuthority::new(envelope).evaluate(&correction, AuthorityContext::default())
    }

    #[test]
    fn ticket_can_only_be_issued_from_allow() {
        let mut decision = allowed_authority();
        decision.verdict = AuthorityVerdict::RequireApproval;

        let result = AuthorityTicket::issue(
            ActionId::new("action:1").expect("valid id"),
            &decision,
            Timestamp::new(10),
        );

        assert_eq!(result, Err(ExecutionBlock::AuthorityDidNotAllow));
    }

    #[test]
    fn ticket_preserves_action_identity_and_authority_lineage() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:42").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("allowed decision should issue ticket");

        assert_eq!(ticket.action_id.as_str(), "action:42");
        assert_eq!(ticket.source_bead.as_str(), "bead:execution:1");
        assert_eq!(ticket.authority_proof_refs, vec!["proof:movement:1"]);
    }

    #[test]
    fn dispatch_does_not_imply_external_success() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:dispatch").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("ticket");
        let mut trace = ExecutionTrace::new(ticket);

        trace
            .record_dispatch(Timestamp::new(11), "proof:dispatch:1")
            .expect("dispatch should record");

        assert_eq!(trace.status(), ExecutionStatus::DispatchedEffectUnknown);
        assert!(trace.external.is_none());
    }

    #[test]
    fn external_outcome_requires_dispatch_first() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:no-dispatch").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("ticket");
        let mut trace = ExecutionTrace::new(ticket);

        assert_eq!(
            trace.record_external_outcome(
                Timestamp::new(12),
                ExecutionOutcome::Succeeded,
                "proof:external:1",
            ),
            Err(ExecutionBlock::DispatchNotRecorded)
        );
    }

    #[test]
    fn external_receipt_confirms_success_or_failure_without_changing_identity() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:resolved").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("ticket");
        let mut trace = ExecutionTrace::new(ticket);
        trace
            .record_dispatch(Timestamp::new(11), "proof:dispatch:resolved")
            .expect("dispatch should record");
        let external = trace
            .record_external_outcome(
                Timestamp::new(12),
                ExecutionOutcome::Failed,
                "proof:external:failed",
            )
            .expect("outcome should record");

        assert_eq!(external.action_id.as_str(), "action:resolved");
        assert_eq!(
            trace.status(),
            ExecutionStatus::EffectConfirmed(ExecutionOutcome::Failed)
        );
    }

    #[test]
    fn duplicate_or_temporally_invalid_receipts_are_rejected() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:strict").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("ticket");
        let mut trace = ExecutionTrace::new(ticket);

        assert_eq!(
            trace.record_dispatch(Timestamp::new(9), "proof:too-early"),
            Err(ExecutionBlock::TemporalOrderViolation)
        );
        trace
            .record_dispatch(Timestamp::new(11), "proof:dispatch")
            .expect("valid dispatch");
        assert_eq!(
            trace.record_dispatch(Timestamp::new(12), "proof:dispatch:duplicate"),
            Err(ExecutionBlock::DispatchAlreadyRecorded)
        );
        assert_eq!(
            trace.record_external_outcome(
                Timestamp::new(10),
                ExecutionOutcome::Succeeded,
                "proof:too-early:external",
            ),
            Err(ExecutionBlock::TemporalOrderViolation)
        );
    }

    #[test]
    fn approval_state_is_not_used_as_execution_proof() {
        let decision = allowed_authority();
        let ticket = AuthorityTicket::issue(
            ActionId::new("action:approval-boundary").expect("valid id"),
            &decision,
            Timestamp::new(10),
        )
        .expect("ticket");
        let trace = ExecutionTrace::new(ticket);

        let approval = ApprovalState::Approved;
        assert_eq!(approval, ApprovalState::Approved);
        assert_eq!(trace.status(), ExecutionStatus::AuthorizedNotDispatched);
    }
}
