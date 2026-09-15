use crate::{
    AttemptStatus, ExecutionOutcome, ReconciliationOutcome, RetryDecision, RetryReason, RetryVerdict,
};

use super::*;

pub(super) fn binding_from_events(
    events: &[JournalEvent],
) -> Result<(AuthorityTicket, IdempotencyBinding), JournalError> {
    let first = events.first().ok_or(JournalError::MissingBinding)?;
    let (ticket, binding) = match first {
        JournalEvent::Binding { ticket, binding } => (ticket.clone(), binding.clone()),
        _ => return Err(JournalError::BindingMustBeFirst),
    };
    if events
        .iter()
        .skip(1)
        .any(|event| matches!(event, JournalEvent::Binding { .. }))
    {
        return Err(JournalError::DuplicateBinding);
    }
    Ok((ticket, binding))
}

pub(super) fn replay_events(events: &[JournalEvent]) -> Result<RecoveredRuntime, JournalError> {
    let (ticket, binding) = binding_from_events(events)?;
    let mut ledger = AttemptLedger::new(ticket.clone(), binding.clone())?;
    let mut pending: Option<PreparedAttempt> = None;

    for event in events.iter().skip(1) {
        match event {
            JournalEvent::Binding { .. } => return Err(JournalError::DuplicateBinding),
            JournalEvent::Prepared(prepared) => {
                if let Some(existing) = &pending {
                    return Err(JournalError::PendingPreparedAttempt {
                        ordinal: existing.id.ordinal,
                    });
                }
                let expected = ledger.attempts().len() as u32;
                if prepared.id.ordinal != expected {
                    return Err(JournalError::UnexpectedPreparationOrdinal {
                        expected,
                        received: prepared.id.ordinal,
                    });
                }
                if prepared.id.action_id != ticket.action_id {
                    return Err(JournalError::BindingActionMismatch);
                }
                pending = Some(prepared.clone());
            }
            JournalEvent::Dispatch {
                ordinal,
                dispatched_at,
                proof_ref,
            } => {
                let prepared = pending
                    .take()
                    .ok_or(JournalError::NoPendingPreparedAttempt)?;
                if prepared.id.ordinal != *ordinal {
                    return Err(JournalError::PreparedOrdinalMismatch {
                        expected: prepared.id.ordinal,
                        received: *ordinal,
                    });
                }
                if *dispatched_at < prepared.prepared_at {
                    return Err(JournalError::DispatchBeforePreparation);
                }

                let verdict = if *ordinal == 0 {
                    RetryVerdict::InitialDispatchAllowed
                } else {
                    RetryVerdict::RedispatchAllowed { ordinal: *ordinal }
                };
                let decision = RetryDecision {
                    action_id: ticket.action_id.clone(),
                    idempotency_key: binding.key.clone(),
                    verdict,
                    reasons: Vec::<RetryReason>::new(),
                    proof_refs: prepared.authorization_proof_refs,
                };
                ledger.record_dispatch(&decision, *dispatched_at, proof_ref.clone())?;
            }
            JournalEvent::Reconciliation {
                ordinal,
                observed_at,
                outcome,
                proof_ref,
            } => {
                if let Some(prepared) = &pending {
                    return Err(JournalError::PendingPreparedAttempt {
                        ordinal: prepared.id.ordinal,
                    });
                }
                ledger.record_reconciliation(*ordinal, *observed_at, *outcome, proof_ref.clone())?;
            }
            JournalEvent::External {
                ordinal,
                observed_at,
                outcome,
                proof_ref,
            } => {
                if let Some(prepared) = &pending {
                    return Err(JournalError::PendingPreparedAttempt {
                        ordinal: prepared.id.ordinal,
                    });
                }
                ledger.record_external_outcome(*ordinal, *observed_at, *outcome, proof_ref.clone())?;
            }
        }
    }

    let directive = recovery_directive(&ledger, pending.as_ref());
    Ok(RecoveredRuntime {
        ledger,
        pending_prepared: pending,
        directive,
        repaired_truncated_tail: false,
        last_sequence: 0,
    })
}

fn recovery_directive(
    ledger: &AttemptLedger,
    pending: Option<&PreparedAttempt>,
) -> RecoveryDirective {
    if let Some(prepared) = pending {
        return RecoveryDirective::ReconcilePreparedAttempt {
            ordinal: prepared.id.ordinal,
        };
    }
    if ledger.has_conflicting_evidence() {
        return RecoveryDirective::Block;
    }

    let Some(latest) = ledger.latest() else {
        return RecoveryDirective::ReadyForInitialPreparation;
    };

    // A late success on an older attempt means continuing from only the latest
    // attempt would be unsafe. AttemptLedger's retry bridge detects this.
    if ledger.retry_inputs().is_err() {
        return RecoveryDirective::Block;
    }

    match latest.status() {
        AttemptStatus::DispatchedEffectUnknown
        | AttemptStatus::Reconciled(ReconciliationOutcome::StillUnknown) => {
            RecoveryDirective::ReconcileDispatchedAttempt {
                ordinal: latest.id.ordinal,
            }
        }
        AttemptStatus::Reconciled(ReconciliationOutcome::NoEffectConfirmed)
        | AttemptStatus::Reconciled(ReconciliationOutcome::EffectFailed)
        | AttemptStatus::EffectConfirmed(ExecutionOutcome::Failed) => {
            RecoveryDirective::EvaluateRetry {
                ordinal: latest.id.ordinal,
            }
        }
        AttemptStatus::Reconciled(ReconciliationOutcome::EffectSucceeded)
        | AttemptStatus::EffectConfirmed(ExecutionOutcome::Succeeded) => {
            RecoveryDirective::CloseSucceeded {
                ordinal: latest.id.ordinal,
            }
        }
    }
}
