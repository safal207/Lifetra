use crate::{
    AttemptStatus, ExecutionOutcome, ReconciliationOutcome, RetryDecision, RetryReason,
    RetryVerdict,
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
    let mut prepared_reconciliations = Vec::<PreparedReconciliationReceipt>::new();
    let mut next_attempt_ordinal = 0_u32;

    for event in events.iter().skip(1) {
        match event {
            JournalEvent::Binding { .. } => return Err(JournalError::DuplicateBinding),
            JournalEvent::Prepared(prepared) => {
                if let Some(existing) = &pending {
                    return Err(JournalError::PendingPreparedAttempt {
                        ordinal: existing.id.ordinal,
                    });
                }
                if prepared.id.ordinal != next_attempt_ordinal {
                    return Err(JournalError::UnexpectedPreparationOrdinal {
                        expected: next_attempt_ordinal,
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

                let prior_prepared_resolution = ordinal.checked_sub(1).and_then(|previous| {
                    latest_terminal_prepared_reconciliation(&prepared_reconciliations, previous)
                        .filter(|_| {
                            !ledger
                                .attempts()
                                .iter()
                                .any(|attempt| attempt.id.ordinal == previous)
                        })
                });
                if let Some(resolution) = prior_prepared_resolution {
                    ledger.record_dispatch_after_external_resolution(
                        &decision,
                        *dispatched_at,
                        proof_ref.clone(),
                        resolution.attempt_id.ordinal,
                        &resolution.proof_ref,
                    )?;
                } else {
                    ledger.record_dispatch(&decision, *dispatched_at, proof_ref.clone())?;
                }
                next_attempt_ordinal = ordinal.saturating_add(1);
            }
            JournalEvent::Reconciliation {
                ordinal,
                observed_at,
                outcome,
                proof_ref,
            } => {
                if let Some(prepared) = pending.as_ref() {
                    if prepared.id.ordinal != *ordinal {
                        return Err(JournalError::PreparedOrdinalMismatch {
                            expected: prepared.id.ordinal,
                            received: *ordinal,
                        });
                    }
                    if *observed_at < prepared.prepared_at {
                        return Err(JournalError::ReconciliationBeforePreparation);
                    }
                    prepared_reconciliations.push(PreparedReconciliationReceipt {
                        attempt_id: prepared.id.clone(),
                        observed_at: *observed_at,
                        outcome: *outcome,
                        proof_ref: proof_ref.clone(),
                    });
                    if *outcome != ReconciliationOutcome::StillUnknown {
                        pending = None;
                        next_attempt_ordinal = ordinal.saturating_add(1);
                    }
                    continue;
                }

                let prepared_only = prepared_reconciliations
                    .iter()
                    .any(|receipt| receipt.attempt_id.ordinal == *ordinal)
                    && !ledger
                        .attempts()
                        .iter()
                        .any(|attempt| attempt.id.ordinal == *ordinal);
                if prepared_only {
                    if let Some(previous) =
                        latest_prepared_reconciliation(&prepared_reconciliations, *ordinal)
                    {
                        if *observed_at < previous.observed_at {
                            return Err(JournalError::ReconciliationBeforePreparation);
                        }
                    }
                    prepared_reconciliations.push(PreparedReconciliationReceipt {
                        attempt_id: AttemptId::new(ticket.action_id.clone(), *ordinal),
                        observed_at: *observed_at,
                        outcome: *outcome,
                        proof_ref: proof_ref.clone(),
                    });
                } else {
                    ledger.record_reconciliation(
                        *ordinal,
                        *observed_at,
                        *outcome,
                        proof_ref.clone(),
                    )?;
                }
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
                ledger.record_external_outcome(
                    *ordinal,
                    *observed_at,
                    *outcome,
                    proof_ref.clone(),
                )?;
            }
        }
    }

    let directive = recovery_directive(
        &ledger,
        pending.as_ref(),
        &prepared_reconciliations,
        next_attempt_ordinal,
    );
    Ok(RecoveredRuntime {
        ledger,
        pending_prepared: pending,
        prepared_reconciliations,
        next_attempt_ordinal,
        directive,
        repaired_truncated_tail: false,
        last_sequence: 0,
    })
}

fn recovery_directive(
    ledger: &AttemptLedger,
    pending: Option<&PreparedAttempt>,
    prepared_reconciliations: &[PreparedReconciliationReceipt],
    next_attempt_ordinal: u32,
) -> RecoveryDirective {
    if has_prepared_conflicting_evidence(prepared_reconciliations) {
        return RecoveryDirective::Block;
    }
    if prepared_success_precedes_later_attempt(prepared_reconciliations, next_attempt_ordinal) {
        return RecoveryDirective::Block;
    }
    if let Some(prepared) = pending {
        return RecoveryDirective::ReconcilePreparedAttempt {
            ordinal: prepared.id.ordinal,
        };
    }
    if ledger.has_conflicting_evidence() {
        return RecoveryDirective::Block;
    }

    if let Some(latest_ordinal) = next_attempt_ordinal.checked_sub(1) {
        if let Some(resolution) =
            latest_terminal_prepared_reconciliation(prepared_reconciliations, latest_ordinal)
                .filter(|_| {
                    !ledger
                        .attempts()
                        .iter()
                        .any(|attempt| attempt.id.ordinal == latest_ordinal)
                })
        {
            return match resolution.outcome {
                ReconciliationOutcome::EffectSucceeded => RecoveryDirective::CloseSucceeded {
                    ordinal: latest_ordinal,
                },
                ReconciliationOutcome::EffectFailed | ReconciliationOutcome::NoEffectConfirmed => {
                    RecoveryDirective::EvaluateRetry {
                        ordinal: latest_ordinal,
                    }
                }
                ReconciliationOutcome::StillUnknown => {
                    RecoveryDirective::ReconcilePreparedAttempt {
                        ordinal: latest_ordinal,
                    }
                }
            };
        }
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

pub(super) fn latest_prepared_reconciliation(
    receipts: &[PreparedReconciliationReceipt],
    ordinal: u32,
) -> Option<&PreparedReconciliationReceipt> {
    receipts
        .iter()
        .rev()
        .find(|receipt| receipt.attempt_id.ordinal == ordinal)
}

pub(super) fn latest_terminal_prepared_reconciliation(
    receipts: &[PreparedReconciliationReceipt],
    ordinal: u32,
) -> Option<&PreparedReconciliationReceipt> {
    receipts.iter().rev().find(|receipt| {
        receipt.attempt_id.ordinal == ordinal
            && receipt.outcome != ReconciliationOutcome::StillUnknown
    })
}

fn has_prepared_conflicting_evidence(receipts: &[PreparedReconciliationReceipt]) -> bool {
    for receipt in receipts {
        if receipt.outcome == ReconciliationOutcome::StillUnknown {
            continue;
        }
        if receipts.iter().any(|other| {
            other.attempt_id.ordinal == receipt.attempt_id.ordinal
                && other.outcome != ReconciliationOutcome::StillUnknown
                && other.outcome != receipt.outcome
        }) {
            return true;
        }
    }
    false
}

fn prepared_success_precedes_later_attempt(
    receipts: &[PreparedReconciliationReceipt],
    next_attempt_ordinal: u32,
) -> bool {
    receipts.iter().any(|receipt| {
        receipt.outcome == ReconciliationOutcome::EffectSucceeded
            && receipt.attempt_id.ordinal.saturating_add(1) < next_attempt_ordinal
    })
}
