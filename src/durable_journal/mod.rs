use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use lifetra_bead::BeadId;
use lifetra_core::Timestamp;

use crate::{
    ActionId, AttemptBlock, AttemptId, AttemptLedger, AuthorityTicket, ExecutionMode,
    ExecutionOutcome, IdempotencyBinding, ReconciliationOutcome, RetryContext, RetryDecision,
    RetryVerdict,
};

mod format;
mod recovery;
#[cfg(test)]
mod tests;

const JOURNAL_VERSION: &str = "LJ1";

/// Durable write-ahead intent for one physical attempt.
///
/// Once this record is fsynced, a crash before a local dispatch receipt must be
/// treated as ambiguous: the external call may or may not have crossed the
/// process boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAttempt {
    pub id: AttemptId,
    pub prepared_at: Timestamp,
    pub authorization_proof_refs: Vec<String>,
}

/// Provider reconciliation evidence for a prepared attempt that has no local
/// dispatch receipt. This is deliberately not a `DispatchReceipt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedReconciliationReceipt {
    pub attempt_id: AttemptId,
    pub observed_at: Timestamp,
    pub outcome: ReconciliationOutcome,
    pub proof_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDirective {
    ReadyForInitialPreparation,
    ReconcilePreparedAttempt { ordinal: u32 },
    ReconcileDispatchedAttempt { ordinal: u32 },
    EvaluateRetry { ordinal: u32 },
    CloseSucceeded { ordinal: u32 },
    Block,
}

/// Replayed restart state reconstructed only from durable journal records.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveredRuntime {
    pub ledger: AttemptLedger,
    pub pending_prepared: Option<PreparedAttempt>,
    pub prepared_reconciliations: Vec<PreparedReconciliationReceipt>,
    pub next_attempt_ordinal: u32,
    pub directive: RecoveryDirective,
    pub repaired_truncated_tail: bool,
    pub last_sequence: u64,
}

impl RecoveredRuntime {
    pub fn retry_context(&self) -> RetryContext {
        RetryContext {
            redispatches_used: self.next_attempt_ordinal.saturating_sub(1),
        }
    }

    pub fn latest_prepared_reconciliation(
        &self,
        ordinal: u32,
    ) -> Option<&PreparedReconciliationReceipt> {
        self.prepared_reconciliations
            .iter()
            .rev()
            .find(|receipt| receipt.attempt_id.ordinal == ordinal)
    }

    pub fn latest_terminal_prepared_reconciliation(
        &self,
        ordinal: u32,
    ) -> Option<&PreparedReconciliationReceipt> {
        self.prepared_reconciliations.iter().rev().find(|receipt| {
            receipt.attempt_id.ordinal == ordinal
                && receipt.outcome != ReconciliationOutcome::StillUnknown
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    Io(String),
    JournalAlreadyExists,
    EmptyJournal,
    MissingBinding,
    DuplicateBinding,
    BindingMustBeFirst,
    BindingActionMismatch,
    EmptyIdempotencyKey,
    EmptyProof,
    InvalidRecord,
    InvalidChecksum { sequence: u64 },
    SequenceGap { expected: u64, received: u64 },
    InvalidHex,
    InvalidUtf8,
    InvalidNumber,
    InvalidExecutionMode,
    InvalidReconciliationOutcome,
    InvalidExecutionOutcome,
    InvalidActionId,
    DecisionActionMismatch,
    DecisionIdempotencyMismatch,
    DecisionDoesNotAuthorizePreparation,
    RecoveryDirectiveDoesNotAuthorizePreparation,
    PendingPreparedAttempt { ordinal: u32 },
    NoPendingPreparedAttempt,
    PreparedOrdinalMismatch { expected: u32, received: u32 },
    DispatchBeforePreparation,
    ReconciliationBeforePreparation,
    UnexpectedPreparationOrdinal { expected: u32, received: u32 },
    RecoveredPreparedRequiresReconciliation { ordinal: u32 },
    PreparedResolutionNotRetrySafe { ordinal: u32 },
    Attempt(AttemptBlock),
}

impl From<AttemptBlock> for JournalError {
    fn from(value: AttemptBlock) -> Self {
        Self::Attempt(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum JournalEvent {
    Binding {
        ticket: AuthorityTicket,
        binding: IdempotencyBinding,
    },
    Prepared(PreparedAttempt),
    Dispatch {
        ordinal: u32,
        dispatched_at: Timestamp,
        proof_ref: String,
    },
    Reconciliation {
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ReconciliationOutcome,
        proof_ref: String,
    },
    External {
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ExecutionOutcome,
        proof_ref: String,
    },
}

/// Append-only file journal with fsync-before-side-effect preparation.
///
/// The journal is intentionally conservative: a durable `Prepared` record with
/// no durable dispatch receipt recovers to `ReconcilePreparedAttempt`, never to
/// permission for a blind dispatch.
pub struct DurableJournal {
    path: PathBuf,
    file: File,
    ticket: AuthorityTicket,
    binding: IdempotencyBinding,
    next_sequence: u64,
    repaired_truncated_tail: bool,
    prepared_in_process: Option<u32>,
}

impl DurableJournal {
    pub fn create(
        path: impl AsRef<Path>,
        ticket: AuthorityTicket,
        binding: IdempotencyBinding,
    ) -> Result<Self, JournalError> {
        if binding.action_id != ticket.action_id {
            return Err(JournalError::BindingActionMismatch);
        }
        if binding.key.trim().is_empty() {
            return Err(JournalError::EmptyIdempotencyKey);
        }

        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(io_error)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    JournalError::JournalAlreadyExists
                } else {
                    io_error(error)
                }
            })?;

        let mut journal = Self {
            path,
            file,
            ticket: ticket.clone(),
            binding: binding.clone(),
            next_sequence: 0,
            repaired_truncated_tail: false,
            prepared_in_process: None,
        };
        journal.append_event(&JournalEvent::Binding { ticket, binding })?;
        Ok(journal)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path).map_err(io_error)?;
        let parsed = format::parse_records(&bytes)?;
        let (ticket, binding) = recovery::binding_from_events(&parsed.events)?;

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(io_error)?;

        if parsed.truncated_tail {
            file.set_len(parsed.valid_len as u64).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
        }
        file.seek(SeekFrom::End(0)).map_err(io_error)?;

        Ok(Self {
            path,
            file,
            ticket,
            binding,
            next_sequence: parsed.next_sequence,
            repaired_truncated_tail: parsed.truncated_tail,
            prepared_in_process: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn ticket(&self) -> &AuthorityTicket {
        &self.ticket
    }

    pub fn binding(&self) -> &IdempotencyBinding {
        &self.binding
    }

    /// Fsyncs a write-ahead attempt intent before the caller performs the
    /// external side effect.
    pub fn prepare_attempt(
        &mut self,
        decision: &RetryDecision,
        prepared_at: Timestamp,
    ) -> Result<AttemptId, JournalError> {
        self.validate_decision_identity(decision)?;
        let recovered = self.recover()?;

        if let Some(pending) = recovered.pending_prepared.as_ref() {
            return Err(JournalError::PendingPreparedAttempt {
                ordinal: pending.id.ordinal,
            });
        }

        let ordinal = match decision.verdict {
            RetryVerdict::InitialDispatchAllowed => 0,
            RetryVerdict::RedispatchAllowed { ordinal } => ordinal,
            RetryVerdict::ReconcileFirst
            | RetryVerdict::CloseSucceeded
            | RetryVerdict::CloseFailed
            | RetryVerdict::Block => return Err(JournalError::DecisionDoesNotAuthorizePreparation),
        };

        let expected = recovered.next_attempt_ordinal;
        if ordinal != expected {
            return Err(JournalError::UnexpectedPreparationOrdinal {
                expected,
                received: ordinal,
            });
        }

        match recovered.directive {
            RecoveryDirective::ReadyForInitialPreparation if ordinal == 0 => {}
            RecoveryDirective::EvaluateRetry { ordinal: previous }
                if ordinal == previous.saturating_add(1) => {}
            _ => return Err(JournalError::RecoveryDirectiveDoesNotAuthorizePreparation),
        }

        if ordinal == 0 {
            let mut probe = recovered.ledger.clone();
            probe.record_dispatch(decision, prepared_at, "journal:prepare-probe")?;
        } else {
            let previous_ordinal = ordinal - 1;
            if let Some(resolution) = recovered
                .latest_terminal_prepared_reconciliation(previous_ordinal)
                .filter(|_| {
                    !recovered
                        .ledger
                        .attempts()
                        .iter()
                        .any(|attempt| attempt.id.ordinal == previous_ordinal)
                })
            {
                if !matches!(
                    resolution.outcome,
                    ReconciliationOutcome::EffectFailed | ReconciliationOutcome::NoEffectConfirmed
                ) {
                    return Err(JournalError::PreparedResolutionNotRetrySafe {
                        ordinal: previous_ordinal,
                    });
                }
                if !decision
                    .proof_refs
                    .iter()
                    .any(|proof| proof == &resolution.proof_ref)
                {
                    return Err(JournalError::Attempt(
                        AttemptBlock::RetryProofLineageMissing,
                    ));
                }
            } else {
                let mut probe = recovered.ledger.clone();
                probe.record_dispatch(decision, prepared_at, "journal:prepare-probe")?;
            }
        }

        let prepared = PreparedAttempt {
            id: AttemptId::new(self.ticket.action_id.clone(), ordinal),
            prepared_at,
            authorization_proof_refs: decision.proof_refs.clone(),
        };
        self.append_event(&JournalEvent::Prepared(prepared.clone()))?;
        self.prepared_in_process = Some(ordinal);
        Ok(prepared.id)
    }

    /// Records durable local evidence that the prepared call crossed the
    /// dispatch boundary. A prepared attempt recovered from a prior process
    /// cannot be marked dispatched without reconciliation.
    pub fn record_dispatch(
        &mut self,
        ordinal: u32,
        dispatched_at: Timestamp,
        proof_ref: impl Into<String>,
    ) -> Result<(), JournalError> {
        let proof_ref = non_empty_proof(proof_ref)?;
        let recovered = self.recover()?;
        let pending = recovered
            .pending_prepared
            .ok_or(JournalError::NoPendingPreparedAttempt)?;
        if pending.id.ordinal != ordinal {
            return Err(JournalError::PreparedOrdinalMismatch {
                expected: pending.id.ordinal,
                received: ordinal,
            });
        }
        if self.prepared_in_process != Some(ordinal) {
            return Err(JournalError::RecoveredPreparedRequiresReconciliation { ordinal });
        }
        if dispatched_at < pending.prepared_at {
            return Err(JournalError::DispatchBeforePreparation);
        }

        self.append_event(&JournalEvent::Dispatch {
            ordinal,
            dispatched_at,
            proof_ref,
        })?;
        self.prepared_in_process = None;
        Ok(())
    }

    /// Persists provider reconciliation evidence. If the ordinal is currently
    /// only `Prepared`, the receipt remains prepared-scoped and does not invent
    /// a local dispatch receipt.
    pub fn record_reconciliation(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ReconciliationOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<(), JournalError> {
        let proof_ref = non_empty_proof(proof_ref)?;
        let recovered = self.recover()?;

        if let Some(pending) = recovered.pending_prepared.as_ref() {
            if pending.id.ordinal != ordinal {
                return Err(JournalError::PreparedOrdinalMismatch {
                    expected: pending.id.ordinal,
                    received: ordinal,
                });
            }
            if observed_at < pending.prepared_at {
                return Err(JournalError::ReconciliationBeforePreparation);
            }
            self.append_event(&JournalEvent::Reconciliation {
                ordinal,
                observed_at,
                outcome,
                proof_ref,
            })?;
            self.prepared_in_process = None;
            return Ok(());
        }

        if let Some(previous) = recovered.latest_prepared_reconciliation(ordinal) {
            if !recovered
                .ledger
                .attempts()
                .iter()
                .any(|attempt| attempt.id.ordinal == ordinal)
            {
                if observed_at < previous.observed_at {
                    return Err(JournalError::ReconciliationBeforePreparation);
                }
                return self.append_event(&JournalEvent::Reconciliation {
                    ordinal,
                    observed_at,
                    outcome,
                    proof_ref,
                });
            }
        }

        let mut probe = recovered.ledger.clone();
        probe.record_reconciliation(ordinal, observed_at, outcome, proof_ref.clone())?;

        self.append_event(&JournalEvent::Reconciliation {
            ordinal,
            observed_at,
            outcome,
            proof_ref,
        })
    }

    pub fn record_external_outcome(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ExecutionOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<(), JournalError> {
        let proof_ref = non_empty_proof(proof_ref)?;
        let recovered = self.recover()?;
        if let Some(pending) = recovered.pending_prepared {
            return Err(JournalError::PendingPreparedAttempt {
                ordinal: pending.id.ordinal,
            });
        }
        let mut probe = recovered.ledger.clone();
        probe.record_external_outcome(ordinal, observed_at, outcome, proof_ref.clone())?;

        self.append_event(&JournalEvent::External {
            ordinal,
            observed_at,
            outcome,
            proof_ref,
        })
    }

    pub fn recover(&self) -> Result<RecoveredRuntime, JournalError> {
        let bytes = fs::read(&self.path).map_err(io_error)?;
        let parsed = format::parse_records(&bytes)?;
        let mut recovered = recovery::replay_events(&parsed.events)?;
        recovered.repaired_truncated_tail = self.repaired_truncated_tail || parsed.truncated_tail;
        recovered.last_sequence = parsed.next_sequence.saturating_sub(1);
        Ok(recovered)
    }

    fn validate_decision_identity(&self, decision: &RetryDecision) -> Result<(), JournalError> {
        if decision.action_id != self.ticket.action_id {
            return Err(JournalError::DecisionActionMismatch);
        }
        if decision.idempotency_key != self.binding.key {
            return Err(JournalError::DecisionIdempotencyMismatch);
        }
        Ok(())
    }

    fn append_event(&mut self, event: &JournalEvent) -> Result<(), JournalError> {
        let payload = format::encode_event(event);
        let sequence = self.next_sequence;
        let checksum = format::checksum(sequence, &payload);
        let line = format!("{sequence}\t{checksum:016x}\t{payload}\n");

        self.file.seek(SeekFrom::End(0)).map_err(io_error)?;
        self.file.write_all(line.as_bytes()).map_err(io_error)?;
        // Deliberate write-ahead durability barrier before external side effects.
        self.file.sync_all().map_err(io_error)?;
        self.next_sequence += 1;
        Ok(())
    }
}

fn non_empty_proof(value: impl Into<String>) -> Result<String, JournalError> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(JournalError::EmptyProof);
    }
    Ok(value)
}

fn io_error(error: std::io::Error) -> JournalError {
    JournalError::Io(error.to_string())
}
