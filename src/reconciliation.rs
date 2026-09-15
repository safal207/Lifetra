use lifetra_core::Timestamp;

use crate::{ActionId, AuthorityTicket, ExecutionOutcome, ExecutionStatus, ExecutionTrace};

/// Provider-facing idempotency key permanently bound to one logical action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyBinding {
    pub action_id: ActionId,
    pub key: String,
}

impl IdempotencyBinding {
    pub fn new(ticket: &AuthorityTicket, key: impl Into<String>) -> Result<Self, RetryBlock> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(RetryBlock::EmptyIdempotencyKey);
        }

        Ok(Self {
            action_id: ticket.action_id.clone(),
            key,
        })
    }
}

/// Externally reconciled state for an already-dispatched action.
///
/// `NoEffectConfirmed` is deliberately stronger than a generic "not found".
/// It should only be used when provider semantics and external proof establish
/// that the side effect did not occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationOutcome {
    EffectSucceeded,
    EffectFailed,
    NoEffectConfirmed,
    StillUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationReceipt {
    pub action_id: ActionId,
    pub observed_at: Timestamp,
    pub outcome: ReconciliationOutcome,
    pub proof_ref: String,
}

impl ReconciliationReceipt {
    pub fn new(
        action_id: ActionId,
        observed_at: Timestamp,
        outcome: ReconciliationOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<Self, RetryBlock> {
        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(RetryBlock::EmptyReconciliationProof);
        }

        Ok(Self {
            action_id,
            observed_at,
            outcome,
            proof_ref,
        })
    }
}

/// Explicit retry policy. A confirmed failure is not automatically retryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_redispatches: u32,
    pub allow_after_confirmed_failure: bool,
    pub allow_after_confirmed_no_effect: bool,
}

impl RetryPolicy {
    pub fn new(
        max_redispatches: u32,
        allow_after_confirmed_failure: bool,
        allow_after_confirmed_no_effect: bool,
    ) -> Self {
        Self {
            max_redispatches,
            allow_after_confirmed_failure,
            allow_after_confirmed_no_effect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetryContext {
    pub redispatches_used: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryVerdict {
    InitialDispatchAllowed,
    ReconcileFirst,
    RedispatchAllowed { ordinal: u32 },
    CloseSucceeded,
    CloseFailed,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryReason {
    AuthorizedNotDispatched,
    EffectUnknown,
    ReconciliationRequired,
    ReconciliationStillUnknown,
    ReconciliationConfirmedSuccess,
    ReconciliationConfirmedFailure,
    ReconciliationConfirmedNoEffect,
    ExternalConfirmedSuccess,
    ExternalConfirmedFailure,
    RetryDisabled,
    RetryLimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryBlock {
    EmptyIdempotencyKey,
    EmptyReconciliationProof,
    IdempotencyActionMismatch,
    ReconciliationActionMismatch,
    ReconciliationBeforeDispatch,
}

/// Auditable retry/reconciliation result for one logical action identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryDecision {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub verdict: RetryVerdict,
    pub reasons: Vec<RetryReason>,
    pub proof_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAuthority {
    pub policy: RetryPolicy,
}

impl RetryAuthority {
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy }
    }

    /// Decides whether the same logical action may be dispatched again.
    ///
    /// A dispatched action with unknown effect is never blindly retried. It
    /// must first receive a reconciliation receipt. `StillUnknown` keeps the
    /// action in reconcile-first state.
    pub fn evaluate(
        &self,
        trace: &ExecutionTrace,
        binding: &IdempotencyBinding,
        reconciliation: Option<&ReconciliationReceipt>,
        context: RetryContext,
    ) -> Result<RetryDecision, RetryBlock> {
        if binding.action_id != trace.ticket.action_id {
            return Err(RetryBlock::IdempotencyActionMismatch);
        }

        let base = |verdict, reasons, proof_refs| RetryDecision {
            action_id: trace.ticket.action_id.clone(),
            idempotency_key: binding.key.clone(),
            verdict,
            reasons,
            proof_refs,
        };

        match trace.status() {
            ExecutionStatus::AuthorizedNotDispatched => Ok(base(
                RetryVerdict::InitialDispatchAllowed,
                vec![RetryReason::AuthorizedNotDispatched],
                trace.ticket.authority_proof_refs.clone(),
            )),
            ExecutionStatus::EffectConfirmed(ExecutionOutcome::Succeeded) => {
                let external = trace.external.as_ref().expect("confirmed status has receipt");
                Ok(base(
                    RetryVerdict::CloseSucceeded,
                    vec![RetryReason::ExternalConfirmedSuccess],
                    vec![external.proof_ref.clone()],
                ))
            }
            ExecutionStatus::EffectConfirmed(ExecutionOutcome::Failed) => {
                let external = trace.external.as_ref().expect("confirmed status has receipt");
                self.retry_or_close(
                    trace,
                    binding,
                    context,
                    self.policy.allow_after_confirmed_failure,
                    RetryReason::ExternalConfirmedFailure,
                    vec![external.proof_ref.clone()],
                )
            }
            ExecutionStatus::DispatchedEffectUnknown => {
                let dispatch = trace.dispatch.as_ref().expect("dispatched status has receipt");
                let Some(reconciliation) = reconciliation else {
                    return Ok(base(
                        RetryVerdict::ReconcileFirst,
                        vec![RetryReason::EffectUnknown, RetryReason::ReconciliationRequired],
                        vec![dispatch.proof_ref.clone()],
                    ));
                };

                if reconciliation.action_id != trace.ticket.action_id {
                    return Err(RetryBlock::ReconciliationActionMismatch);
                }
                if reconciliation.observed_at < dispatch.dispatched_at {
                    return Err(RetryBlock::ReconciliationBeforeDispatch);
                }

                let proofs = vec![dispatch.proof_ref.clone(), reconciliation.proof_ref.clone()];
                match reconciliation.outcome {
                    ReconciliationOutcome::StillUnknown => Ok(base(
                        RetryVerdict::ReconcileFirst,
                        vec![RetryReason::ReconciliationStillUnknown],
                        proofs,
                    )),
                    ReconciliationOutcome::EffectSucceeded => Ok(base(
                        RetryVerdict::CloseSucceeded,
                        vec![RetryReason::ReconciliationConfirmedSuccess],
                        proofs,
                    )),
                    ReconciliationOutcome::EffectFailed => self.retry_or_close(
                        trace,
                        binding,
                        context,
                        self.policy.allow_after_confirmed_failure,
                        RetryReason::ReconciliationConfirmedFailure,
                        proofs,
                    ),
                    ReconciliationOutcome::NoEffectConfirmed => self.retry_or_close(
                        trace,
                        binding,
                        context,
                        self.policy.allow_after_confirmed_no_effect,
                        RetryReason::ReconciliationConfirmedNoEffect,
                        proofs,
                    ),
                }
            }
        }
    }

    fn retry_or_close(
        &self,
        trace: &ExecutionTrace,
        binding: &IdempotencyBinding,
        context: RetryContext,
        retry_enabled: bool,
        evidence_reason: RetryReason,
        proof_refs: Vec<String>,
    ) -> Result<RetryDecision, RetryBlock> {
        let base = |verdict, reasons| RetryDecision {
            action_id: trace.ticket.action_id.clone(),
            idempotency_key: binding.key.clone(),
            verdict,
            reasons,
            proof_refs: proof_refs.clone(),
        };

        if !retry_enabled {
            return Ok(base(
                RetryVerdict::CloseFailed,
                vec![evidence_reason, RetryReason::RetryDisabled],
            ));
        }

        if context.redispatches_used >= self.policy.max_redispatches {
            return Ok(base(
                RetryVerdict::Block,
                vec![evidence_reason, RetryReason::RetryLimitReached],
            ));
        }

        Ok(base(
            RetryVerdict::RedispatchAllowed {
                ordinal: context.redispatches_used + 1,
            },
            vec![evidence_reason],
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AuthorityTicket, BeadId, ExecutionMode, ExecutionTrace, ExternalExecutionReceipt,
        DispatchReceipt,
    };

    use super::*;

    fn ticket() -> AuthorityTicket {
        AuthorityTicket {
            action_id: ActionId::new("action:retry:1").expect("valid id"),
            source_bead: BeadId::new("bead:retry:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:1".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn binding(ticket: &AuthorityTicket) -> IdempotencyBinding {
        IdempotencyBinding::new(ticket, "idem:action:retry:1").expect("valid key")
    }

    fn dispatched_trace() -> ExecutionTrace {
        ExecutionTrace {
            ticket: ticket(),
            dispatch: Some(DispatchReceipt {
                action_id: ActionId::new("action:retry:1").expect("valid id"),
                dispatched_at: Timestamp::new(11),
                proof_ref: "proof:dispatch:1".into(),
            }),
            external: None,
        }
    }

    #[test]
    fn initial_dispatch_is_allowed_without_reconciliation() {
        let trace = ExecutionTrace::new(ticket());
        let binding = binding(&trace.ticket);
        let authority = RetryAuthority::new(RetryPolicy::new(1, true, true));

        let decision = authority
            .evaluate(&trace, &binding, None, RetryContext::default())
            .expect("valid evaluation");

        assert_eq!(decision.verdict, RetryVerdict::InitialDispatchAllowed);
    }

    #[test]
    fn unknown_after_dispatch_requires_reconciliation_before_retry() {
        let trace = dispatched_trace();
        let binding = binding(&trace.ticket);
        let authority = RetryAuthority::new(RetryPolicy::new(1, true, true));

        let decision = authority
            .evaluate(&trace, &binding, None, RetryContext::default())
            .expect("valid evaluation");

        assert_eq!(decision.verdict, RetryVerdict::ReconcileFirst);
        assert!(decision.reasons.contains(&RetryReason::ReconciliationRequired));
    }

    #[test]
    fn still_unknown_reconciliation_never_authorizes_redispatch() {
        let trace = dispatched_trace();
        let binding = binding(&trace.ticket);
        let receipt = ReconciliationReceipt::new(
            trace.ticket.action_id.clone(),
            Timestamp::new(12),
            ReconciliationOutcome::StillUnknown,
            "proof:reconcile:unknown",
        )
        .expect("valid receipt");
        let authority = RetryAuthority::new(RetryPolicy::new(3, true, true));

        let decision = authority
            .evaluate(&trace, &binding, Some(&receipt), RetryContext::default())
            .expect("valid evaluation");

        assert_eq!(decision.verdict, RetryVerdict::ReconcileFirst);
    }

    #[test]
    fn confirmed_no_effect_can_authorize_same_action_redispatch() {
        let trace = dispatched_trace();
        let binding = binding(&trace.ticket);
        let receipt = ReconciliationReceipt::new(
            trace.ticket.action_id.clone(),
            Timestamp::new(12),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:reconcile:no-effect",
        )
        .expect("valid receipt");
        let authority = RetryAuthority::new(RetryPolicy::new(2, false, true));

        let decision = authority
            .evaluate(&trace, &binding, Some(&receipt), RetryContext::default())
            .expect("valid evaluation");

        assert_eq!(
            decision.verdict,
            RetryVerdict::RedispatchAllowed { ordinal: 1 }
        );
        assert_eq!(decision.action_id, trace.ticket.action_id);
        assert_eq!(decision.idempotency_key, "idem:action:retry:1");
    }

    #[test]
    fn reconciliation_success_closes_action_instead_of_retrying() {
        let trace = dispatched_trace();
        let binding = binding(&trace.ticket);
        let receipt = ReconciliationReceipt::new(
            trace.ticket.action_id.clone(),
            Timestamp::new(12),
            ReconciliationOutcome::EffectSucceeded,
            "proof:reconcile:success",
        )
        .expect("valid receipt");
        let authority = RetryAuthority::new(RetryPolicy::new(4, true, true));

        let decision = authority
            .evaluate(&trace, &binding, Some(&receipt), RetryContext::default())
            .expect("valid evaluation");

        assert_eq!(decision.verdict, RetryVerdict::CloseSucceeded);
    }

    #[test]
    fn confirmed_failure_is_retryable_only_when_policy_allows() {
        let mut trace = dispatched_trace();
        trace.external = Some(ExternalExecutionReceipt {
            action_id: trace.ticket.action_id.clone(),
            observed_at: Timestamp::new(12),
            outcome: ExecutionOutcome::Failed,
            proof_ref: "proof:external:failed".into(),
        });
        let binding = binding(&trace.ticket);

        let closed = RetryAuthority::new(RetryPolicy::new(2, false, true))
            .evaluate(&trace, &binding, None, RetryContext::default())
            .expect("valid evaluation");
        assert_eq!(closed.verdict, RetryVerdict::CloseFailed);

        let retry = RetryAuthority::new(RetryPolicy::new(2, true, true))
            .evaluate(&trace, &binding, None, RetryContext::default())
            .expect("valid evaluation");
        assert_eq!(retry.verdict, RetryVerdict::RedispatchAllowed { ordinal: 1 });
    }

    #[test]
    fn retry_limit_blocks_further_redispatch() {
        let trace = dispatched_trace();
        let binding = binding(&trace.ticket);
        let receipt = ReconciliationReceipt::new(
            trace.ticket.action_id.clone(),
            Timestamp::new(12),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:reconcile:no-effect",
        )
        .expect("valid receipt");
        let authority = RetryAuthority::new(RetryPolicy::new(1, true, true));

        let decision = authority
            .evaluate(
                &trace,
                &binding,
                Some(&receipt),
                RetryContext {
                    redispatches_used: 1,
                },
            )
            .expect("valid evaluation");

        assert_eq!(decision.verdict, RetryVerdict::Block);
        assert!(decision.reasons.contains(&RetryReason::RetryLimitReached));
    }

    #[test]
    fn idempotency_binding_cannot_be_reused_for_another_action() {
        let trace = dispatched_trace();
        let other_ticket = AuthorityTicket {
            action_id: ActionId::new("action:other").expect("valid id"),
            ..ticket()
        };
        let wrong_binding = binding(&other_ticket);
        let authority = RetryAuthority::new(RetryPolicy::new(1, true, true));

        assert_eq!(
            authority.evaluate(&trace, &wrong_binding, None, RetryContext::default()),
            Err(RetryBlock::IdempotencyActionMismatch)
        );
    }
}
