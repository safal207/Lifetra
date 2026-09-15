use lifetra_core::Timestamp;

use crate::{
    ActionId, AuthorityTicket, DispatchReceipt, ExecutionOutcome, ExecutionTrace,
    ExternalExecutionReceipt, IdempotencyBinding, ReconciliationOutcome, ReconciliationReceipt,
    RetryContext, RetryDecision, RetryVerdict,
};

/// Stable identity for one physical dispatch attempt of a logical action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttemptId {
    pub action_id: ActionId,
    pub ordinal: u32,
}

impl AttemptId {
    pub fn new(action_id: ActionId, ordinal: u32) -> Self {
        Self { action_id, ordinal }
    }
}

/// Dispatch evidence scoped to one physical attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptDispatchReceipt {
    pub attempt_id: AttemptId,
    pub dispatched_at: Timestamp,
    pub proof_ref: String,
}

/// External outcome evidence scoped to one physical attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptExternalReceipt {
    pub attempt_id: AttemptId,
    pub observed_at: Timestamp,
    pub outcome: ExecutionOutcome,
    pub proof_ref: String,
}

/// Reconciliation evidence scoped to one physical attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptReconciliationReceipt {
    pub attempt_id: AttemptId,
    pub observed_at: Timestamp,
    pub outcome: ReconciliationOutcome,
    pub proof_ref: String,
}

/// Current knowledge about one physical dispatch attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptStatus {
    DispatchedEffectUnknown,
    Reconciled(ReconciliationOutcome),
    EffectConfirmed(ExecutionOutcome),
}

/// Append-only evidence record for one physical dispatch attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub id: AttemptId,
    pub dispatch: AttemptDispatchReceipt,
    pub authorization_proof_refs: Vec<String>,
    pub reconciliations: Vec<AttemptReconciliationReceipt>,
    pub external: Option<AttemptExternalReceipt>,
}

impl AttemptRecord {
    pub fn status(&self) -> AttemptStatus {
        if let Some(external) = &self.external {
            return AttemptStatus::EffectConfirmed(external.outcome);
        }

        if let Some(reconciliation) = self.reconciliations.last() {
            return AttemptStatus::Reconciled(reconciliation.outcome);
        }

        AttemptStatus::DispatchedEffectUnknown
    }

    pub fn latest_reconciliation(&self) -> Option<&AttemptReconciliationReceipt> {
        self.reconciliations.last()
    }

    pub fn has_conflicting_evidence(&self) -> bool {
        let mut terminal: Option<ReconciliationOutcome> = None;
        for receipt in &self.reconciliations {
            if receipt.outcome == ReconciliationOutcome::StillUnknown {
                continue;
            }
            if let Some(previous) = terminal {
                if previous != receipt.outcome {
                    return true;
                }
            } else {
                terminal = Some(receipt.outcome);
            }
        }

        match (&self.external, terminal) {
            (Some(external), Some(ReconciliationOutcome::EffectSucceeded)) => {
                external.outcome != ExecutionOutcome::Succeeded
            }
            (Some(external), Some(ReconciliationOutcome::EffectFailed)) => {
                external.outcome != ExecutionOutcome::Failed
            }
            (Some(_), Some(ReconciliationOutcome::NoEffectConfirmed)) => true,
            _ => false,
        }
    }

    fn retry_safe_resolution_proof(&self) -> Option<&str> {
        match self.status() {
            AttemptStatus::EffectConfirmed(ExecutionOutcome::Failed) => self
                .external
                .as_ref()
                .map(|receipt| receipt.proof_ref.as_str()),
            AttemptStatus::Reconciled(ReconciliationOutcome::EffectFailed)
            | AttemptStatus::Reconciled(ReconciliationOutcome::NoEffectConfirmed) => self
                .latest_reconciliation()
                .map(|receipt| receipt.proof_ref.as_str()),
            AttemptStatus::DispatchedEffectUnknown
            | AttemptStatus::EffectConfirmed(ExecutionOutcome::Succeeded)
            | AttemptStatus::Reconciled(ReconciliationOutcome::EffectSucceeded)
            | AttemptStatus::Reconciled(ReconciliationOutcome::StillUnknown) => None,
        }
    }

    fn latest_observed_at(&self) -> Timestamp {
        let mut latest = self.dispatch.dispatched_at;
        if let Some(external) = &self.external {
            if external.observed_at > latest {
                latest = external.observed_at;
            }
        }
        for reconciliation in &self.reconciliations {
            if reconciliation.observed_at > latest {
                latest = reconciliation.observed_at;
            }
        }
        latest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptBlock {
    IdempotencyActionMismatch,
    RetryDecisionActionMismatch,
    RetryDecisionIdempotencyMismatch,
    RetryDecisionDoesNotAuthorizeDispatch,
    InitialAttemptAlreadyRecorded,
    UnexpectedAttemptOrdinal { expected: u32, received: u32 },
    PreviousAttemptNotRetrySafe,
    RetryProofLineageMissing,
    PriorAttemptSucceeded,
    ConflictingEvidence,
    EmptyDispatchProof,
    EmptyExternalProof,
    EmptyReconciliationProof,
    UnknownAttempt { ordinal: u32 },
    ExternalOutcomeAlreadyRecorded { ordinal: u32 },
    TemporalOrderViolation,
}

/// Append-only ledger of physical attempts for one logical `ActionId`.
///
/// The ledger keeps the provider-facing idempotency binding stable while each
/// network dispatch receives a distinct ordinal. It never treats the existence
/// of an idempotency key as permission to retry.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptLedger {
    pub ticket: AuthorityTicket,
    pub binding: IdempotencyBinding,
    attempts: Vec<AttemptRecord>,
}

impl AttemptLedger {
    pub fn new(ticket: AuthorityTicket, binding: IdempotencyBinding) -> Result<Self, AttemptBlock> {
        if binding.action_id != ticket.action_id {
            return Err(AttemptBlock::IdempotencyActionMismatch);
        }

        Ok(Self {
            ticket,
            binding,
            attempts: Vec::new(),
        })
    }

    pub fn attempts(&self) -> &[AttemptRecord] {
        &self.attempts
    }

    pub fn latest(&self) -> Option<&AttemptRecord> {
        self.attempts.last()
    }

    pub fn retry_context(&self) -> RetryContext {
        RetryContext {
            redispatches_used: self.attempts.len().saturating_sub(1) as u32,
        }
    }

    pub fn has_conflicting_evidence(&self) -> bool {
        self.attempts
            .iter()
            .any(AttemptRecord::has_conflicting_evidence)
    }

    /// Records one physical dispatch authorized by a retry decision.
    ///
    /// Initial dispatch is attempt `0`. A redispatch ordinal must exactly match
    /// the next ledger ordinal and the previous attempt must already have a
    /// retry-safe evidence state (`Failed` or `NoEffectConfirmed`).
    pub fn record_dispatch(
        &mut self,
        decision: &RetryDecision,
        dispatched_at: Timestamp,
        proof_ref: impl Into<String>,
    ) -> Result<&AttemptRecord, AttemptBlock> {
        self.validate_decision_identity(decision)?;
        if self.has_conflicting_evidence() {
            return Err(AttemptBlock::ConflictingEvidence);
        }
        if self.prior_attempt_succeeded() {
            return Err(AttemptBlock::PriorAttemptSucceeded);
        }

        let ordinal = match decision.verdict {
            RetryVerdict::InitialDispatchAllowed => {
                if !self.attempts.is_empty() {
                    return Err(AttemptBlock::InitialAttemptAlreadyRecorded);
                }
                0
            }
            RetryVerdict::RedispatchAllowed { ordinal } => {
                let expected = self.attempts.len() as u32;
                if ordinal != expected {
                    return Err(AttemptBlock::UnexpectedAttemptOrdinal {
                        expected,
                        received: ordinal,
                    });
                }

                let previous = self
                    .attempts
                    .last()
                    .ok_or(AttemptBlock::PreviousAttemptNotRetrySafe)?;
                let required_proof = previous
                    .retry_safe_resolution_proof()
                    .ok_or(AttemptBlock::PreviousAttemptNotRetrySafe)?;
                if !decision
                    .proof_refs
                    .iter()
                    .any(|proof| proof == required_proof)
                {
                    return Err(AttemptBlock::RetryProofLineageMissing);
                }
                if dispatched_at < previous.latest_observed_at() {
                    return Err(AttemptBlock::TemporalOrderViolation);
                }
                ordinal
            }
            RetryVerdict::ReconcileFirst
            | RetryVerdict::CloseSucceeded
            | RetryVerdict::CloseFailed
            | RetryVerdict::Block => {
                return Err(AttemptBlock::RetryDecisionDoesNotAuthorizeDispatch)
            }
        };

        if dispatched_at < self.ticket.issued_at {
            return Err(AttemptBlock::TemporalOrderViolation);
        }

        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(AttemptBlock::EmptyDispatchProof);
        }

        let id = AttemptId::new(self.ticket.action_id.clone(), ordinal);
        self.attempts.push(AttemptRecord {
            id: id.clone(),
            dispatch: AttemptDispatchReceipt {
                attempt_id: id,
                dispatched_at,
                proof_ref,
            },
            authorization_proof_refs: decision.proof_refs.clone(),
            reconciliations: Vec::new(),
            external: None,
        });

        Ok(self
            .attempts
            .last()
            .expect("attempt was appended immediately before lookup"))
    }

    pub fn record_external_outcome(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ExecutionOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<&AttemptExternalReceipt, AttemptBlock> {
        let attempt = self.attempt_mut(ordinal)?;
        if attempt.external.is_some() {
            return Err(AttemptBlock::ExternalOutcomeAlreadyRecorded { ordinal });
        }
        if observed_at < attempt.dispatch.dispatched_at {
            return Err(AttemptBlock::TemporalOrderViolation);
        }

        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(AttemptBlock::EmptyExternalProof);
        }

        attempt.external = Some(AttemptExternalReceipt {
            attempt_id: attempt.id.clone(),
            observed_at,
            outcome,
            proof_ref,
        });

        Ok(attempt
            .external
            .as_ref()
            .expect("external receipt was recorded immediately before lookup"))
    }

    pub fn record_reconciliation(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ReconciliationOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<&AttemptReconciliationReceipt, AttemptBlock> {
        let attempt = self.attempt_mut(ordinal)?;
        if observed_at < attempt.dispatch.dispatched_at {
            return Err(AttemptBlock::TemporalOrderViolation);
        }

        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(AttemptBlock::EmptyReconciliationProof);
        }

        attempt.reconciliations.push(AttemptReconciliationReceipt {
            attempt_id: attempt.id.clone(),
            observed_at,
            outcome,
            proof_ref,
        });

        Ok(attempt
            .reconciliations
            .last()
            .expect("reconciliation was appended immediately before lookup"))
    }

    /// Builds the existing retry-authority inputs from the latest physical
    /// attempt while preserving the complete prior-attempt ledger.
    ///
    /// A late success on an older attempt or contradictory evidence blocks this
    /// bridge so callers cannot silently continue retrying through ambiguity.
    pub fn retry_inputs(
        &self,
    ) -> Result<(ExecutionTrace, Option<ReconciliationReceipt>, RetryContext), AttemptBlock> {
        if self.has_conflicting_evidence() {
            return Err(AttemptBlock::ConflictingEvidence);
        }
        if self.prior_attempt_succeeded() {
            return Err(AttemptBlock::PriorAttemptSucceeded);
        }

        let Some(latest) = self.attempts.last() else {
            return Ok((
                ExecutionTrace::new(self.ticket.clone()),
                None,
                RetryContext::default(),
            ));
        };

        let trace = ExecutionTrace {
            ticket: self.ticket.clone(),
            dispatch: Some(DispatchReceipt {
                action_id: self.ticket.action_id.clone(),
                dispatched_at: latest.dispatch.dispatched_at,
                proof_ref: latest.dispatch.proof_ref.clone(),
            }),
            external: latest
                .external
                .as_ref()
                .map(|external| ExternalExecutionReceipt {
                    action_id: self.ticket.action_id.clone(),
                    observed_at: external.observed_at,
                    outcome: external.outcome,
                    proof_ref: external.proof_ref.clone(),
                }),
        };

        let reconciliation = latest
            .latest_reconciliation()
            .map(|receipt| ReconciliationReceipt {
                action_id: self.ticket.action_id.clone(),
                observed_at: receipt.observed_at,
                outcome: receipt.outcome,
                proof_ref: receipt.proof_ref.clone(),
            });

        Ok((trace, reconciliation, self.retry_context()))
    }

    fn validate_decision_identity(&self, decision: &RetryDecision) -> Result<(), AttemptBlock> {
        if decision.action_id != self.ticket.action_id {
            return Err(AttemptBlock::RetryDecisionActionMismatch);
        }
        if decision.idempotency_key != self.binding.key {
            return Err(AttemptBlock::RetryDecisionIdempotencyMismatch);
        }
        Ok(())
    }

    fn attempt_mut(&mut self, ordinal: u32) -> Result<&mut AttemptRecord, AttemptBlock> {
        self.attempts
            .get_mut(ordinal as usize)
            .filter(|attempt| attempt.id.ordinal == ordinal)
            .ok_or(AttemptBlock::UnknownAttempt { ordinal })
    }

    fn prior_attempt_succeeded(&self) -> bool {
        if self.attempts.len() < 2 {
            return false;
        }

        self.attempts[..self.attempts.len() - 1]
            .iter()
            .any(|attempt| {
                matches!(
                    attempt.status(),
                    AttemptStatus::EffectConfirmed(ExecutionOutcome::Succeeded)
                        | AttemptStatus::Reconciled(ReconciliationOutcome::EffectSucceeded)
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use lifetra_bead::BeadId;

    use crate::{ExecutionMode, RetryReason};

    use super::*;

    fn ticket() -> AuthorityTicket {
        AuthorityTicket {
            action_id: ActionId::new("action:ledger:1").expect("valid action id"),
            source_bead: BeadId::new("bead:ledger:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:ledger".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn binding(ticket: &AuthorityTicket) -> IdempotencyBinding {
        IdempotencyBinding::new(ticket, "idem:ledger:1").expect("valid idempotency key")
    }

    fn initial_decision(ticket: &AuthorityTicket, binding: &IdempotencyBinding) -> RetryDecision {
        RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: binding.key.clone(),
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: ticket.authority_proof_refs.clone(),
        }
    }

    fn redispatch_decision(
        ticket: &AuthorityTicket,
        binding: &IdempotencyBinding,
        ordinal: u32,
        proof_ref: &str,
    ) -> RetryDecision {
        RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: binding.key.clone(),
            verdict: RetryVerdict::RedispatchAllowed { ordinal },
            reasons: vec![RetryReason::ReconciliationConfirmedNoEffect],
            proof_refs: vec![proof_ref.into()],
        }
    }

    fn ledger() -> AttemptLedger {
        let ticket = ticket();
        let binding = binding(&ticket);
        AttemptLedger::new(ticket, binding).expect("valid ledger")
    }

    #[test]
    fn initial_dispatch_becomes_attempt_zero() {
        let mut ledger = ledger();
        let expected_action = ledger.ticket.action_id.clone();
        let decision = initial_decision(&ledger.ticket, &ledger.binding);

        let attempt_id = ledger
            .record_dispatch(&decision, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch should record")
            .id
            .clone();

        assert_eq!(attempt_id.ordinal, 0);
        assert_eq!(attempt_id.action_id, expected_action);
        assert_eq!(ledger.retry_context().redispatches_used, 0);
    }

    #[test]
    fn unknown_attempt_cannot_be_redispatched_even_with_forged_allow() {
        let mut ledger = ledger();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        let forged = redispatch_decision(
            &ledger.ticket,
            &ledger.binding,
            1,
            "proof:made-up-resolution",
        );

        assert_eq!(
            ledger.record_dispatch(&forged, Timestamp::new(12), "proof:dispatch:1"),
            Err(AttemptBlock::PreviousAttemptNotRetrySafe)
        );
    }

    #[test]
    fn no_effect_resolution_allows_next_physical_attempt_with_same_identity() {
        let mut ledger = ledger();
        let expected_action = ledger.ticket.action_id.clone();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:0",
            )
            .expect("reconciliation");
        let retry = redispatch_decision(&ledger.ticket, &ledger.binding, 1, "proof:no-effect:0");

        let second_id = ledger
            .record_dispatch(&retry, Timestamp::new(13), "proof:dispatch:1")
            .expect("redispatch should record")
            .id
            .clone();

        assert_eq!(second_id.ordinal, 1);
        assert_eq!(second_id.action_id, expected_action);
        assert_eq!(ledger.binding.key, "idem:ledger:1");
        assert_eq!(ledger.retry_context().redispatches_used, 1);
    }

    #[test]
    fn retry_decision_must_carry_previous_resolution_proof() {
        let mut ledger = ledger();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:0",
            )
            .expect("reconciliation");
        let retry =
            redispatch_decision(&ledger.ticket, &ledger.binding, 1, "proof:wrong-resolution");

        assert_eq!(
            ledger.record_dispatch(&retry, Timestamp::new(13), "proof:dispatch:1"),
            Err(AttemptBlock::RetryProofLineageMissing)
        );
    }

    #[test]
    fn retry_inputs_use_latest_attempt_and_derive_retry_count() {
        let mut ledger = ledger();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:0",
            )
            .expect("reconciliation");
        let retry = redispatch_decision(&ledger.ticket, &ledger.binding, 1, "proof:no-effect:0");
        ledger
            .record_dispatch(&retry, Timestamp::new(13), "proof:dispatch:1")
            .expect("redispatch");
        ledger
            .record_reconciliation(
                1,
                Timestamp::new(14),
                ReconciliationOutcome::StillUnknown,
                "proof:still-unknown:1",
            )
            .expect("reconciliation");

        let (trace, reconciliation, context) = ledger.retry_inputs().expect("safe retry view");

        assert_eq!(
            trace.dispatch.expect("dispatch").proof_ref,
            "proof:dispatch:1"
        );
        assert_eq!(
            reconciliation.expect("reconciliation").outcome,
            ReconciliationOutcome::StillUnknown
        );
        assert_eq!(context.redispatches_used, 1);
    }

    #[test]
    fn late_success_on_prior_attempt_blocks_future_retry_view() {
        let mut ledger = ledger();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:0",
            )
            .expect("reconciliation");
        let retry = redispatch_decision(&ledger.ticket, &ledger.binding, 1, "proof:no-effect:0");
        ledger
            .record_dispatch(&retry, Timestamp::new(13), "proof:dispatch:1")
            .expect("redispatch");

        ledger
            .record_external_outcome(
                0,
                Timestamp::new(14),
                ExecutionOutcome::Succeeded,
                "proof:late-success:0",
            )
            .expect("late external evidence should be preserved");

        assert_eq!(
            ledger.retry_inputs(),
            Err(AttemptBlock::ConflictingEvidence)
        );
    }

    #[test]
    fn contradictory_reconciliation_and_external_evidence_is_visible() {
        let mut ledger = ledger();
        let initial = initial_decision(&ledger.ticket, &ledger.binding);
        ledger
            .record_dispatch(&initial, Timestamp::new(11), "proof:dispatch:0")
            .expect("initial dispatch");
        ledger
            .record_reconciliation(
                0,
                Timestamp::new(12),
                ReconciliationOutcome::NoEffectConfirmed,
                "proof:no-effect:0",
            )
            .expect("reconciliation");
        ledger
            .record_external_outcome(
                0,
                Timestamp::new(13),
                ExecutionOutcome::Succeeded,
                "proof:success:0",
            )
            .expect("external evidence should be recorded");

        assert!(ledger.has_conflicting_evidence());
        assert_eq!(
            ledger.retry_inputs(),
            Err(AttemptBlock::ConflictingEvidence)
        );
    }
}
