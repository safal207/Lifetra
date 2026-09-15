use crate::{
    ActionId, AttemptBlock, DurableJournal, JournalError, ReconciliationOutcome, RecoveryDirective,
    RetryAuthority, RetryBlock, RetryDecision, RetryReason, RetryVerdict, Timestamp,
};

/// Which uncertainty boundary the provider is being asked to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderReconciliationPhase {
    /// A write-ahead `PreparedAttempt` survived restart without a local dispatch receipt.
    PreparedAmbiguous,
    /// A local dispatch receipt exists but the external effect remains unknown.
    DispatchedUnknown,
}

/// Provider-neutral lookup request. Adapters may translate the stable action and
/// idempotency identities into provider-specific APIs, operation IDs, or ledgers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReconciliationQuery {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub attempt_ordinal: u32,
    pub phase: ProviderReconciliationPhase,
}

/// External observation returned by a provider adapter.
///
/// `proof_ref` must identify inspectable evidence for the observation. A plain
/// timeout, 404, or silence must be represented as `StillUnknown`, not as
/// `NoEffectConfirmed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderObservation {
    pub observed_at: Timestamp,
    pub outcome: ReconciliationOutcome,
    pub proof_ref: String,
}

/// Provider-specific lookup boundary. The core runtime does not assume HTTP,
/// databases, blockchains, payment rails, or any particular transport.
pub trait ProviderReconciliationAdapter {
    type Error;

    fn reconcile(
        &self,
        query: &ProviderReconciliationQuery,
    ) -> Result<ProviderObservation, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderReconciliationResult {
    pub query: ProviderReconciliationQuery,
    pub observation: ProviderObservation,
    pub decision: RetryDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderReconciliationError<E> {
    NotReconciliationState,
    EmptyProviderProof,
    Adapter(E),
    Journal(JournalError),
    Attempt(AttemptBlock),
    Retry(RetryBlock),
}

impl<E> From<JournalError> for ProviderReconciliationError<E> {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

impl<E> From<AttemptBlock> for ProviderReconciliationError<E> {
    fn from(value: AttemptBlock) -> Self {
        Self::Attempt(value)
    }
}

impl<E> From<RetryBlock> for ProviderReconciliationError<E> {
    fn from(value: RetryBlock) -> Self {
        Self::Retry(value)
    }
}

/// One-shot bridge from a durable recovery directive to external proof and an
/// auditable retry/close verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderReconciler {
    pub retry_authority: RetryAuthority,
}

impl ProviderReconciler {
    pub fn new(retry_authority: RetryAuthority) -> Self {
        Self { retry_authority }
    }

    pub fn reconcile_once<A: ProviderReconciliationAdapter>(
        &self,
        journal: &mut DurableJournal,
        adapter: &A,
    ) -> Result<ProviderReconciliationResult, ProviderReconciliationError<A::Error>> {
        let before = journal.recover()?;
        let (attempt_ordinal, phase) = match before.directive {
            RecoveryDirective::ReconcilePreparedAttempt { ordinal } => {
                (ordinal, ProviderReconciliationPhase::PreparedAmbiguous)
            }
            RecoveryDirective::ReconcileDispatchedAttempt { ordinal } => {
                (ordinal, ProviderReconciliationPhase::DispatchedUnknown)
            }
            RecoveryDirective::ReadyForInitialPreparation
            | RecoveryDirective::EvaluateRetry { .. }
            | RecoveryDirective::CloseSucceeded { .. }
            | RecoveryDirective::Block => {
                return Err(ProviderReconciliationError::NotReconciliationState)
            }
        };

        let query = ProviderReconciliationQuery {
            action_id: journal.ticket().action_id.clone(),
            idempotency_key: journal.binding().key.clone(),
            attempt_ordinal,
            phase,
        };
        let observation = adapter
            .reconcile(&query)
            .map_err(ProviderReconciliationError::Adapter)?;
        if observation.proof_ref.trim().is_empty() {
            return Err(ProviderReconciliationError::EmptyProviderProof);
        }

        journal.record_reconciliation(
            attempt_ordinal,
            observation.observed_at,
            observation.outcome,
            observation.proof_ref.clone(),
        )?;
        let after = journal.recover()?;

        let decision = if after.directive == RecoveryDirective::Block {
            RetryDecision {
                action_id: query.action_id.clone(),
                idempotency_key: query.idempotency_key.clone(),
                verdict: RetryVerdict::Block,
                reasons: Vec::new(),
                proof_refs: vec![observation.proof_ref.clone()],
            }
        } else {
            match phase {
                ProviderReconciliationPhase::PreparedAmbiguous => self.prepared_decision(
                    &query,
                    &observation,
                    after.retry_context(),
                ),
                ProviderReconciliationPhase::DispatchedUnknown => {
                    let (trace, reconciliation, _) = after.ledger.retry_inputs()?;
                    self.retry_authority.evaluate(
                        &trace,
                        journal.binding(),
                        reconciliation.as_ref(),
                        after.retry_context(),
                    )?
                }
            }
        };

        Ok(ProviderReconciliationResult {
            query,
            observation,
            decision,
        })
    }

    fn prepared_decision(
        &self,
        query: &ProviderReconciliationQuery,
        observation: &ProviderObservation,
        context: crate::RetryContext,
    ) -> RetryDecision {
        let base = |verdict, reasons| RetryDecision {
            action_id: query.action_id.clone(),
            idempotency_key: query.idempotency_key.clone(),
            verdict,
            reasons,
            proof_refs: vec![observation.proof_ref.clone()],
        };

        match observation.outcome {
            ReconciliationOutcome::StillUnknown => base(
                RetryVerdict::ReconcileFirst,
                vec![RetryReason::ReconciliationStillUnknown],
            ),
            ReconciliationOutcome::EffectSucceeded => base(
                RetryVerdict::CloseSucceeded,
                vec![RetryReason::ReconciliationConfirmedSuccess],
            ),
            ReconciliationOutcome::EffectFailed => self.prepared_retry_or_close(
                context,
                self.retry_authority.policy.allow_after_confirmed_failure,
                RetryReason::ReconciliationConfirmedFailure,
                base,
            ),
            ReconciliationOutcome::NoEffectConfirmed => self.prepared_retry_or_close(
                context,
                self.retry_authority.policy.allow_after_confirmed_no_effect,
                RetryReason::ReconciliationConfirmedNoEffect,
                base,
            ),
        }
    }

    fn prepared_retry_or_close<F>(
        &self,
        context: crate::RetryContext,
        retry_enabled: bool,
        evidence_reason: RetryReason,
        base: F,
    ) -> RetryDecision
    where
        F: Fn(RetryVerdict, Vec<RetryReason>) -> RetryDecision,
    {
        if !retry_enabled {
            return base(
                RetryVerdict::CloseFailed,
                vec![evidence_reason, RetryReason::RetryDisabled],
            );
        }
        if context.redispatches_used >= self.retry_authority.policy.max_redispatches {
            return base(
                RetryVerdict::Block,
                vec![evidence_reason, RetryReason::RetryLimitReached],
            );
        }

        base(
            RetryVerdict::RedispatchAllowed {
                ordinal: context.redispatches_used + 1,
            },
            vec![evidence_reason],
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::{AuthorityTicket, BeadId, ExecutionMode, IdempotencyBinding, RetryPolicy};

    use super::*;

    #[derive(Debug)]
    struct MockAdapter {
        result: Result<ProviderObservation, &'static str>,
        queries: RefCell<Vec<ProviderReconciliationQuery>>,
    }

    impl ProviderReconciliationAdapter for MockAdapter {
        type Error = &'static str;

        fn reconcile(
            &self,
            query: &ProviderReconciliationQuery,
        ) -> Result<ProviderObservation, Self::Error> {
            self.queries.borrow_mut().push(query.clone());
            self.result.clone()
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lifetra-provider-reconcile-{name}-{}-{nonce}.journal",
            std::process::id()
        ))
    }

    fn ticket() -> AuthorityTicket {
        AuthorityTicket {
            action_id: ActionId::new("action:provider:1").expect("valid action id"),
            source_bead: BeadId::new("bead:provider:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:provider".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn create_prepared_crash(path: &PathBuf) {
        let ticket = ticket();
        let binding = IdempotencyBinding::new(&ticket, "idem:provider:1").expect("valid binding");
        let mut journal = DurableJournal::create(path, ticket.clone(), binding.clone())
            .expect("journal should create");
        let initial = RetryDecision {
            action_id: ticket.action_id,
            idempotency_key: binding.key,
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: vec!["proof:authority:provider".into()],
        };
        journal
            .prepare_attempt(&initial, Timestamp::new(11))
            .expect("prepare should persist");
    }

    fn reconciler() -> ProviderReconciler {
        ProviderReconciler::new(RetryAuthority::new(RetryPolicy::new(1, false, true)))
    }

    #[test]
    fn prepared_no_effect_can_advance_to_real_attempt_one_without_fake_zero() {
        let path = temp_path("prepared-no-effect");
        create_prepared_crash(&path);
        let mut journal = DurableJournal::open(&path).expect("journal should reopen");
        let adapter = MockAdapter {
            result: Ok(ProviderObservation {
                observed_at: Timestamp::new(12),
                outcome: ReconciliationOutcome::NoEffectConfirmed,
                proof_ref: "proof:provider:no-effect:0".into(),
            }),
            queries: RefCell::new(Vec::new()),
        };

        let result = reconciler()
            .reconcile_once(&mut journal, &adapter)
            .expect("provider reconciliation should succeed");
        assert_eq!(
            result.decision.verdict,
            RetryVerdict::RedispatchAllowed { ordinal: 1 }
        );
        assert_eq!(
            result.query.phase,
            ProviderReconciliationPhase::PreparedAmbiguous
        );
        assert_eq!(result.query.idempotency_key, "idem:provider:1");

        journal
            .prepare_attempt(&result.decision, Timestamp::new(13))
            .expect("provider proof should authorize next prepare");
        journal
            .record_dispatch(1, Timestamp::new(14), "proof:dispatch:1")
            .expect("attempt one should dispatch");

        let recovered = journal.recover().expect("journal should replay");
        assert_eq!(recovered.next_attempt_ordinal, 2);
        assert_eq!(recovered.ledger.attempts().len(), 1);
        assert_eq!(recovered.ledger.attempts()[0].id.ordinal, 1);
        assert!(recovered
            .ledger
            .attempts()
            .iter()
            .all(|attempt| attempt.id.ordinal != 0));
        fs::remove_file(path).ok();
    }

    #[test]
    fn prepared_success_closes_without_fabricating_dispatch() {
        let path = temp_path("prepared-success");
        create_prepared_crash(&path);
        let mut journal = DurableJournal::open(&path).expect("journal should reopen");
        let adapter = MockAdapter {
            result: Ok(ProviderObservation {
                observed_at: Timestamp::new(12),
                outcome: ReconciliationOutcome::EffectSucceeded,
                proof_ref: "proof:provider:success:0".into(),
            }),
            queries: RefCell::new(Vec::new()),
        };

        let result = reconciler()
            .reconcile_once(&mut journal, &adapter)
            .expect("provider reconciliation should succeed");
        assert_eq!(result.decision.verdict, RetryVerdict::CloseSucceeded);
        let recovered = journal.recover().expect("journal should replay");
        assert_eq!(
            recovered.directive,
            RecoveryDirective::CloseSucceeded { ordinal: 0 }
        );
        assert!(recovered.ledger.attempts().is_empty());
        assert_eq!(recovered.prepared_reconciliations.len(), 1);
        fs::remove_file(path).ok();
    }

    #[test]
    fn prepared_still_unknown_remains_reconcile_first() {
        let path = temp_path("prepared-unknown");
        create_prepared_crash(&path);
        let mut journal = DurableJournal::open(&path).expect("journal should reopen");
        let adapter = MockAdapter {
            result: Ok(ProviderObservation {
                observed_at: Timestamp::new(12),
                outcome: ReconciliationOutcome::StillUnknown,
                proof_ref: "proof:provider:unknown:0".into(),
            }),
            queries: RefCell::new(Vec::new()),
        };

        let result = reconciler()
            .reconcile_once(&mut journal, &adapter)
            .expect("provider reconciliation should succeed");
        assert_eq!(result.decision.verdict, RetryVerdict::ReconcileFirst);
        assert_eq!(
            journal.recover().expect("journal should replay").directive,
            RecoveryDirective::ReconcilePreparedAttempt { ordinal: 0 }
        );
        fs::remove_file(path).ok();
    }

    #[test]
    fn dispatched_unknown_uses_existing_retry_authority() {
        let path = temp_path("dispatched-no-effect");
        let ticket = ticket();
        let binding = IdempotencyBinding::new(&ticket, "idem:provider:1").expect("valid binding");
        let mut journal = DurableJournal::create(&path, ticket.clone(), binding.clone())
            .expect("journal should create");
        let initial = RetryDecision {
            action_id: ticket.action_id,
            idempotency_key: binding.key,
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: vec!["proof:authority:provider".into()],
        };
        journal
            .prepare_attempt(&initial, Timestamp::new(11))
            .expect("prepare should persist");
        journal
            .record_dispatch(0, Timestamp::new(12), "proof:dispatch:0")
            .expect("dispatch should persist");
        drop(journal);

        let mut journal = DurableJournal::open(&path).expect("journal should reopen");
        let adapter = MockAdapter {
            result: Ok(ProviderObservation {
                observed_at: Timestamp::new(13),
                outcome: ReconciliationOutcome::NoEffectConfirmed,
                proof_ref: "proof:provider:no-effect:0".into(),
            }),
            queries: RefCell::new(Vec::new()),
        };
        let result = reconciler()
            .reconcile_once(&mut journal, &adapter)
            .expect("provider reconciliation should succeed");

        assert_eq!(
            result.query.phase,
            ProviderReconciliationPhase::DispatchedUnknown
        );
        assert_eq!(
            result.decision.verdict,
            RetryVerdict::RedispatchAllowed { ordinal: 1 }
        );
        assert!(result
            .decision
            .proof_refs
            .contains(&"proof:dispatch:0".to_owned()));
        assert!(result
            .decision
            .proof_refs
            .contains(&"proof:provider:no-effect:0".to_owned()));
        fs::remove_file(path).ok();
    }

    #[test]
    fn adapter_error_does_not_mutate_durable_history() {
        let path = temp_path("adapter-error");
        create_prepared_crash(&path);
        let mut journal = DurableJournal::open(&path).expect("journal should reopen");
        let before = journal.recover().expect("journal should replay").last_sequence;
        let adapter = MockAdapter {
            result: Err("provider unavailable"),
            queries: RefCell::new(Vec::new()),
        };

        let result = reconciler().reconcile_once(&mut journal, &adapter);
        assert_eq!(
            result,
            Err(ProviderReconciliationError::Adapter("provider unavailable"))
        );
        assert_eq!(
            journal.recover().expect("journal should replay").last_sequence,
            before
        );
        fs::remove_file(path).ok();
    }
}
