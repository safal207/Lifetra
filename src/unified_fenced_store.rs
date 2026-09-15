use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lifetra_bead::{EvidenceRef, EvidenceStatus};
use lifetra_core::Timestamp;

use crate::{
    ActionId, AuthorityTicket, FencedActuatorOutcome, FencedActuatorReceipt,
    FencedActuatorRejection, FencedAttemptPermit, IdempotencyBinding, ReconciliationOutcome,
    RecoveryWorkerId, RetryDecision, RetryVerdict,
};

/// Stable operation identity held in the same transactional record as lease,
/// preparation, external evidence, and projection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedOperationBinding {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub operation_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedLeaseAuthority {
    pub owner: RecoveryWorkerId,
    pub epoch: u64,
    pub revision: u64,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedDispatchEvidence {
    pub dispatched_at: Timestamp,
    pub proof_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedPreparedAttempt {
    pub permit: FencedAttemptPermit,
    pub prepared_at: Timestamp,
    pub authorization_proof_refs: Vec<String>,
    pub dispatch: Option<UnifiedDispatchEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedEffectOutcome {
    EffectSucceeded,
    EffectFailed,
    NoEffectConfirmed,
    StillUnknown,
    IdentityConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedEvidenceSource {
    ActuatorReceipt,
    Reconciliation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedEffectEvidence {
    pub attempt_ordinal: u32,
    pub observed_at: Timestamp,
    pub outcome: UnifiedEffectOutcome,
    pub proof_ref: String,
    pub source: UnifiedEvidenceSource,
}

/// Marker committed atomically with positive effect evidence inside the unified
/// store. It means the proof is eligible to seed a later bead; it does not mutate
/// any already-committed historical bead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedProjectionMarker {
    pub attempt_ordinal: u32,
    pub proof_ref: String,
    pub projected_at: Timestamp,
}

/// One CAS-versioned action record.
///
/// All control/evidence fields below are replaced as one store transaction. This
/// reduces local cross-store gaps, but it does not make an arbitrary external side
/// effect part of the same transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct UnifiedActionRecord {
    pub binding: UnifiedOperationBinding,
    pub revision: u64,
    pub lease: Option<UnifiedLeaseAuthority>,
    pub attempts: Vec<UnifiedPreparedAttempt>,
    pub evidence: Vec<UnifiedEffectEvidence>,
    pub projections: Vec<UnifiedProjectionMarker>,
}

impl UnifiedActionRecord {
    pub fn latest_attempt(&self) -> Option<&UnifiedPreparedAttempt> {
        self.attempts.last()
    }

    pub fn latest_evidence(&self, ordinal: u32) -> Option<&UnifiedEffectEvidence> {
        self.evidence
            .iter()
            .rev()
            .find(|evidence| evidence.attempt_ordinal == ordinal)
    }

    pub fn has_conflicting_evidence(&self) -> bool {
        for left in &self.evidence {
            if !is_terminal(left.outcome) {
                continue;
            }
            if self.evidence.iter().any(|right| {
                right.attempt_ordinal == left.attempt_ordinal
                    && is_terminal(right.outcome)
                    && right.outcome != left.outcome
            }) {
                return true;
            }
        }
        false
    }

    pub fn projected_supported_evidence(&self) -> Vec<EvidenceRef> {
        self.projections
            .iter()
            .filter_map(|projection| {
                self.evidence
                    .iter()
                    .find(|evidence| {
                        evidence.attempt_ordinal == projection.attempt_ordinal
                            && evidence.proof_ref == projection.proof_ref
                            && evidence.outcome == UnifiedEffectOutcome::EffectSucceeded
                    })
                    .map(|evidence| {
                        EvidenceRef::new(
                            evidence.proof_ref.clone(),
                            EvidenceStatus::Supported,
                            "unified fenced store confirmed the external side effect",
                        )
                    })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedFencingToken {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub epoch: u64,
}

impl From<UnifiedFencingToken> for crate::FencingToken {
    fn from(value: UnifiedFencingToken) -> Self {
        Self {
            action_id: value.action_id,
            owner: value.owner,
            epoch: value.epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedReconciliationObservation {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub operation_ref: String,
    pub attempt_ordinal: u32,
    pub observed_at: Timestamp,
    pub outcome: ReconciliationOutcome,
    pub proof_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedEvidenceCommit {
    pub proof_ref: String,
    pub outcome: UnifiedEffectOutcome,
    pub projected: bool,
    pub record_revision: u64,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedRuntimeDirective {
    ReadyForPreparation,
    Reconcile { ordinal: u32 },
    EvaluateRetry { ordinal: u32, proof_ref: String },
    CloseSucceeded { ordinal: u32, proof_ref: String },
    BlockIdentityConflict { proof_ref: String },
    BlockConflictingEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedConfigBlock {
    EmptyIdempotencyKey,
    EmptyOperationRef,
    InvalidTtl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifiedFencedBlock<E> {
    Config(UnifiedConfigBlock),
    Store(E),
    ActionAlreadyExists,
    ActionMissing,
    Contended,
    ActionMismatch,
    IdempotencyMismatch,
    LeaseHeld {
        owner: RecoveryWorkerId,
        epoch: u64,
        expires_at: Timestamp,
    },
    LeaseMissing,
    LeaseExpired {
        epoch: u64,
    },
    StaleFence {
        presented_epoch: u64,
        current_epoch: u64,
    },
    WrongOwner,
    EpochOverflow,
    LeaseRevisionOverflow,
    RecordRevisionOverflow,
    DecisionDoesNotAuthorizePreparation,
    UnexpectedAttemptOrdinal {
        expected: u32,
        received: u32,
    },
    PreviousAttemptNotRetrySafe,
    RetryProofLineageMissing,
    AttemptMissing {
        ordinal: u32,
    },
    PermitMismatch,
    DispatchAlreadyRecorded {
        ordinal: u32,
    },
    EmptyProof,
    ObservationBeforePreparation,
    ReceiptIdentityMismatch,
    ReconciliationIdentityMismatch,
    ConflictingProofIdentity,
}

impl<E> From<UnifiedConfigBlock> for UnifiedFencedBlock<E> {
    fn from(value: UnifiedConfigBlock) -> Self {
        Self::Config(value)
    }
}

/// Storage contract for one action-wide transactional record.
///
/// `compare_and_swap` MUST replace the complete record atomically. Production
/// implementations should use an authoritative store-time or bounded-time
/// mechanism when validating lease expiry; a read-then-write implementation is
/// not sufficient for multi-worker safety.
pub trait UnifiedFencedStore {
    type Error;

    fn load(&self, action_id: &ActionId) -> Result<Option<UnifiedActionRecord>, Self::Error>;

    fn compare_and_swap(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: UnifiedActionRecord,
    ) -> Result<bool, Self::Error>;
}

#[derive(Debug, Clone)]
pub struct UnifiedFencedRuntime<S> {
    store: S,
}

impl<S: UnifiedFencedStore> UnifiedFencedRuntime<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn create_action(
        &self,
        ticket: &AuthorityTicket,
        binding: &IdempotencyBinding,
        operation_ref: impl Into<String>,
    ) -> Result<UnifiedActionRecord, UnifiedFencedBlock<S::Error>> {
        if binding.action_id != ticket.action_id {
            return Err(UnifiedFencedBlock::ActionMismatch);
        }
        if binding.key.trim().is_empty() {
            return Err(UnifiedConfigBlock::EmptyIdempotencyKey.into());
        }
        let operation_ref = operation_ref.into();
        if operation_ref.trim().is_empty() {
            return Err(UnifiedConfigBlock::EmptyOperationRef.into());
        }

        let record = UnifiedActionRecord {
            binding: UnifiedOperationBinding {
                action_id: ticket.action_id.clone(),
                idempotency_key: binding.key.clone(),
                operation_ref,
            },
            revision: 0,
            lease: None,
            attempts: Vec::new(),
            evidence: Vec::new(),
            projections: Vec::new(),
        };
        let swapped = self
            .store
            .compare_and_swap(&ticket.action_id, None, record.clone())
            .map_err(UnifiedFencedBlock::Store)?;
        if !swapped {
            return Err(UnifiedFencedBlock::ActionAlreadyExists);
        }
        Ok(record)
    }

    pub fn load(
        &self,
        action_id: &ActionId,
    ) -> Result<UnifiedActionRecord, UnifiedFencedBlock<S::Error>> {
        self.store
            .load(action_id)
            .map_err(UnifiedFencedBlock::Store)?
            .ok_or(UnifiedFencedBlock::ActionMissing)
    }

    pub fn acquire(
        &self,
        action_id: &ActionId,
        owner: RecoveryWorkerId,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<UnifiedFencingToken, UnifiedFencedBlock<S::Error>> {
        let expires_at = unified_expiry(now, ttl_seconds)?;
        let mut record = self.load(action_id)?;
        let expected_revision = record.revision;
        let epoch = match &record.lease {
            None => 1,
            Some(lease) => {
                if lease.expires_at > now {
                    return Err(UnifiedFencedBlock::LeaseHeld {
                        owner: lease.owner.clone(),
                        epoch: lease.epoch,
                        expires_at: lease.expires_at,
                    });
                }
                lease
                    .epoch
                    .checked_add(1)
                    .ok_or(UnifiedFencedBlock::EpochOverflow)?
            }
        };
        record.lease = Some(UnifiedLeaseAuthority {
            owner: owner.clone(),
            epoch,
            revision: 0,
            expires_at,
        });
        record.revision = next_record_revision(record.revision)?;
        self.swap(action_id, expected_revision, record)?;
        Ok(UnifiedFencingToken {
            action_id: action_id.clone(),
            owner,
            epoch,
        })
    }

    pub fn renew(
        &self,
        token: &UnifiedFencingToken,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<UnifiedLeaseAuthority, UnifiedFencedBlock<S::Error>> {
        let expires_at = unified_expiry(now, ttl_seconds)?;
        let mut record = self.load(&token.action_id)?;
        let expected_revision = record.revision;
        let current = current_lease(&record, token, now)?.clone();
        let lease_revision = current
            .revision
            .checked_add(1)
            .ok_or(UnifiedFencedBlock::LeaseRevisionOverflow)?;
        let replacement = UnifiedLeaseAuthority {
            owner: current.owner,
            epoch: current.epoch,
            revision: lease_revision,
            expires_at,
        };
        record.lease = Some(replacement.clone());
        record.revision = next_record_revision(record.revision)?;
        self.swap(&token.action_id, expected_revision, record)?;
        Ok(replacement)
    }

    pub fn prepare_attempt(
        &self,
        token: &UnifiedFencingToken,
        decision: &RetryDecision,
        prepared_at: Timestamp,
        now: Timestamp,
    ) -> Result<FencedAttemptPermit, UnifiedFencedBlock<S::Error>> {
        let mut record = self.load(&token.action_id)?;
        let expected_revision = record.revision;
        current_lease(&record, token, now)?;
        validate_decision_identity(&record, decision)?;

        let ordinal = match decision.verdict {
            RetryVerdict::InitialDispatchAllowed => 0,
            RetryVerdict::RedispatchAllowed { ordinal } => ordinal,
            RetryVerdict::ReconcileFirst
            | RetryVerdict::CloseSucceeded
            | RetryVerdict::CloseFailed
            | RetryVerdict::Block => {
                return Err(UnifiedFencedBlock::DecisionDoesNotAuthorizePreparation)
            }
        };
        let expected = record.attempts.last().map_or(0, |attempt| {
            attempt.permit.attempt_ordinal.saturating_add(1)
        });
        if ordinal != expected {
            return Err(UnifiedFencedBlock::UnexpectedAttemptOrdinal {
                expected,
                received: ordinal,
            });
        }
        if ordinal > 0 {
            let previous_ordinal = ordinal - 1;
            let evidence = record
                .latest_evidence(previous_ordinal)
                .ok_or(UnifiedFencedBlock::PreviousAttemptNotRetrySafe)?;
            if !matches!(
                evidence.outcome,
                UnifiedEffectOutcome::EffectFailed | UnifiedEffectOutcome::NoEffectConfirmed
            ) {
                return Err(UnifiedFencedBlock::PreviousAttemptNotRetrySafe);
            }
            if !decision
                .proof_refs
                .iter()
                .any(|proof| proof == &evidence.proof_ref)
            {
                return Err(UnifiedFencedBlock::RetryProofLineageMissing);
            }
        }

        let permit = FencedAttemptPermit {
            action_id: token.action_id.clone(),
            owner: token.owner.clone(),
            fencing_epoch: token.epoch,
            attempt_ordinal: ordinal,
            idempotency_key: record.binding.idempotency_key.clone(),
        };
        record.attempts.push(UnifiedPreparedAttempt {
            permit: permit.clone(),
            prepared_at,
            authorization_proof_refs: decision.proof_refs.clone(),
            dispatch: None,
        });
        record.revision = next_record_revision(record.revision)?;
        self.swap(&token.action_id, expected_revision, record)?;
        Ok(permit)
    }

    pub fn record_dispatch(
        &self,
        token: &UnifiedFencingToken,
        permit: &FencedAttemptPermit,
        dispatched_at: Timestamp,
        proof_ref: impl Into<String>,
        now: Timestamp,
    ) -> Result<(), UnifiedFencedBlock<S::Error>> {
        let proof_ref = non_empty_unified_proof(proof_ref)?;
        let mut record = self.load(&token.action_id)?;
        let expected_revision = record.revision;
        current_lease(&record, token, now)?;
        if permit.action_id != token.action_id
            || permit.owner != token.owner
            || permit.fencing_epoch != token.epoch
            || permit.idempotency_key != record.binding.idempotency_key
        {
            return Err(UnifiedFencedBlock::PermitMismatch);
        }
        let attempt = record
            .attempts
            .iter_mut()
            .find(|attempt| attempt.permit.attempt_ordinal == permit.attempt_ordinal)
            .ok_or(UnifiedFencedBlock::AttemptMissing {
                ordinal: permit.attempt_ordinal,
            })?;
        if attempt.permit != *permit {
            return Err(UnifiedFencedBlock::PermitMismatch);
        }
        if attempt.dispatch.is_some() {
            return Err(UnifiedFencedBlock::DispatchAlreadyRecorded {
                ordinal: permit.attempt_ordinal,
            });
        }
        if dispatched_at < attempt.prepared_at {
            return Err(UnifiedFencedBlock::ObservationBeforePreparation);
        }
        attempt.dispatch = Some(UnifiedDispatchEvidence {
            dispatched_at,
            proof_ref,
        });
        record.revision = next_record_revision(record.revision)?;
        self.swap(&token.action_id, expected_revision, record)
    }

    /// Admits externally produced evidence without requiring the producing worker
    /// to still own the current lease. Evidence about a past valid epoch is not
    /// execution authority for a future action.
    pub fn record_actuator_receipt(
        &self,
        receipt: &FencedActuatorReceipt,
    ) -> Result<UnifiedEvidenceCommit, UnifiedFencedBlock<S::Error>> {
        let proof_ref = non_empty_unified_proof(receipt.proof_ref.clone())?;
        let mut record = self.load(&receipt.request.action_id)?;
        let expected_revision = record.revision;
        validate_receipt_identity(&record, receipt)?;
        let outcome = match receipt.outcome {
            FencedActuatorOutcome::Applied | FencedActuatorOutcome::AlreadyApplied { .. } => {
                UnifiedEffectOutcome::EffectSucceeded
            }
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict) => {
                UnifiedEffectOutcome::IdentityConflict
            }
            FencedActuatorOutcome::Rejected(_) => UnifiedEffectOutcome::StillUnknown,
        };
        self.commit_evidence(
            &mut record,
            expected_revision,
            UnifiedEffectEvidence {
                attempt_ordinal: receipt.request.attempt_ordinal,
                observed_at: receipt.observed_at,
                outcome,
                proof_ref,
                source: UnifiedEvidenceSource::ActuatorReceipt,
            },
        )
    }

    pub fn record_reconciliation(
        &self,
        observation: &UnifiedReconciliationObservation,
    ) -> Result<UnifiedEvidenceCommit, UnifiedFencedBlock<S::Error>> {
        let proof_ref = non_empty_unified_proof(observation.proof_ref.clone())?;
        let mut record = self.load(&observation.action_id)?;
        let expected_revision = record.revision;
        validate_reconciliation_identity(&record, observation)?;
        let outcome = match observation.outcome {
            ReconciliationOutcome::EffectSucceeded => UnifiedEffectOutcome::EffectSucceeded,
            ReconciliationOutcome::EffectFailed => UnifiedEffectOutcome::EffectFailed,
            ReconciliationOutcome::NoEffectConfirmed => UnifiedEffectOutcome::NoEffectConfirmed,
            ReconciliationOutcome::StillUnknown => UnifiedEffectOutcome::StillUnknown,
        };
        self.commit_evidence(
            &mut record,
            expected_revision,
            UnifiedEffectEvidence {
                attempt_ordinal: observation.attempt_ordinal,
                observed_at: observation.observed_at,
                outcome,
                proof_ref,
                source: UnifiedEvidenceSource::Reconciliation,
            },
        )
    }

    pub fn directive(
        &self,
        action_id: &ActionId,
    ) -> Result<UnifiedRuntimeDirective, UnifiedFencedBlock<S::Error>> {
        let record = self.load(action_id)?;
        if let Some(conflict) = record
            .evidence
            .iter()
            .rev()
            .find(|evidence| evidence.outcome == UnifiedEffectOutcome::IdentityConflict)
        {
            return Ok(UnifiedRuntimeDirective::BlockIdentityConflict {
                proof_ref: conflict.proof_ref.clone(),
            });
        }
        if record.has_conflicting_evidence() {
            return Ok(UnifiedRuntimeDirective::BlockConflictingEvidence);
        }
        let Some(latest) = record.latest_attempt() else {
            return Ok(UnifiedRuntimeDirective::ReadyForPreparation);
        };
        let ordinal = latest.permit.attempt_ordinal;
        let Some(evidence) = record.latest_evidence(ordinal) else {
            return Ok(UnifiedRuntimeDirective::Reconcile { ordinal });
        };
        match evidence.outcome {
            UnifiedEffectOutcome::EffectSucceeded => Ok(UnifiedRuntimeDirective::CloseSucceeded {
                ordinal,
                proof_ref: evidence.proof_ref.clone(),
            }),
            UnifiedEffectOutcome::EffectFailed | UnifiedEffectOutcome::NoEffectConfirmed => {
                Ok(UnifiedRuntimeDirective::EvaluateRetry {
                    ordinal,
                    proof_ref: evidence.proof_ref.clone(),
                })
            }
            UnifiedEffectOutcome::StillUnknown => {
                Ok(UnifiedRuntimeDirective::Reconcile { ordinal })
            }
            UnifiedEffectOutcome::IdentityConflict => {
                Ok(UnifiedRuntimeDirective::BlockIdentityConflict {
                    proof_ref: evidence.proof_ref.clone(),
                })
            }
        }
    }

    pub fn projected_supported_evidence(
        &self,
        action_id: &ActionId,
    ) -> Result<Vec<EvidenceRef>, UnifiedFencedBlock<S::Error>> {
        Ok(self.load(action_id)?.projected_supported_evidence())
    }

    fn commit_evidence(
        &self,
        record: &mut UnifiedActionRecord,
        expected_revision: u64,
        evidence: UnifiedEffectEvidence,
    ) -> Result<UnifiedEvidenceCommit, UnifiedFencedBlock<S::Error>> {
        let attempt = record
            .attempts
            .iter()
            .find(|attempt| attempt.permit.attempt_ordinal == evidence.attempt_ordinal)
            .ok_or(UnifiedFencedBlock::AttemptMissing {
                ordinal: evidence.attempt_ordinal,
            })?;
        if evidence.observed_at < attempt.prepared_at {
            return Err(UnifiedFencedBlock::ObservationBeforePreparation);
        }
        if let Some(existing) = record
            .evidence
            .iter()
            .find(|existing| existing.proof_ref == evidence.proof_ref)
        {
            if existing == &evidence {
                let projected = record.projections.iter().any(|projection| {
                    projection.proof_ref == evidence.proof_ref
                        && projection.attempt_ordinal == evidence.attempt_ordinal
                });
                return Ok(UnifiedEvidenceCommit {
                    proof_ref: evidence.proof_ref,
                    outcome: evidence.outcome,
                    projected,
                    record_revision: record.revision,
                    duplicate: true,
                });
            }
            return Err(UnifiedFencedBlock::ConflictingProofIdentity);
        }

        let projected = evidence.outcome == UnifiedEffectOutcome::EffectSucceeded;
        if projected {
            record.projections.push(UnifiedProjectionMarker {
                attempt_ordinal: evidence.attempt_ordinal,
                proof_ref: evidence.proof_ref.clone(),
                projected_at: evidence.observed_at,
            });
        }
        record.evidence.push(evidence.clone());
        record.revision = next_record_revision(record.revision)?;
        let revision = record.revision;
        self.swap(&record.binding.action_id, expected_revision, record.clone())?;
        Ok(UnifiedEvidenceCommit {
            proof_ref: evidence.proof_ref,
            outcome: evidence.outcome,
            projected,
            record_revision: revision,
            duplicate: false,
        })
    }

    fn swap(
        &self,
        action_id: &ActionId,
        expected_revision: u64,
        replacement: UnifiedActionRecord,
    ) -> Result<(), UnifiedFencedBlock<S::Error>> {
        let swapped = self
            .store
            .compare_and_swap(action_id, Some(expected_revision), replacement)
            .map_err(UnifiedFencedBlock::Store)?;
        if !swapped {
            return Err(UnifiedFencedBlock::Contended);
        }
        Ok(())
    }
}

fn current_lease<'a, E>(
    record: &'a UnifiedActionRecord,
    token: &UnifiedFencingToken,
    now: Timestamp,
) -> Result<&'a UnifiedLeaseAuthority, UnifiedFencedBlock<E>> {
    if record.binding.action_id != token.action_id {
        return Err(UnifiedFencedBlock::ActionMismatch);
    }
    let lease = record
        .lease
        .as_ref()
        .ok_or(UnifiedFencedBlock::LeaseMissing)?;
    if lease.epoch != token.epoch {
        return Err(UnifiedFencedBlock::StaleFence {
            presented_epoch: token.epoch,
            current_epoch: lease.epoch,
        });
    }
    if lease.owner != token.owner {
        return Err(UnifiedFencedBlock::WrongOwner);
    }
    if lease.expires_at <= now {
        return Err(UnifiedFencedBlock::LeaseExpired { epoch: lease.epoch });
    }
    Ok(lease)
}

fn validate_decision_identity<E>(
    record: &UnifiedActionRecord,
    decision: &RetryDecision,
) -> Result<(), UnifiedFencedBlock<E>> {
    if decision.action_id != record.binding.action_id {
        return Err(UnifiedFencedBlock::ActionMismatch);
    }
    if decision.idempotency_key != record.binding.idempotency_key {
        return Err(UnifiedFencedBlock::IdempotencyMismatch);
    }
    Ok(())
}

fn validate_receipt_identity<E>(
    record: &UnifiedActionRecord,
    receipt: &FencedActuatorReceipt,
) -> Result<(), UnifiedFencedBlock<E>> {
    if receipt.request.action_id != record.binding.action_id
        || receipt.request.idempotency_key != record.binding.idempotency_key
        || receipt.request.operation_ref != record.binding.operation_ref
    {
        return Err(UnifiedFencedBlock::ReceiptIdentityMismatch);
    }
    let attempt = record
        .attempts
        .iter()
        .find(|attempt| attempt.permit.attempt_ordinal == receipt.request.attempt_ordinal)
        .ok_or(UnifiedFencedBlock::AttemptMissing {
            ordinal: receipt.request.attempt_ordinal,
        })?;
    if attempt.permit.action_id != receipt.request.action_id
        || attempt.permit.owner != receipt.request.owner
        || attempt.permit.fencing_epoch != receipt.request.fencing_epoch
        || attempt.permit.idempotency_key != receipt.request.idempotency_key
    {
        return Err(UnifiedFencedBlock::ReceiptIdentityMismatch);
    }
    if receipt.observed_at < attempt.prepared_at {
        return Err(UnifiedFencedBlock::ObservationBeforePreparation);
    }
    Ok(())
}

fn validate_reconciliation_identity<E>(
    record: &UnifiedActionRecord,
    observation: &UnifiedReconciliationObservation,
) -> Result<(), UnifiedFencedBlock<E>> {
    if observation.action_id != record.binding.action_id
        || observation.idempotency_key != record.binding.idempotency_key
        || observation.operation_ref != record.binding.operation_ref
    {
        return Err(UnifiedFencedBlock::ReconciliationIdentityMismatch);
    }
    let attempt = record
        .attempts
        .iter()
        .find(|attempt| attempt.permit.attempt_ordinal == observation.attempt_ordinal)
        .ok_or(UnifiedFencedBlock::AttemptMissing {
            ordinal: observation.attempt_ordinal,
        })?;
    if observation.observed_at < attempt.prepared_at {
        return Err(UnifiedFencedBlock::ObservationBeforePreparation);
    }
    Ok(())
}

fn non_empty_unified_proof<E>(
    proof_ref: impl Into<String>,
) -> Result<String, UnifiedFencedBlock<E>> {
    let proof_ref = proof_ref.into();
    if proof_ref.trim().is_empty() {
        return Err(UnifiedFencedBlock::EmptyProof);
    }
    Ok(proof_ref)
}

fn unified_expiry<E>(now: Timestamp, ttl_seconds: i64) -> Result<Timestamp, UnifiedFencedBlock<E>> {
    if ttl_seconds <= 0 {
        return Err(UnifiedConfigBlock::InvalidTtl.into());
    }
    let expires_at = now
        .epoch_seconds()
        .checked_add(ttl_seconds)
        .ok_or(UnifiedConfigBlock::InvalidTtl)?;
    Ok(Timestamp::new(expires_at))
}

fn next_record_revision<E>(current: u64) -> Result<u64, UnifiedFencedBlock<E>> {
    current
        .checked_add(1)
        .ok_or(UnifiedFencedBlock::RecordRevisionOverflow)
}

fn is_terminal(outcome: UnifiedEffectOutcome) -> bool {
    matches!(
        outcome,
        UnifiedEffectOutcome::EffectSucceeded
            | UnifiedEffectOutcome::EffectFailed
            | UnifiedEffectOutcome::NoEffectConfirmed
            | UnifiedEffectOutcome::IdentityConflict
    )
}

/// Process-local atomic reference implementation.
///
/// All fields for one action are compared and replaced under one mutex. This is
/// intentionally a test/example store, not a distributed production backend.
#[derive(Debug, Clone, Default)]
pub struct InMemoryUnifiedFencedStore {
    inner: Arc<Mutex<HashMap<String, UnifiedActionRecord>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InMemoryUnifiedStoreError {
    Poisoned,
}

impl UnifiedFencedStore for InMemoryUnifiedFencedStore {
    type Error = InMemoryUnifiedStoreError;

    fn load(&self, action_id: &ActionId) -> Result<Option<UnifiedActionRecord>, Self::Error> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| InMemoryUnifiedStoreError::Poisoned)?;
        Ok(guard.get(action_id.as_str()).cloned())
    }

    fn compare_and_swap(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: UnifiedActionRecord,
    ) -> Result<bool, Self::Error> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| InMemoryUnifiedStoreError::Poisoned)?;
        let current_revision = guard.get(action_id.as_str()).map(|record| record.revision);
        if current_revision != expected_revision {
            return Ok(false);
        }
        guard.insert(action_id.as_str().to_owned(), replacement);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use lifetra_bead::{BeadId, BeadScale, TrajectoryBead};

    use crate::{
        ExecutionMode, FencedActuatorController, FencedActuatorRequest, InMemoryActuatorAuthority,
        InMemoryFencedActuator, RetryReason,
    };

    use super::*;

    fn ticket() -> AuthorityTicket {
        AuthorityTicket {
            action_id: ActionId::new("action:unified:1").expect("action"),
            source_bead: BeadId::new("bead:unified:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:unified".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn binding(ticket: &AuthorityTicket) -> IdempotencyBinding {
        IdempotencyBinding::new(ticket, "idem:unified:1").expect("binding")
    }

    fn worker(value: &str) -> RecoveryWorkerId {
        RecoveryWorkerId::new(value).expect("worker")
    }

    fn initial_decision(ticket: &AuthorityTicket) -> RetryDecision {
        RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: "idem:unified:1".into(),
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: ticket.authority_proof_refs.clone(),
        }
    }

    fn runtime() -> (
        UnifiedFencedRuntime<InMemoryUnifiedFencedStore>,
        AuthorityTicket,
    ) {
        let store = InMemoryUnifiedFencedStore::default();
        let runtime = UnifiedFencedRuntime::new(store);
        let ticket = ticket();
        runtime
            .create_action(&ticket, &binding(&ticket), "operation:unified:payout:1")
            .expect("create action");
        (runtime, ticket)
    }

    #[test]
    fn create_binds_operation_identity_once() {
        let (runtime, ticket) = runtime();
        let record = runtime.load(&ticket.action_id).expect("record");
        assert_eq!(record.binding.idempotency_key, "idem:unified:1");
        assert_eq!(record.binding.operation_ref, "operation:unified:payout:1");
        assert_eq!(record.revision, 0);
        assert_eq!(
            runtime.create_action(&ticket, &binding(&ticket), "operation:other"),
            Err(UnifiedFencedBlock::ActionAlreadyExists)
        );
    }

    #[test]
    fn takeover_advances_epoch_and_stales_old_worker() {
        let (runtime, ticket) = runtime();
        let stale = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                10,
            )
            .expect("worker a");
        let current = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-b"),
                Timestamp::new(111),
                20,
            )
            .expect("worker b");
        assert_eq!(stale.epoch, 1);
        assert_eq!(current.epoch, 2);
        assert_eq!(
            runtime.prepare_attempt(
                &stale,
                &initial_decision(&ticket),
                Timestamp::new(112),
                Timestamp::new(112),
            ),
            Err(UnifiedFencedBlock::StaleFence {
                presented_epoch: 1,
                current_epoch: 2,
            })
        );
    }

    #[test]
    fn success_receipt_and_projection_commit_in_same_revision() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let before = runtime.load(&ticket.action_id).expect("before").revision;
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(&permit, "operation:unified:payout:1")
                .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::Applied,
            proof_ref: "proof:unified:applied".into(),
        };
        let commit = runtime
            .record_actuator_receipt(&receipt)
            .expect("commit evidence");
        let record = runtime.load(&ticket.action_id).expect("after");
        assert!(commit.projected);
        assert_eq!(commit.record_revision, before + 1);
        assert_eq!(record.evidence.len(), 1);
        assert_eq!(record.projections.len(), 1);
        assert_eq!(
            record.evidence[0].proof_ref,
            record.projections[0].proof_ref
        );
    }

    #[test]
    fn late_valid_receipt_from_prior_epoch_is_still_admitted_as_evidence() {
        let (runtime, ticket) = runtime();
        let stale = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                10,
            )
            .expect("worker a");
        let permit = runtime
            .prepare_attempt(
                &stale,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        runtime
            .acquire(
                &ticket.action_id,
                worker("worker-b"),
                Timestamp::new(111),
                30,
            )
            .expect("worker b takeover");

        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(&permit, "operation:unified:payout:1")
                .expect("request"),
            observed_at: Timestamp::new(109),
            outcome: FencedActuatorOutcome::Applied,
            proof_ref: "proof:unified:late-success".into(),
        };
        runtime
            .record_actuator_receipt(&receipt)
            .expect("past evidence remains admissible");
        assert_eq!(
            runtime.directive(&ticket.action_id).expect("directive"),
            UnifiedRuntimeDirective::CloseSucceeded {
                ordinal: 0,
                proof_ref: "proof:unified:late-success".into(),
            }
        );
    }

    #[test]
    fn rejected_receipt_remains_reconcile_first() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(&permit, "operation:unified:payout:1")
                .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch {
                current_epoch: 2,
            }),
            proof_ref: "proof:unified:rejected".into(),
        };
        let commit = runtime
            .record_actuator_receipt(&receipt)
            .expect("record rejection");
        assert!(!commit.projected);
        assert_eq!(
            runtime.directive(&ticket.action_id).expect("directive"),
            UnifiedRuntimeDirective::Reconcile { ordinal: 0 }
        );
    }

    #[test]
    fn no_effect_reconciliation_requires_proof_lineage_for_retry() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                40,
            )
            .expect("lease");
        runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare zero");
        let observation = UnifiedReconciliationObservation {
            action_id: ticket.action_id.clone(),
            idempotency_key: "idem:unified:1".into(),
            operation_ref: "operation:unified:payout:1".into(),
            attempt_ordinal: 0,
            observed_at: Timestamp::new(102),
            outcome: ReconciliationOutcome::NoEffectConfirmed,
            proof_ref: "proof:unified:no-effect".into(),
        };
        runtime
            .record_reconciliation(&observation)
            .expect("no effect proof");
        let missing_lineage = RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: "idem:unified:1".into(),
            verdict: RetryVerdict::RedispatchAllowed { ordinal: 1 },
            reasons: Vec::new(),
            proof_refs: Vec::new(),
        };
        assert_eq!(
            runtime.prepare_attempt(
                &token,
                &missing_lineage,
                Timestamp::new(103),
                Timestamp::new(103),
            ),
            Err(UnifiedFencedBlock::RetryProofLineageMissing)
        );

        let retry = RetryDecision {
            proof_refs: vec!["proof:unified:no-effect".into()],
            ..missing_lineage
        };
        let permit = runtime
            .prepare_attempt(&token, &retry, Timestamp::new(103), Timestamp::new(103))
            .expect("retry with lineage");
        assert_eq!(permit.attempt_ordinal, 1);
    }

    #[test]
    fn identity_conflict_is_fail_closed() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(&permit, "operation:unified:payout:1")
                .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict),
            proof_ref: "proof:unified:identity-conflict".into(),
        };
        runtime
            .record_actuator_receipt(&receipt)
            .expect("record conflict");
        assert_eq!(
            runtime.directive(&ticket.action_id).expect("directive"),
            UnifiedRuntimeDirective::BlockIdentityConflict {
                proof_ref: "proof:unified:identity-conflict".into(),
            }
        );
    }

    #[test]
    fn projected_success_can_seed_next_bead() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(&permit, "operation:unified:payout:1")
                .expect("request"),
            observed_at: Timestamp::new(102),
            outcome: FencedActuatorOutcome::Applied,
            proof_ref: "proof:unified:bead".into(),
        };
        runtime
            .record_actuator_receipt(&receipt)
            .expect("record success");
        let evidence = runtime
            .projected_supported_evidence(&ticket.action_id)
            .expect("evidence");
        let bead = TrajectoryBead::new(
            BeadId::new("bead:unified:after"),
            BeadScale::Event,
            Timestamp::new(103),
            Timestamp::new(104),
        )
        .with_evidence(evidence[0].clone());
        assert_eq!(bead.supported_evidence_count(), 1);
    }

    #[test]
    fn direct_store_cas_makes_contention_visible() {
        let (runtime, ticket) = runtime();
        let store = runtime.store().clone();
        let record = runtime.load(&ticket.action_id).expect("record");
        let mut replacement = record.clone();
        replacement.revision = 1;
        assert!(!store
            .compare_and_swap(&ticket.action_id, Some(99), replacement)
            .expect("cas"));
    }

    #[test]
    fn reference_actuator_can_feed_unified_store_without_duplicate_effect() {
        let (runtime, ticket) = runtime();
        let token = runtime
            .acquire(
                &ticket.action_id,
                worker("worker-a"),
                Timestamp::new(100),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(
                &token,
                &initial_decision(&ticket),
                Timestamp::new(101),
                Timestamp::new(101),
            )
            .expect("prepare");
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(102)).expect("time");
        actuator
            .install_authority(InMemoryActuatorAuthority {
                action_id: ticket.action_id.clone(),
                owner: permit.owner.clone(),
                epoch: permit.fencing_epoch,
                expires_at: Timestamp::new(130),
            })
            .expect("authority");
        let receipt = FencedActuatorController
            .execute(&permit, "operation:unified:payout:1", &actuator)
            .expect("apply");
        runtime
            .record_actuator_receipt(&receipt)
            .expect("store receipt");
        let duplicate = FencedActuatorController
            .execute(&permit, "operation:unified:payout:1", &actuator)
            .expect("duplicate observation");
        assert!(matches!(
            duplicate.outcome,
            FencedActuatorOutcome::AlreadyApplied { .. }
        ));
        runtime
            .record_actuator_receipt(&duplicate)
            .expect("store duplicate proof");
        assert_eq!(actuator.applied_count().expect("count"), 1);
    }
}
