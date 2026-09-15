use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use lifetra_bead::{EvidenceRef, EvidenceStatus};
use lifetra_core::Timestamp;

use crate::{
    ActionId, DurableJournal, ExecutionOutcome, FencedActuatorOutcome, FencedActuatorReceipt,
    FencedActuatorRejection, FencedActuatorRequest, FencedAttemptPermit, FencedDurableJournal,
    FencedJournalError, InMemoryFencedActuator, InMemoryFencedActuatorError, JournalError,
    ReconciliationOutcome, RecoveryDirective, RecoveryLeaseStore, RecoveryWorkerId, RetryDecision,
};

const BRIDGE_VERSION: &str = "AB1";

/// Stable semantic identity for one logical side effect.
///
/// The binding is persisted before any actuator request is returned to the caller,
/// so a restart can reconcile by `ActionId + idempotency_key + operation_ref` even
/// if the process died after the external effect but before persisting the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActuatorBridgeBinding {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub operation_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedActuatorCall {
    pub permit: FencedAttemptPermit,
    pub prepared_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActuatorRecoveryQuery {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub operation_ref: String,
    pub attempt_ordinal: u32,
    pub prepared_owner: RecoveryWorkerId,
    pub prepared_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorRecoveryOutcome {
    EffectSucceeded {
        applied_epoch: u64,
        applied_attempt_ordinal: u32,
    },
    StillUnknown,
    IdentityConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActuatorRecoveryObservation {
    pub query: ActuatorRecoveryQuery,
    pub observed_at: Timestamp,
    pub outcome: ActuatorRecoveryOutcome,
    pub proof_ref: String,
}

pub trait ActuatorRecoveryAdapter {
    type Error;

    fn reconcile(
        &self,
        query: &ActuatorRecoveryQuery,
        observed_at: Timestamp,
    ) -> Result<ActuatorRecoveryObservation, Self::Error>;
}

/// Reference reconciliation adapter for the process-local fenced actuator.
///
/// Absence is deliberately `StillUnknown`, never `NoEffectConfirmed`.
impl ActuatorRecoveryAdapter for InMemoryFencedActuator {
    type Error = InMemoryFencedActuatorError;

    fn reconcile(
        &self,
        query: &ActuatorRecoveryQuery,
        observed_at: Timestamp,
    ) -> Result<ActuatorRecoveryObservation, Self::Error> {
        let effect = self.applied_effect(&query.action_id)?;
        let outcome = match effect {
            Some(effect)
                if effect.idempotency_key == query.idempotency_key
                    && effect.operation_ref == query.operation_ref =>
            {
                ActuatorRecoveryOutcome::EffectSucceeded {
                    applied_epoch: effect.applied_epoch,
                    applied_attempt_ordinal: effect.applied_attempt_ordinal,
                }
            }
            Some(_) => ActuatorRecoveryOutcome::IdentityConflict,
            None => ActuatorRecoveryOutcome::StillUnknown,
        };
        let proof_ref = match &outcome {
            ActuatorRecoveryOutcome::EffectSucceeded {
                applied_epoch,
                applied_attempt_ordinal,
            } => format!(
                "proof:in-memory-actuator-recovery:{}:{}:{}",
                query.action_id.as_str(),
                applied_epoch,
                applied_attempt_ordinal
            ),
            ActuatorRecoveryOutcome::StillUnknown => format!(
                "proof:in-memory-actuator-recovery:{}:unknown",
                query.action_id.as_str()
            ),
            ActuatorRecoveryOutcome::IdentityConflict => format!(
                "proof:in-memory-actuator-recovery:{}:identity-conflict",
                query.action_id.as_str()
            ),
        };

        Ok(ActuatorRecoveryObservation {
            query: query.clone(),
            observed_at,
            outcome,
            proof_ref,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableActuatorEvidence {
    Receipt {
        sequence: u64,
        receipt: FencedActuatorReceipt,
    },
    Recovery {
        sequence: u64,
        observation: ActuatorRecoveryObservation,
    },
}

impl DurableActuatorEvidence {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Receipt { sequence, .. } | Self::Recovery { sequence, .. } => *sequence,
        }
    }

    pub fn proof_ref(&self) -> &str {
        match self {
            Self::Receipt { receipt, .. } => &receipt.proof_ref,
            Self::Recovery { observation, .. } => &observation.proof_ref,
        }
    }

    pub fn attempt_ordinal(&self) -> u32 {
        match self {
            Self::Receipt { receipt, .. } => receipt.request.attempt_ordinal,
            Self::Recovery { observation, .. } => observation.query.attempt_ordinal,
        }
    }

    pub fn confirms_effect(&self) -> bool {
        match self {
            Self::Receipt { receipt, .. } => matches!(
                receipt.outcome,
                FencedActuatorOutcome::Applied | FencedActuatorOutcome::AlreadyApplied { .. }
            ),
            Self::Recovery { observation, .. } => matches!(
                observation.outcome,
                ActuatorRecoveryOutcome::EffectSucceeded { .. }
            ),
        }
    }

    pub fn has_identity_conflict(&self) -> bool {
        match self {
            Self::Receipt { receipt, .. } => matches!(
                receipt.outcome,
                FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict)
            ),
            Self::Recovery { observation, .. } => {
                observation.outcome == ActuatorRecoveryOutcome::IdentityConflict
            }
        }
    }

    /// Converts only positive external effect evidence into bead evidence.
    /// Rejections and unknown observations never become a supported transition.
    pub fn as_supported_evidence(&self) -> Result<EvidenceRef, ActuatorBridgeError> {
        if !self.confirms_effect() {
            return Err(ActuatorBridgeError::EvidenceDoesNotConfirmEffect);
        }
        Ok(EvidenceRef::new(
            self.proof_ref(),
            EvidenceStatus::Supported,
            "fenced actuator confirmed the logical side effect",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredActuatorBridge {
    pub binding: ActuatorBridgeBinding,
    pub prepared_calls: Vec<PreparedActuatorCall>,
    pub evidence: Vec<DurableActuatorEvidence>,
    pub synced_sequences: Vec<u64>,
    pub last_sequence: u64,
}

impl RecoveredActuatorBridge {
    pub fn latest_prepared(&self, ordinal: u32) -> Option<&PreparedActuatorCall> {
        self.prepared_calls
            .iter()
            .rev()
            .find(|prepared| prepared.permit.attempt_ordinal == ordinal)
    }

    pub fn pending_evidence(&self) -> Vec<&DurableActuatorEvidence> {
        let synced: HashSet<u64> = self.synced_sequences.iter().copied().collect();
        self.evidence
            .iter()
            .filter(|evidence| !synced.contains(&evidence.sequence()))
            .collect()
    }

    pub fn latest_success_evidence(&self) -> Option<&DurableActuatorEvidence> {
        self.evidence
            .iter()
            .rev()
            .find(|evidence| evidence.confirms_effect())
    }

    pub fn has_identity_conflict(&self) -> bool {
        self.evidence
            .iter()
            .any(DurableActuatorEvidence::has_identity_conflict)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorBridgeDirective {
    Clean,
    ProjectEvidence { sequence: u64 },
    Reconcile(ActuatorRecoveryQuery),
    CloseSucceeded { proof_ref: String },
    BlockIdentityConflict { proof_ref: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorBridgeError {
    Io(String),
    BridgeAlreadyExists,
    EmptyBridge,
    MissingBinding,
    DuplicateBinding,
    BindingMustBeFirst,
    EmptyIdempotencyKey,
    EmptyOperationRef,
    EmptyProof,
    InvalidRecord,
    SequenceGap { expected: u64, received: u64 },
    InvalidHex,
    InvalidUtf8,
    InvalidNumber,
    InvalidActionId,
    InvalidWorkerId,
    InvalidReceiptOutcome,
    InvalidRecoveryOutcome,
    Journal(JournalError),
    JournalActionMismatch,
    JournalIdempotencyMismatch,
    PreparedIdentityMismatch,
    ReceiptIdentityMismatch,
    RecoveryIdentityMismatch,
    MissingPreparedCall { ordinal: u32 },
    ObservationBeforePreparation,
    NotReconciliationState,
    EvidenceDoesNotConfirmEffect,
}

impl From<JournalError> for ActuatorBridgeError {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorBridgeRuntimeError<E> {
    Bridge(ActuatorBridgeError),
    Fenced(FencedJournalError<E>),
}

impl<E> From<ActuatorBridgeError> for ActuatorBridgeRuntimeError<E> {
    fn from(value: ActuatorBridgeError) -> Self {
        Self::Bridge(value)
    }
}

impl<E> From<FencedJournalError<E>> for ActuatorBridgeRuntimeError<E> {
    fn from(value: FencedJournalError<E>) -> Self {
        Self::Fenced(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActuatorBridgeReconcileError<E> {
    Bridge(ActuatorBridgeError),
    Adapter(E),
}

impl<E> From<ActuatorBridgeError> for ActuatorBridgeReconcileError<E> {
    fn from(value: ActuatorBridgeError) -> Self {
        Self::Bridge(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BridgeEvent {
    Binding(ActuatorBridgeBinding),
    Prepared(PreparedActuatorCall),
    Receipt(FencedActuatorReceipt),
    Recovery(ActuatorRecoveryObservation),
    Synced { evidence_sequence: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BridgeRecord {
    sequence: u64,
    event: BridgeEvent,
}

/// Durable sidecar that preserves operation identity and actuator evidence.
///
/// Each event is written to an immutable sequence file via temp-file + fsync +
/// rename + directory fsync. The sidecar remains separate from `DurableJournal`:
/// a crash between the two stores is repaired by `project_pending`, not hidden.
pub struct DurableActuatorReceiptBridge {
    path: PathBuf,
    binding: ActuatorBridgeBinding,
    next_sequence: u64,
}

impl DurableActuatorReceiptBridge {
    pub fn create(
        path: impl AsRef<Path>,
        journal: &DurableJournal,
        operation_ref: impl Into<String>,
    ) -> Result<Self, ActuatorBridgeError> {
        let operation_ref = operation_ref.into();
        if operation_ref.trim().is_empty() {
            return Err(ActuatorBridgeError::EmptyOperationRef);
        }
        if journal.binding().key.trim().is_empty() {
            return Err(ActuatorBridgeError::EmptyIdempotencyKey);
        }

        let path = path.as_ref().to_path_buf();
        fs::create_dir(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ActuatorBridgeError::BridgeAlreadyExists
            } else {
                io_error(error)
            }
        })?;
        let binding = ActuatorBridgeBinding {
            action_id: journal.ticket().action_id.clone(),
            idempotency_key: journal.binding().key.clone(),
            operation_ref,
        };
        let mut bridge = Self {
            path,
            binding: binding.clone(),
            next_sequence: 0,
        };
        bridge.append_event(&BridgeEvent::Binding(binding))?;
        Ok(bridge)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ActuatorBridgeError> {
        let path = path.as_ref().to_path_buf();
        let records = read_records(&path)?;
        let binding = binding_from_records(&records)?;
        let next_sequence = records
            .last()
            .map_or(0, |record| record.sequence.saturating_add(1));
        Ok(Self {
            path,
            binding,
            next_sequence,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn binding(&self) -> &ActuatorBridgeBinding {
        &self.binding
    }

    /// Main journal PREP is fsynced first; the fenced permit is then persisted in
    /// the bridge. The caller only receives the permit after both durable writes.
    pub fn prepare<S: RecoveryLeaseStore>(
        &mut self,
        fenced: &mut FencedDurableJournal<S>,
        decision: &RetryDecision,
        prepared_at: Timestamp,
        now: Timestamp,
    ) -> Result<FencedAttemptPermit, ActuatorBridgeRuntimeError<S::Error>> {
        self.validate_journal_identity(fenced.journal())?;
        let permit = fenced.prepare_attempt(decision, prepared_at, now)?;
        self.validate_permit_identity(&permit)?;
        self.append_event(&BridgeEvent::Prepared(PreparedActuatorCall {
            permit: permit.clone(),
            prepared_at,
        }))?;
        Ok(permit)
    }

    /// Saves the external receipt before attempting to project it into the main journal.
    pub fn persist_receipt(
        &mut self,
        receipt: &FencedActuatorReceipt,
    ) -> Result<u64, ActuatorBridgeError> {
        self.validate_receipt(receipt)?;
        self.append_event(&BridgeEvent::Receipt(receipt.clone()))
    }

    /// Builds the exact identity required after a crash. The operation reference
    /// comes from the bridge BIND record, not from volatile process memory.
    pub fn recovery_query(
        &self,
        journal: &DurableJournal,
    ) -> Result<ActuatorRecoveryQuery, ActuatorBridgeError> {
        self.validate_journal_identity(journal)?;
        let journal_state = journal.recover()?;
        let ordinal = match journal_state.directive {
            RecoveryDirective::ReconcilePreparedAttempt { ordinal }
            | RecoveryDirective::ReconcileDispatchedAttempt { ordinal } => ordinal,
            _ => return Err(ActuatorBridgeError::NotReconciliationState),
        };
        let state = self.recover()?;
        let prepared = state
            .latest_prepared(ordinal)
            .ok_or(ActuatorBridgeError::MissingPreparedCall { ordinal })?;
        Ok(ActuatorRecoveryQuery {
            action_id: self.binding.action_id.clone(),
            idempotency_key: self.binding.idempotency_key.clone(),
            operation_ref: self.binding.operation_ref.clone(),
            attempt_ordinal: ordinal,
            prepared_owner: prepared.permit.owner.clone(),
            prepared_epoch: prepared.permit.fencing_epoch,
        })
    }

    pub fn reconcile_after_crash<A: ActuatorRecoveryAdapter>(
        &mut self,
        journal: &DurableJournal,
        adapter: &A,
        observed_at: Timestamp,
    ) -> Result<ActuatorRecoveryObservation, ActuatorBridgeReconcileError<A::Error>> {
        let query = self.recovery_query(journal)?;
        let observation = adapter
            .reconcile(&query, observed_at)
            .map_err(ActuatorBridgeReconcileError::Adapter)?;
        self.persist_recovery_observation(&observation)?;
        Ok(observation)
    }

    pub fn persist_recovery_observation(
        &mut self,
        observation: &ActuatorRecoveryObservation,
    ) -> Result<u64, ActuatorBridgeError> {
        self.validate_recovery_observation(observation)?;
        self.append_event(&BridgeEvent::Recovery(observation.clone()))
    }

    /// Converges durable actuator evidence into the main execution journal.
    ///
    /// If the journal write succeeded before a crash but the bridge SYNC marker did
    /// not, proof-reference detection makes the replay idempotent.
    pub fn project_pending<S: RecoveryLeaseStore>(
        &mut self,
        fenced: &mut FencedDurableJournal<S>,
        now: Timestamp,
    ) -> Result<usize, ActuatorBridgeRuntimeError<S::Error>> {
        self.validate_journal_identity(fenced.journal())?;
        let pending_state = self.recover()?;
        let sequences: Vec<u64> = pending_state
            .pending_evidence()
            .iter()
            .map(|evidence| evidence.sequence())
            .collect();
        let mut projected = 0_usize;

        for sequence in sequences {
            let state = self.recover()?;
            let evidence = state
                .evidence
                .iter()
                .find(|evidence| evidence.sequence() == sequence)
                .cloned()
                .ok_or(ActuatorBridgeError::InvalidRecord)?;
            let journal_state = fenced.journal().recover()?;
            if !journal_contains_proof(&journal_state, evidence.proof_ref()) {
                self.project_one(fenced, &evidence, now)?;
            }
            self.append_event(&BridgeEvent::Synced {
                evidence_sequence: sequence,
            })?;
            projected += 1;
        }
        Ok(projected)
    }

    pub fn recover(&self) -> Result<RecoveredActuatorBridge, ActuatorBridgeError> {
        let records = read_records(&self.path)?;
        let binding = binding_from_records(&records)?;
        let mut prepared_calls = Vec::new();
        let mut evidence = Vec::new();
        let mut synced_sequences = Vec::new();

        for record in records.iter().skip(1) {
            match &record.event {
                BridgeEvent::Binding(_) => return Err(ActuatorBridgeError::DuplicateBinding),
                BridgeEvent::Prepared(prepared) => prepared_calls.push(prepared.clone()),
                BridgeEvent::Receipt(receipt) => evidence.push(DurableActuatorEvidence::Receipt {
                    sequence: record.sequence,
                    receipt: receipt.clone(),
                }),
                BridgeEvent::Recovery(observation) => {
                    evidence.push(DurableActuatorEvidence::Recovery {
                        sequence: record.sequence,
                        observation: observation.clone(),
                    });
                }
                BridgeEvent::Synced { evidence_sequence } => {
                    synced_sequences.push(*evidence_sequence);
                }
            }
        }

        Ok(RecoveredActuatorBridge {
            binding,
            prepared_calls,
            evidence,
            synced_sequences,
            last_sequence: records.last().map_or(0, |record| record.sequence),
        })
    }

    pub fn directive(
        &self,
        journal: &DurableJournal,
    ) -> Result<ActuatorBridgeDirective, ActuatorBridgeError> {
        self.validate_journal_identity(journal)?;
        let state = self.recover()?;
        if let Some(conflict) = state
            .evidence
            .iter()
            .rev()
            .find(|evidence| evidence.has_identity_conflict())
        {
            return Ok(ActuatorBridgeDirective::BlockIdentityConflict {
                proof_ref: conflict.proof_ref().to_owned(),
            });
        }
        if let Some(pending) = state.pending_evidence().first() {
            return Ok(ActuatorBridgeDirective::ProjectEvidence {
                sequence: pending.sequence(),
            });
        }
        let journal_state = journal.recover()?;
        if matches!(
            journal_state.directive,
            RecoveryDirective::CloseSucceeded { .. }
        ) {
            if let Some(success) = state.latest_success_evidence() {
                return Ok(ActuatorBridgeDirective::CloseSucceeded {
                    proof_ref: success.proof_ref().to_owned(),
                });
            }
        }
        if matches!(
            journal_state.directive,
            RecoveryDirective::ReconcilePreparedAttempt { .. }
                | RecoveryDirective::ReconcileDispatchedAttempt { .. }
        ) {
            return Ok(ActuatorBridgeDirective::Reconcile(
                self.recovery_query(journal)?,
            ));
        }
        Ok(ActuatorBridgeDirective::Clean)
    }

    fn project_one<S: RecoveryLeaseStore>(
        &self,
        fenced: &mut FencedDurableJournal<S>,
        evidence: &DurableActuatorEvidence,
        now: Timestamp,
    ) -> Result<(), ActuatorBridgeRuntimeError<S::Error>> {
        let ordinal = evidence.attempt_ordinal();
        let observed_at = match evidence {
            DurableActuatorEvidence::Receipt { receipt, .. } => receipt.observed_at,
            DurableActuatorEvidence::Recovery { observation, .. } => observation.observed_at,
        };
        let proof_ref = evidence.proof_ref().to_owned();

        match evidence {
            DurableActuatorEvidence::Receipt { receipt, .. } => match receipt.outcome {
                FencedActuatorOutcome::Applied => {
                    let journal_state = fenced.journal().recover()?;
                    let locally_dispatched = journal_state
                        .ledger
                        .attempts()
                        .iter()
                        .any(|attempt| attempt.id.ordinal == ordinal);
                    if locally_dispatched {
                        fenced.record_external_outcome(
                            ordinal,
                            observed_at,
                            ExecutionOutcome::Succeeded,
                            proof_ref,
                            now,
                        )?;
                    } else {
                        fenced.record_reconciliation(
                            ordinal,
                            observed_at,
                            ReconciliationOutcome::EffectSucceeded,
                            proof_ref,
                            now,
                        )?;
                    }
                }
                FencedActuatorOutcome::AlreadyApplied { .. } => fenced.record_reconciliation(
                    ordinal,
                    observed_at,
                    ReconciliationOutcome::EffectSucceeded,
                    proof_ref,
                    now,
                )?,
                FencedActuatorOutcome::Rejected(_) => fenced.record_reconciliation(
                    ordinal,
                    observed_at,
                    ReconciliationOutcome::StillUnknown,
                    proof_ref,
                    now,
                )?,
            },
            DurableActuatorEvidence::Recovery { observation, .. } => {
                let outcome = match observation.outcome {
                    ActuatorRecoveryOutcome::EffectSucceeded { .. } => {
                        ReconciliationOutcome::EffectSucceeded
                    }
                    ActuatorRecoveryOutcome::StillUnknown
                    | ActuatorRecoveryOutcome::IdentityConflict => {
                        ReconciliationOutcome::StillUnknown
                    }
                };
                fenced.record_reconciliation(ordinal, observed_at, outcome, proof_ref, now)?;
            }
        }
        Ok(())
    }

    fn validate_journal_identity(&self, journal: &DurableJournal) -> Result<(), ActuatorBridgeError> {
        if journal.ticket().action_id != self.binding.action_id {
            return Err(ActuatorBridgeError::JournalActionMismatch);
        }
        if journal.binding().key != self.binding.idempotency_key {
            return Err(ActuatorBridgeError::JournalIdempotencyMismatch);
        }
        Ok(())
    }

    fn validate_permit_identity(
        &self,
        permit: &FencedAttemptPermit,
    ) -> Result<(), ActuatorBridgeError> {
        if permit.action_id != self.binding.action_id
            || permit.idempotency_key != self.binding.idempotency_key
        {
            return Err(ActuatorBridgeError::PreparedIdentityMismatch);
        }
        Ok(())
    }

    fn validate_receipt(&self, receipt: &FencedActuatorReceipt) -> Result<(), ActuatorBridgeError> {
        if receipt.proof_ref.trim().is_empty() {
            return Err(ActuatorBridgeError::EmptyProof);
        }
        if receipt.request.action_id != self.binding.action_id
            || receipt.request.idempotency_key != self.binding.idempotency_key
            || receipt.request.operation_ref != self.binding.operation_ref
        {
            return Err(ActuatorBridgeError::ReceiptIdentityMismatch);
        }
        let state = self.recover()?;
        let prepared = state
            .latest_prepared(receipt.request.attempt_ordinal)
            .ok_or(ActuatorBridgeError::MissingPreparedCall {
                ordinal: receipt.request.attempt_ordinal,
            })?;
        if prepared.permit.owner != receipt.request.owner
            || prepared.permit.fencing_epoch != receipt.request.fencing_epoch
        {
            return Err(ActuatorBridgeError::ReceiptIdentityMismatch);
        }
        if receipt.observed_at < prepared.prepared_at {
            return Err(ActuatorBridgeError::ObservationBeforePreparation);
        }
        Ok(())
    }

    fn validate_recovery_observation(
        &self,
        observation: &ActuatorRecoveryObservation,
    ) -> Result<(), ActuatorBridgeError> {
        if observation.proof_ref.trim().is_empty() {
            return Err(ActuatorBridgeError::EmptyProof);
        }
        let query = &observation.query;
        if query.action_id != self.binding.action_id
            || query.idempotency_key != self.binding.idempotency_key
            || query.operation_ref != self.binding.operation_ref
        {
            return Err(ActuatorBridgeError::RecoveryIdentityMismatch);
        }
        let state = self.recover()?;
        let prepared = state
            .latest_prepared(query.attempt_ordinal)
            .ok_or(ActuatorBridgeError::MissingPreparedCall {
                ordinal: query.attempt_ordinal,
            })?;
        if prepared.permit.owner != query.prepared_owner
            || prepared.permit.fencing_epoch != query.prepared_epoch
        {
            return Err(ActuatorBridgeError::RecoveryIdentityMismatch);
        }
        if observation.observed_at < prepared.prepared_at {
            return Err(ActuatorBridgeError::ObservationBeforePreparation);
        }
        Ok(())
    }

    fn append_event(&mut self, event: &BridgeEvent) -> Result<u64, ActuatorBridgeError> {
        let sequence = self.next_sequence;
        let final_path = self.path.join(format!("{sequence:020}.evt"));
        let temp_path = self.path.join(format!(
            ".tmp-{}-{sequence:020}",
            std::process::id()
        ));
        let payload = encode_event(event);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(io_error)?;
        file.write_all(payload.as_bytes()).map_err(io_error)?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(&temp_path, &final_path).map_err(io_error)?;
        sync_directory(&self.path)?;
        self.next_sequence += 1;
        Ok(sequence)
    }
}

fn journal_contains_proof(state: &crate::RecoveredRuntime, proof_ref: &str) -> bool {
    if state
        .prepared_reconciliations
        .iter()
        .any(|receipt| receipt.proof_ref == proof_ref)
    {
        return true;
    }
    state.ledger.attempts().iter().any(|attempt| {
        attempt.dispatch.proof_ref == proof_ref
            || attempt
                .reconciliations
                .iter()
                .any(|receipt| receipt.proof_ref == proof_ref)
            || attempt
                .external
                .as_ref()
                .is_some_and(|receipt| receipt.proof_ref == proof_ref)
    })
}

fn read_records(path: &Path) -> Result<Vec<BridgeRecord>, ActuatorBridgeError> {
    if !path.is_dir() {
        return Err(ActuatorBridgeError::EmptyBridge);
    }
    let mut entries = Vec::<(u64, PathBuf)>::new();
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(sequence_raw) = name.strip_suffix(".evt") else {
            continue;
        };
        let sequence = sequence_raw
            .parse::<u64>()
            .map_err(|_| ActuatorBridgeError::InvalidRecord)?;
        entries.push((sequence, entry.path()));
    }
    entries.sort_by_key(|(sequence, _)| *sequence);
    if entries.is_empty() {
        return Err(ActuatorBridgeError::EmptyBridge);
    }

    let mut records = Vec::with_capacity(entries.len());
    for (expected, (sequence, file_path)) in entries.into_iter().enumerate() {
        let expected = expected as u64;
        if sequence != expected {
            return Err(ActuatorBridgeError::SequenceGap {
                expected,
                received: sequence,
            });
        }
        let payload = fs::read_to_string(file_path).map_err(io_error)?;
        let payload = payload
            .strip_suffix('\n')
            .ok_or(ActuatorBridgeError::InvalidRecord)?;
        records.push(BridgeRecord {
            sequence,
            event: decode_event(payload)?,
        });
    }

    normalize_records(records)
}

fn binding_from_records(records: &[BridgeRecord]) -> Result<ActuatorBridgeBinding, ActuatorBridgeError> {
    let first = records.first().ok_or(ActuatorBridgeError::MissingBinding)?;
    let binding = match &first.event {
        BridgeEvent::Binding(binding) => binding.clone(),
        _ => return Err(ActuatorBridgeError::BindingMustBeFirst),
    };
    if records
        .iter()
        .skip(1)
        .any(|record| matches!(record.event, BridgeEvent::Binding(_)))
    {
        return Err(ActuatorBridgeError::DuplicateBinding);
    }
    Ok(binding)
}

fn normalize_records(
    mut records: Vec<BridgeRecord>,
) -> Result<Vec<BridgeRecord>, ActuatorBridgeError> {
    let binding = binding_from_records(&records)?;
    for record in records.iter_mut().skip(1) {
        match &mut record.event {
            BridgeEvent::Prepared(prepared) => {
                prepared.permit.action_id = binding.action_id.clone();
                prepared.permit.idempotency_key = binding.idempotency_key.clone();
            }
            BridgeEvent::Receipt(receipt) => {
                receipt.request.action_id = binding.action_id.clone();
                receipt.request.idempotency_key = binding.idempotency_key.clone();
                receipt.request.operation_ref = binding.operation_ref.clone();
            }
            BridgeEvent::Recovery(observation) => {
                observation.query.action_id = binding.action_id.clone();
                observation.query.idempotency_key = binding.idempotency_key.clone();
                observation.query.operation_ref = binding.operation_ref.clone();
            }
            BridgeEvent::Binding(_) | BridgeEvent::Synced { .. } => {}
        }
    }
    Ok(records)
}

fn encode_event(event: &BridgeEvent) -> String {
    match event {
        BridgeEvent::Binding(binding) => [
            BRIDGE_VERSION.to_owned(),
            "BIND".to_owned(),
            encode_string(binding.action_id.as_str()),
            encode_string(&binding.idempotency_key),
            encode_string(&binding.operation_ref),
        ]
        .join("\t"),
        BridgeEvent::Prepared(prepared) => [
            BRIDGE_VERSION.to_owned(),
            "PREP".to_owned(),
            encode_string(prepared.permit.owner.as_str()),
            prepared.permit.fencing_epoch.to_string(),
            prepared.permit.attempt_ordinal.to_string(),
            prepared.prepared_at.epoch_seconds().to_string(),
        ]
        .join("\t"),
        BridgeEvent::Receipt(receipt) => {
            let (code, arg1, arg2) = encode_receipt_outcome(&receipt.outcome);
            [
                BRIDGE_VERSION.to_owned(),
                "RCPT".to_owned(),
                encode_string(receipt.request.owner.as_str()),
                receipt.request.fencing_epoch.to_string(),
                receipt.request.attempt_ordinal.to_string(),
                receipt.observed_at.epoch_seconds().to_string(),
                code.to_owned(),
                arg1.to_string(),
                arg2.to_string(),
                encode_string(&receipt.proof_ref),
            ]
            .join("\t")
        }
        BridgeEvent::Recovery(observation) => {
            let (code, arg1, arg2) = encode_recovery_outcome(&observation.outcome);
            [
                BRIDGE_VERSION.to_owned(),
                "RECO".to_owned(),
                encode_string(observation.query.prepared_owner.as_str()),
                observation.query.prepared_epoch.to_string(),
                observation.query.attempt_ordinal.to_string(),
                observation.observed_at.epoch_seconds().to_string(),
                code.to_owned(),
                arg1.to_string(),
                arg2.to_string(),
                encode_string(&observation.proof_ref),
            ]
            .join("\t")
        }
        BridgeEvent::Synced { evidence_sequence } => [
            BRIDGE_VERSION.to_owned(),
            "SYNC".to_owned(),
            evidence_sequence.to_string(),
        ]
        .join("\t"),
    }
}

fn decode_event(payload: &str) -> Result<BridgeEvent, ActuatorBridgeError> {
    let mut fields = payload.split('\t');
    if fields.next() != Some(BRIDGE_VERSION) {
        return Err(ActuatorBridgeError::InvalidRecord);
    }
    let kind = fields.next().ok_or(ActuatorBridgeError::InvalidRecord)?;
    let remaining: Vec<&str> = fields.collect();
    match kind {
        "BIND" => decode_binding(&remaining),
        "PREP" => decode_prepared(&remaining),
        "RCPT" => decode_receipt(&remaining),
        "RECO" => decode_recovery(&remaining),
        "SYNC" => {
            if remaining.len() != 1 {
                return Err(ActuatorBridgeError::InvalidRecord);
            }
            Ok(BridgeEvent::Synced {
                evidence_sequence: parse_u64(Some(remaining[0]))?,
            })
        }
        _ => Err(ActuatorBridgeError::InvalidRecord),
    }
}

fn decode_binding(fields: &[&str]) -> Result<BridgeEvent, ActuatorBridgeError> {
    if fields.len() != 3 {
        return Err(ActuatorBridgeError::InvalidRecord);
    }
    let action_id = ActionId::new(decode_string(fields[0])?)
        .map_err(|_| ActuatorBridgeError::InvalidActionId)?;
    let idempotency_key = decode_string(fields[1])?;
    let operation_ref = decode_string(fields[2])?;
    if idempotency_key.trim().is_empty() {
        return Err(ActuatorBridgeError::EmptyIdempotencyKey);
    }
    if operation_ref.trim().is_empty() {
        return Err(ActuatorBridgeError::EmptyOperationRef);
    }
    Ok(BridgeEvent::Binding(ActuatorBridgeBinding {
        action_id,
        idempotency_key,
        operation_ref,
    }))
}

fn decode_prepared(fields: &[&str]) -> Result<BridgeEvent, ActuatorBridgeError> {
    if fields.len() != 4 {
        return Err(ActuatorBridgeError::InvalidRecord);
    }
    let owner = RecoveryWorkerId::new(decode_string(fields[0])?)
        .map_err(|_| ActuatorBridgeError::InvalidWorkerId)?;
    Ok(BridgeEvent::Prepared(PreparedActuatorCall {
        permit: FencedAttemptPermit {
            action_id: placeholder_action()?,
            owner,
            fencing_epoch: parse_u64(Some(fields[1]))?,
            attempt_ordinal: parse_u32(Some(fields[2]))?,
            idempotency_key: String::new(),
        },
        prepared_at: Timestamp::new(parse_i64(Some(fields[3]))?),
    }))
}

fn decode_receipt(fields: &[&str]) -> Result<BridgeEvent, ActuatorBridgeError> {
    if fields.len() != 8 {
        return Err(ActuatorBridgeError::InvalidRecord);
    }
    let owner = RecoveryWorkerId::new(decode_string(fields[0])?)
        .map_err(|_| ActuatorBridgeError::InvalidWorkerId)?;
    Ok(BridgeEvent::Receipt(FencedActuatorReceipt {
        request: FencedActuatorRequest {
            action_id: placeholder_action()?,
            owner,
            fencing_epoch: parse_u64(Some(fields[1]))?,
            attempt_ordinal: parse_u32(Some(fields[2]))?,
            idempotency_key: String::new(),
            operation_ref: String::new(),
        },
        observed_at: Timestamp::new(parse_i64(Some(fields[3]))?),
        outcome: decode_receipt_outcome(
            fields[4],
            parse_u64(Some(fields[5]))?,
            parse_u32(Some(fields[6]))?,
        )?,
        proof_ref: decode_string(fields[7])?,
    }))
}

fn decode_recovery(fields: &[&str]) -> Result<BridgeEvent, ActuatorBridgeError> {
    if fields.len() != 8 {
        return Err(ActuatorBridgeError::InvalidRecord);
    }
    let owner = RecoveryWorkerId::new(decode_string(fields[0])?)
        .map_err(|_| ActuatorBridgeError::InvalidWorkerId)?;
    Ok(BridgeEvent::Recovery(ActuatorRecoveryObservation {
        query: ActuatorRecoveryQuery {
            action_id: placeholder_action()?,
            idempotency_key: String::new(),
            operation_ref: String::new(),
            attempt_ordinal: parse_u32(Some(fields[2]))?,
            prepared_owner: owner,
            prepared_epoch: parse_u64(Some(fields[1]))?,
        },
        observed_at: Timestamp::new(parse_i64(Some(fields[3]))?),
        outcome: decode_recovery_outcome(
            fields[4],
            parse_u64(Some(fields[5]))?,
            parse_u32(Some(fields[6]))?,
        )?,
        proof_ref: decode_string(fields[7])?,
    }))
}

fn placeholder_action() -> Result<ActionId, ActuatorBridgeError> {
    ActionId::new("bridge:placeholder").map_err(|_| ActuatorBridgeError::InvalidActionId)
}

fn encode_receipt_outcome(outcome: &FencedActuatorOutcome) -> (&'static str, u64, u32) {
    match outcome {
        FencedActuatorOutcome::Applied => ("A", 0, 0),
        FencedActuatorOutcome::AlreadyApplied {
            applied_epoch,
            applied_attempt_ordinal,
        } => ("D", *applied_epoch, *applied_attempt_ordinal),
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch { current_epoch }) => {
            ("S", *current_epoch, 0)
        }
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::FutureEpoch { current_epoch }) => {
            ("F", *current_epoch, 0)
        }
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::WrongOwner) => ("W", 0, 0),
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::LeaseExpired { current_epoch }) => {
            ("L", *current_epoch, 0)
        }
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict) => ("I", 0, 0),
    }
}

fn decode_receipt_outcome(
    code: &str,
    arg1: u64,
    arg2: u32,
) -> Result<FencedActuatorOutcome, ActuatorBridgeError> {
    match code {
        "A" => Ok(FencedActuatorOutcome::Applied),
        "D" => Ok(FencedActuatorOutcome::AlreadyApplied {
            applied_epoch: arg1,
            applied_attempt_ordinal: arg2,
        }),
        "S" => Ok(FencedActuatorOutcome::Rejected(
            FencedActuatorRejection::StaleEpoch {
                current_epoch: arg1,
            },
        )),
        "F" => Ok(FencedActuatorOutcome::Rejected(
            FencedActuatorRejection::FutureEpoch {
                current_epoch: arg1,
            },
        )),
        "W" => Ok(FencedActuatorOutcome::Rejected(
            FencedActuatorRejection::WrongOwner,
        )),
        "L" => Ok(FencedActuatorOutcome::Rejected(
            FencedActuatorRejection::LeaseExpired {
                current_epoch: arg1,
            },
        )),
        "I" => Ok(FencedActuatorOutcome::Rejected(
            FencedActuatorRejection::IdentityConflict,
        )),
        _ => Err(ActuatorBridgeError::InvalidReceiptOutcome),
    }
}

fn encode_recovery_outcome(outcome: &ActuatorRecoveryOutcome) -> (&'static str, u64, u32) {
    match outcome {
        ActuatorRecoveryOutcome::EffectSucceeded {
            applied_epoch,
            applied_attempt_ordinal,
        } => ("S", *applied_epoch, *applied_attempt_ordinal),
        ActuatorRecoveryOutcome::StillUnknown => ("U", 0, 0),
        ActuatorRecoveryOutcome::IdentityConflict => ("I", 0, 0),
    }
}

fn decode_recovery_outcome(
    code: &str,
    arg1: u64,
    arg2: u32,
) -> Result<ActuatorRecoveryOutcome, ActuatorBridgeError> {
    match code {
        "S" => Ok(ActuatorRecoveryOutcome::EffectSucceeded {
            applied_epoch: arg1,
            applied_attempt_ordinal: arg2,
        }),
        "U" => Ok(ActuatorRecoveryOutcome::StillUnknown),
        "I" => Ok(ActuatorRecoveryOutcome::IdentityConflict),
        _ => Err(ActuatorBridgeError::InvalidRecoveryOutcome),
    }
}

fn encode_string(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_string(value: &str) -> Result<String, ActuatorBridgeError> {
    if !value.len().is_multiple_of(2) {
        return Err(ActuatorBridgeError::InvalidHex);
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().as_chunks::<2>().0 {
        let pair = std::str::from_utf8(chunk).map_err(|_| ActuatorBridgeError::InvalidHex)?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(|_| ActuatorBridgeError::InvalidHex)?);
    }
    String::from_utf8(bytes).map_err(|_| ActuatorBridgeError::InvalidUtf8)
}

fn parse_u64(value: Option<&str>) -> Result<u64, ActuatorBridgeError> {
    value
        .ok_or(ActuatorBridgeError::InvalidRecord)?
        .parse::<u64>()
        .map_err(|_| ActuatorBridgeError::InvalidNumber)
}

fn parse_u32(value: Option<&str>) -> Result<u32, ActuatorBridgeError> {
    value
        .ok_or(ActuatorBridgeError::InvalidRecord)?
        .parse::<u32>()
        .map_err(|_| ActuatorBridgeError::InvalidNumber)
}

fn parse_i64(value: Option<&str>) -> Result<i64, ActuatorBridgeError> {
    value
        .ok_or(ActuatorBridgeError::InvalidRecord)?
        .parse::<i64>()
        .map_err(|_| ActuatorBridgeError::InvalidNumber)
}

fn sync_directory(path: &Path) -> Result<(), ActuatorBridgeError> {
    let directory = File::open(path).map_err(io_error)?;
    directory.sync_all().map_err(io_error)
}

fn io_error(error: std::io::Error) -> ActuatorBridgeError {
    ActuatorBridgeError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use lifetra_bead::{BeadId, BeadScale, TrajectoryBead};

    use crate::{
        AuthorityTicket, ExecutionMode, FencedActuatorController, InMemoryActuatorAuthority,
        InMemoryRecoveryLeaseStore, RetryReason, RetryVerdict,
    };

    use super::*;

    fn temp_path(name: &str, suffix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lifetra-actuator-bridge-{name}-{}-{nonce}.{suffix}",
            std::process::id()
        ))
    }

    fn ticket() -> AuthorityTicket {
        AuthorityTicket {
            action_id: ActionId::new("action:bridge:1").expect("action"),
            source_bead: BeadId::new("bead:bridge:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:bridge".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn decision(journal: &DurableJournal) -> RetryDecision {
        RetryDecision {
            action_id: journal.ticket().action_id.clone(),
            idempotency_key: journal.binding().key.clone(),
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: journal.ticket().authority_proof_refs.clone(),
        }
    }

    fn setup(
        name: &str,
    ) -> (
        PathBuf,
        PathBuf,
        DurableActuatorReceiptBridge,
        FencedDurableJournal<InMemoryRecoveryLeaseStore>,
        InMemoryRecoveryLeaseStore,
    ) {
        let journal_path = temp_path(name, "journal");
        let bridge_path = temp_path(name, "bridge");
        let ticket = ticket();
        let binding = crate::IdempotencyBinding::new(&ticket, "idem:bridge:1").expect("binding");
        let journal = DurableJournal::create(&journal_path, ticket, binding).expect("journal");
        let bridge = DurableActuatorReceiptBridge::create(
            &bridge_path,
            &journal,
            "operation:bridge:payout:1",
        )
        .expect("bridge");
        let store = InMemoryRecoveryLeaseStore::default();
        let fenced = FencedDurableJournal::acquire(
            journal,
            store.clone(),
            RecoveryWorkerId::new("worker-a").expect("worker"),
            Timestamp::new(100),
            20,
        )
        .expect("lease");
        (journal_path, bridge_path, bridge, fenced, store)
    }

    fn cleanup(journal_path: PathBuf, bridge_path: PathBuf) {
        fs::remove_file(journal_path).ok();
        fs::remove_dir_all(bridge_path).ok();
    }

    #[test]
    fn operation_identity_and_permit_are_durable_before_effect() {
        let (journal_path, bridge_path, mut bridge, mut fenced, _) = setup("identity");
        let initial = decision(fenced.journal());
        bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        drop(fenced);
        drop(bridge);

        let bridge = DurableActuatorReceiptBridge::open(&bridge_path).expect("reopen bridge");
        let state = bridge.recover().expect("recover");
        assert_eq!(state.binding.operation_ref, "operation:bridge:payout:1");
        assert_eq!(state.prepared_calls.len(), 1);
        assert_eq!(state.prepared_calls[0].permit.attempt_ordinal, 0);
        cleanup(journal_path, bridge_path);
    }

    #[test]
    fn applied_receipt_closes_prepared_attempt_without_fake_dispatch() {
        let (journal_path, bridge_path, mut bridge, mut fenced, _) = setup("prepared-success");
        let initial = decision(fenced.journal());
        let permit = bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(102)).expect("time");
        actuator
            .install_authority(InMemoryActuatorAuthority {
                action_id: permit.action_id.clone(),
                owner: permit.owner.clone(),
                epoch: permit.fencing_epoch,
                expires_at: Timestamp::new(120),
            })
            .expect("authority");
        let receipt = FencedActuatorController
            .execute(&permit, bridge.binding().operation_ref.clone(), &actuator)
            .expect("actuator");
        bridge.persist_receipt(&receipt).expect("receipt durable");
        assert_eq!(
            bridge
                .project_pending(&mut fenced, Timestamp::new(103))
                .expect("project"),
            1
        );

        let recovered = fenced.journal().recover().expect("journal recovery");
        assert_eq!(recovered.ledger.attempts().len(), 0);
        assert_eq!(
            recovered.directive,
            RecoveryDirective::CloseSucceeded { ordinal: 0 }
        );
        cleanup(journal_path, bridge_path);
    }

    #[test]
    fn applied_receipt_after_local_dispatch_becomes_external_success() {
        let (journal_path, bridge_path, mut bridge, mut fenced, _) = setup("dispatch-success");
        let initial = decision(fenced.journal());
        let permit = bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        fenced
            .record_dispatch(
                &permit,
                Timestamp::new(102),
                "proof:dispatch:bridge",
                Timestamp::new(102),
            )
            .expect("dispatch");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(
                &permit,
                bridge.binding().operation_ref.clone(),
            )
            .expect("request"),
            observed_at: Timestamp::new(103),
            outcome: FencedActuatorOutcome::Applied,
            proof_ref: "proof:actuator:applied".into(),
        };
        bridge.persist_receipt(&receipt).expect("persist");
        bridge
            .project_pending(&mut fenced, Timestamp::new(104))
            .expect("project");
        let recovered = fenced.journal().recover().expect("recover");
        assert_eq!(
            recovered.ledger.attempts()[0]
                .external
                .as_ref()
                .expect("external")
                .outcome,
            ExecutionOutcome::Succeeded
        );
        cleanup(journal_path, bridge_path);
    }

    #[test]
    fn rejected_receipt_does_not_authorize_retry() {
        let (journal_path, bridge_path, mut bridge, mut fenced, _) = setup("rejected");
        let initial = decision(fenced.journal());
        let permit = bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(
                &permit,
                bridge.binding().operation_ref.clone(),
            )
            .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch {
                current_epoch: 2,
            }),
            proof_ref: "proof:actuator:stale".into(),
        };
        bridge.persist_receipt(&receipt).expect("persist");
        bridge
            .project_pending(&mut fenced, Timestamp::new(103))
            .expect("project");
        assert_eq!(
            fenced.journal().recover().expect("recover").directive,
            RecoveryDirective::ReconcilePreparedAttempt { ordinal: 0 }
        );
        cleanup(journal_path, bridge_path);
    }

    #[test]
    fn crash_after_external_apply_reconciles_by_durable_operation_identity() {
        let (journal_path, bridge_path, mut bridge, mut fenced, store) = setup("crash-after-apply");
        let initial = decision(fenced.journal());
        let permit = bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(102)).expect("time");
        actuator
            .install_authority(InMemoryActuatorAuthority {
                action_id: permit.action_id.clone(),
                owner: permit.owner.clone(),
                epoch: permit.fencing_epoch,
                expires_at: Timestamp::new(120),
            })
            .expect("authority");
        let applied = FencedActuatorController
            .execute(&permit, bridge.binding().operation_ref.clone(), &actuator)
            .expect("external apply");
        assert_eq!(applied.outcome, FencedActuatorOutcome::Applied);
        // Crash window: the external effect exists, but no local receipt was persisted.
        drop(fenced);
        drop(bridge);

        actuator.set_time(Timestamp::new(122)).expect("time");
        actuator
            .install_authority(InMemoryActuatorAuthority {
                action_id: permit.action_id.clone(),
                owner: RecoveryWorkerId::new("worker-b").expect("worker b"),
                epoch: 2,
                expires_at: Timestamp::new(150),
            })
            .expect("takeover authority");

        let journal = DurableJournal::open(&journal_path).expect("reopen journal");
        let mut bridge = DurableActuatorReceiptBridge::open(&bridge_path).expect("reopen bridge");
        let observation = bridge
            .reconcile_after_crash(&journal, &actuator, Timestamp::new(123))
            .expect("reconcile external effect");
        assert_eq!(observation.query.operation_ref, "operation:bridge:payout:1");
        assert!(matches!(
            observation.outcome,
            ActuatorRecoveryOutcome::EffectSucceeded {
                applied_epoch: 1,
                applied_attempt_ordinal: 0,
            }
        ));

        let mut fenced = FencedDurableJournal::acquire(
            journal,
            store,
            RecoveryWorkerId::new("worker-b").expect("worker b"),
            Timestamp::new(121),
            30,
        )
        .expect("worker b lease");
        bridge
            .project_pending(&mut fenced, Timestamp::new(123))
            .expect("project recovered proof");
        assert_eq!(
            fenced.journal().recover().expect("recover").directive,
            RecoveryDirective::CloseSucceeded { ordinal: 0 }
        );
        cleanup(journal_path, bridge_path);
    }

    #[test]
    fn durable_success_evidence_can_seed_next_bead() {
        let (journal_path, bridge_path, mut bridge, mut fenced, _) = setup("bead-proof");
        let initial = decision(fenced.journal());
        let permit = bridge
            .prepare(
                &mut fenced,
                &initial,
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(
                &permit,
                bridge.binding().operation_ref.clone(),
            )
            .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::AlreadyApplied {
                applied_epoch: 1,
                applied_attempt_ordinal: 0,
            },
            proof_ref: "proof:actuator:already-applied".into(),
        };
        bridge.persist_receipt(&receipt).expect("persist");
        let state = bridge.recover().expect("bridge state");
        let evidence = state.evidence[0]
            .as_supported_evidence()
            .expect("successful effect is evidence");
        let bead = TrajectoryBead::new(
            BeadId::new("bead:after-effect"),
            BeadScale::Event,
            Timestamp::new(103),
            Timestamp::new(104),
        )
        .with_evidence(evidence);
        assert_eq!(bead.supported_evidence_count(), 1);
        cleanup(journal_path, bridge_path);
    }
}
