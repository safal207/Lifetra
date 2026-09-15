use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lifetra_core::Timestamp;

use crate::{
    ActionId, DurableJournal, ExecutionOutcome, JournalError, ReconciliationOutcome, RetryDecision,
    RetryVerdict,
};

/// Stable identity for one recovery worker competing for execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecoveryWorkerId(String);

impl RecoveryWorkerId {
    pub fn new(value: impl Into<String>) -> Result<Self, LeaseConfigBlock> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(LeaseConfigBlock::EmptyWorkerId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Version used by an atomic lease compare-and-swap operation.
///
/// `epoch` changes whenever ownership is reacquired after expiry. `revision`
/// changes when the current owner renews the same epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeaseVersion {
    pub epoch: u64,
    pub revision: u64,
}

/// Current authoritative lease record held by the lease store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryLease {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub version: LeaseVersion,
    pub expires_at: Timestamp,
}

/// Monotonic execution authority carried by a worker.
///
/// The epoch, not wall-clock recency, is the fencing identity. A worker holding
/// epoch N must be rejected after another worker acquires epoch N+1.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FencingToken {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub epoch: u64,
}

/// Permit for one physical dispatch under a currently valid fencing epoch.
///
/// The permit is not evidence that dispatch happened. A downstream actuator or
/// provider should propagate/enforce `fencing_epoch` when it has a compatible
/// conditional-write or fencing mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedAttemptPermit {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub fencing_epoch: u64,
    pub attempt_ordinal: u32,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseConfigBlock {
    EmptyWorkerId,
    InvalidTtl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryLeaseBlock<E> {
    Config(LeaseConfigBlock),
    Store(E),
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
    ActionMismatch,
    LeaseContended,
    EpochOverflow,
    RevisionOverflow,
    DecisionDoesNotAuthorizeDispatch,
}

impl<E> From<LeaseConfigBlock> for RecoveryLeaseBlock<E> {
    fn from(value: LeaseConfigBlock) -> Self {
        Self::Config(value)
    }
}

/// Store contract for recovery leases.
///
/// `compare_and_swap` MUST be atomic across all workers sharing the store. A
/// database row with conditional update, Redis transaction/script, etcd compare
/// transaction, or equivalent primitive is appropriate. A plain read-then-write
/// implementation is not sufficient for multi-worker fencing.
pub trait RecoveryLeaseStore {
    type Error;

    fn load(&self, action_id: &ActionId) -> Result<Option<RecoveryLease>, Self::Error>;

    fn compare_and_swap(
        &self,
        action_id: &ActionId,
        expected: Option<LeaseVersion>,
        replacement: RecoveryLease,
    ) -> Result<bool, Self::Error>;
}

/// Lease authority over a CAS-capable store.
#[derive(Debug, Clone)]
pub struct RecoveryLeaseManager<S> {
    store: S,
}

impl<S: RecoveryLeaseStore> RecoveryLeaseManager<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn acquire(
        &self,
        action_id: &ActionId,
        owner: RecoveryWorkerId,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<FencingToken, RecoveryLeaseBlock<S::Error>> {
        let expires_at = expiry_from(now, ttl_seconds)?;
        let current = self
            .store
            .load(action_id)
            .map_err(RecoveryLeaseBlock::Store)?;

        let (expected, epoch) = match current {
            None => (None, 1),
            Some(lease) => {
                if lease.action_id != *action_id {
                    return Err(RecoveryLeaseBlock::ActionMismatch);
                }
                if lease.expires_at > now {
                    return Err(RecoveryLeaseBlock::LeaseHeld {
                        owner: lease.owner,
                        epoch: lease.version.epoch,
                        expires_at: lease.expires_at,
                    });
                }
                let next_epoch = lease
                    .version
                    .epoch
                    .checked_add(1)
                    .ok_or(RecoveryLeaseBlock::EpochOverflow)?;
                (Some(lease.version), next_epoch)
            }
        };

        let replacement = RecoveryLease {
            action_id: action_id.clone(),
            owner: owner.clone(),
            version: LeaseVersion { epoch, revision: 0 },
            expires_at,
        };
        let swapped = self
            .store
            .compare_and_swap(action_id, expected, replacement)
            .map_err(RecoveryLeaseBlock::Store)?;
        if !swapped {
            return Err(RecoveryLeaseBlock::LeaseContended);
        }

        Ok(FencingToken {
            action_id: action_id.clone(),
            owner,
            epoch,
        })
    }

    pub fn renew(
        &self,
        token: &FencingToken,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<RecoveryLease, RecoveryLeaseBlock<S::Error>> {
        let expires_at = expiry_from(now, ttl_seconds)?;
        let current = self.current_for_token(token, now)?;
        let revision = current
            .version
            .revision
            .checked_add(1)
            .ok_or(RecoveryLeaseBlock::RevisionOverflow)?;
        let replacement = RecoveryLease {
            action_id: current.action_id.clone(),
            owner: current.owner.clone(),
            version: LeaseVersion {
                epoch: current.version.epoch,
                revision,
            },
            expires_at,
        };
        let swapped = self
            .store
            .compare_and_swap(&token.action_id, Some(current.version), replacement.clone())
            .map_err(RecoveryLeaseBlock::Store)?;
        if !swapped {
            return Err(RecoveryLeaseBlock::LeaseContended);
        }
        Ok(replacement)
    }

    pub fn assert_current(
        &self,
        token: &FencingToken,
        now: Timestamp,
    ) -> Result<RecoveryLease, RecoveryLeaseBlock<S::Error>> {
        self.current_for_token(token, now)
    }

    pub fn authorize_attempt(
        &self,
        token: &FencingToken,
        decision: &RetryDecision,
        now: Timestamp,
    ) -> Result<FencedAttemptPermit, RecoveryLeaseBlock<S::Error>> {
        self.current_for_token(token, now)?;
        if decision.action_id != token.action_id {
            return Err(RecoveryLeaseBlock::ActionMismatch);
        }
        let attempt_ordinal = match decision.verdict {
            RetryVerdict::InitialDispatchAllowed => 0,
            RetryVerdict::RedispatchAllowed { ordinal } => ordinal,
            RetryVerdict::ReconcileFirst
            | RetryVerdict::CloseSucceeded
            | RetryVerdict::CloseFailed
            | RetryVerdict::Block => {
                return Err(RecoveryLeaseBlock::DecisionDoesNotAuthorizeDispatch)
            }
        };

        Ok(FencedAttemptPermit {
            action_id: token.action_id.clone(),
            owner: token.owner.clone(),
            fencing_epoch: token.epoch,
            attempt_ordinal,
            idempotency_key: decision.idempotency_key.clone(),
        })
    }

    fn current_for_token(
        &self,
        token: &FencingToken,
        now: Timestamp,
    ) -> Result<RecoveryLease, RecoveryLeaseBlock<S::Error>> {
        let current = self
            .store
            .load(&token.action_id)
            .map_err(RecoveryLeaseBlock::Store)?
            .ok_or(RecoveryLeaseBlock::LeaseMissing)?;
        if current.action_id != token.action_id {
            return Err(RecoveryLeaseBlock::ActionMismatch);
        }
        if current.version.epoch != token.epoch {
            return Err(RecoveryLeaseBlock::StaleFence {
                presented_epoch: token.epoch,
                current_epoch: current.version.epoch,
            });
        }
        if current.owner != token.owner {
            return Err(RecoveryLeaseBlock::WrongOwner);
        }
        if current.expires_at <= now {
            return Err(RecoveryLeaseBlock::LeaseExpired {
                epoch: current.version.epoch,
            });
        }
        Ok(current)
    }
}

fn expiry_from<E>(
    now: Timestamp,
    ttl_seconds: i64,
) -> Result<Timestamp, RecoveryLeaseBlock<E>> {
    if ttl_seconds <= 0 {
        return Err(LeaseConfigBlock::InvalidTtl.into());
    }
    let expires_at = now
        .epoch_seconds()
        .checked_add(ttl_seconds)
        .ok_or(LeaseConfigBlock::InvalidTtl)?;
    Ok(Timestamp::new(expires_at))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FencedJournalError<E> {
    Lease(RecoveryLeaseBlock<E>),
    Journal(JournalError),
    PermitActionMismatch,
    PermitOwnerMismatch,
    PermitEpochMismatch,
}

impl<E> From<RecoveryLeaseBlock<E>> for FencedJournalError<E> {
    fn from(value: RecoveryLeaseBlock<E>) -> Self {
        Self::Lease(value)
    }
}

impl<E> From<JournalError> for FencedJournalError<E> {
    fn from(value: JournalError) -> Self {
        Self::Journal(value)
    }
}

/// Multi-worker gate around `DurableJournal`.
///
/// Every mutating local operation first validates the current fencing epoch.
/// The returned `FencedAttemptPermit` must also be propagated to a compatible
/// downstream actuator/provider to obtain end-to-end fencing across the network
/// boundary. Local fencing alone cannot stop a stale process from talking to an
/// external service that ignores fencing epochs.
pub struct FencedDurableJournal<S: RecoveryLeaseStore> {
    journal: DurableJournal,
    leases: RecoveryLeaseManager<S>,
    token: FencingToken,
}

impl<S: RecoveryLeaseStore> FencedDurableJournal<S> {
    pub fn acquire(
        journal: DurableJournal,
        store: S,
        owner: RecoveryWorkerId,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<Self, FencedJournalError<S::Error>> {
        let leases = RecoveryLeaseManager::new(store);
        let token = leases.acquire(&journal.ticket().action_id, owner, now, ttl_seconds)?;
        Ok(Self {
            journal,
            leases,
            token,
        })
    }

    pub fn token(&self) -> &FencingToken {
        &self.token
    }

    pub fn journal(&self) -> &DurableJournal {
        &self.journal
    }

    pub fn renew(
        &self,
        now: Timestamp,
        ttl_seconds: i64,
    ) -> Result<RecoveryLease, FencedJournalError<S::Error>> {
        Ok(self.leases.renew(&self.token, now, ttl_seconds)?)
    }

    pub fn assert_current(
        &self,
        now: Timestamp,
    ) -> Result<RecoveryLease, FencedJournalError<S::Error>> {
        Ok(self.leases.assert_current(&self.token, now)?)
    }

    pub fn prepare_attempt(
        &mut self,
        decision: &RetryDecision,
        prepared_at: Timestamp,
        now: Timestamp,
    ) -> Result<FencedAttemptPermit, FencedJournalError<S::Error>> {
        let permit = self.leases.authorize_attempt(&self.token, decision, now)?;
        let prepared = self.journal.prepare_attempt(decision, prepared_at)?;
        if prepared.action_id != permit.action_id || prepared.ordinal != permit.attempt_ordinal {
            return Err(FencedJournalError::PermitActionMismatch);
        }
        Ok(permit)
    }

    pub fn record_dispatch(
        &mut self,
        permit: &FencedAttemptPermit,
        dispatched_at: Timestamp,
        proof_ref: impl Into<String>,
        now: Timestamp,
    ) -> Result<(), FencedJournalError<S::Error>> {
        self.validate_permit(permit, now)?;
        self.journal
            .record_dispatch(permit.attempt_ordinal, dispatched_at, proof_ref)?;
        Ok(())
    }

    pub fn record_reconciliation(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ReconciliationOutcome,
        proof_ref: impl Into<String>,
        now: Timestamp,
    ) -> Result<(), FencedJournalError<S::Error>> {
        self.leases.assert_current(&self.token, now)?;
        self.journal
            .record_reconciliation(ordinal, observed_at, outcome, proof_ref)?;
        Ok(())
    }

    pub fn record_external_outcome(
        &mut self,
        ordinal: u32,
        observed_at: Timestamp,
        outcome: ExecutionOutcome,
        proof_ref: impl Into<String>,
        now: Timestamp,
    ) -> Result<(), FencedJournalError<S::Error>> {
        self.leases.assert_current(&self.token, now)?;
        self.journal
            .record_external_outcome(ordinal, observed_at, outcome, proof_ref)?;
        Ok(())
    }

    fn validate_permit(
        &self,
        permit: &FencedAttemptPermit,
        now: Timestamp,
    ) -> Result<(), FencedJournalError<S::Error>> {
        self.leases.assert_current(&self.token, now)?;
        if permit.action_id != self.token.action_id {
            return Err(FencedJournalError::PermitActionMismatch);
        }
        if permit.owner != self.token.owner {
            return Err(FencedJournalError::PermitOwnerMismatch);
        }
        if permit.fencing_epoch != self.token.epoch {
            return Err(FencedJournalError::PermitEpochMismatch);
        }
        Ok(())
    }
}

/// Process-local CAS store used by tests/examples.
///
/// This implementation is atomic only among users sharing the same in-process
/// `Arc<Mutex<...>>`. It is NOT a cross-process or distributed lease backend.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRecoveryLeaseStore {
    inner: Arc<Mutex<HashMap<String, RecoveryLease>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InMemoryLeaseError {
    Poisoned,
}

impl RecoveryLeaseStore for InMemoryRecoveryLeaseStore {
    type Error = InMemoryLeaseError;

    fn load(&self, action_id: &ActionId) -> Result<Option<RecoveryLease>, Self::Error> {
        let guard = self.inner.lock().map_err(|_| InMemoryLeaseError::Poisoned)?;
        Ok(guard.get(action_id.as_str()).cloned())
    }

    fn compare_and_swap(
        &self,
        action_id: &ActionId,
        expected: Option<LeaseVersion>,
        replacement: RecoveryLease,
    ) -> Result<bool, Self::Error> {
        let mut guard = self.inner.lock().map_err(|_| InMemoryLeaseError::Poisoned)?;
        let current_version = guard.get(action_id.as_str()).map(|lease| lease.version);
        if current_version != expected {
            return Ok(false);
        }
        guard.insert(action_id.as_str().to_owned(), replacement);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use lifetra_bead::BeadId;

    use crate::{
        AuthorityTicket, ExecutionMode, IdempotencyBinding, RetryReason,
    };

    use super::*;

    fn action_id() -> ActionId {
        ActionId::new("action:fence:1").expect("valid action")
    }

    fn worker(value: &str) -> RecoveryWorkerId {
        RecoveryWorkerId::new(value).expect("valid worker")
    }

    fn temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lifetra-fencing-{name}-{}-{nonce}.journal",
            std::process::id()
        ))
    }

    fn journal(path: &PathBuf) -> DurableJournal {
        let ticket = AuthorityTicket {
            action_id: action_id(),
            source_bead: BeadId::new("bead:fence:1"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:fence".into()],
            issued_at: Timestamp::new(10),
        };
        let binding = IdempotencyBinding::new(&ticket, "idem:fence:1").expect("binding");
        DurableJournal::create(path, ticket, binding).expect("journal")
    }

    fn initial_decision() -> RetryDecision {
        RetryDecision {
            action_id: action_id(),
            idempotency_key: "idem:fence:1".into(),
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: vec!["proof:authority:fence".into()],
        }
    }

    #[test]
    fn active_lease_blocks_second_worker() {
        let store = InMemoryRecoveryLeaseStore::default();
        let a = RecoveryLeaseManager::new(store.clone());
        let b = RecoveryLeaseManager::new(store);
        let action = action_id();
        let token = a
            .acquire(&action, worker("worker-a"), Timestamp::new(100), 30)
            .expect("worker a acquires");

        let blocked = b.acquire(&action, worker("worker-b"), Timestamp::new(110), 30);
        assert!(matches!(blocked, Err(RecoveryLeaseBlock::LeaseHeld { epoch: 1, .. })));
        assert_eq!(token.epoch, 1);
    }

    #[test]
    fn expired_lease_reacquisition_increments_fencing_epoch() {
        let store = InMemoryRecoveryLeaseStore::default();
        let a = RecoveryLeaseManager::new(store.clone());
        let b = RecoveryLeaseManager::new(store);
        let action = action_id();
        let stale = a
            .acquire(&action, worker("worker-a"), Timestamp::new(100), 10)
            .expect("worker a acquires");
        let current = b
            .acquire(&action, worker("worker-b"), Timestamp::new(111), 20)
            .expect("worker b acquires after expiry");

        assert_eq!(stale.epoch, 1);
        assert_eq!(current.epoch, 2);
        assert_eq!(
            a.assert_current(&stale, Timestamp::new(112)),
            Err(RecoveryLeaseBlock::StaleFence {
                presented_epoch: 1,
                current_epoch: 2,
            })
        );
    }

    #[test]
    fn renewal_keeps_epoch_and_advances_revision() {
        let store = InMemoryRecoveryLeaseStore::default();
        let manager = RecoveryLeaseManager::new(store);
        let action = action_id();
        let token = manager
            .acquire(&action, worker("worker-a"), Timestamp::new(100), 10)
            .expect("acquire");
        let renewed = manager
            .renew(&token, Timestamp::new(105), 20)
            .expect("renew");

        assert_eq!(renewed.version.epoch, 1);
        assert_eq!(renewed.version.revision, 1);
        assert_eq!(renewed.expires_at, Timestamp::new(125));
        manager
            .assert_current(&token, Timestamp::new(120))
            .expect("same fence remains current after renewal");
    }

    #[test]
    fn stale_worker_cannot_record_dispatch_after_takeover() {
        let path = temp_path("stale-dispatch");
        let store = InMemoryRecoveryLeaseStore::default();
        let mut fenced = FencedDurableJournal::acquire(
            journal(&path),
            store.clone(),
            worker("worker-a"),
            Timestamp::new(100),
            10,
        )
        .expect("worker a acquires");
        let permit = fenced
            .prepare_attempt(&initial_decision(), Timestamp::new(101), Timestamp::new(101))
            .expect("prepared under current fence");

        let takeover = RecoveryLeaseManager::new(store)
            .acquire(&action_id(), worker("worker-b"), Timestamp::new(111), 20)
            .expect("worker b takes over");
        assert_eq!(takeover.epoch, 2);

        let blocked = fenced.record_dispatch(
            &permit,
            Timestamp::new(112),
            "proof:dispatch:stale",
            Timestamp::new(112),
        );
        assert!(matches!(
            blocked,
            Err(FencedJournalError::Lease(RecoveryLeaseBlock::StaleFence {
                presented_epoch: 1,
                current_epoch: 2,
            }))
        ));
        assert!(fenced
            .journal()
            .recover()
            .expect("replay")
            .pending_prepared
            .is_some());
        fs::remove_file(path).ok();
    }

    #[test]
    fn permit_carries_epoch_but_is_not_dispatch_evidence() {
        let path = temp_path("permit");
        let store = InMemoryRecoveryLeaseStore::default();
        let mut fenced = FencedDurableJournal::acquire(
            journal(&path),
            store,
            worker("worker-a"),
            Timestamp::new(100),
            30,
        )
        .expect("acquire");
        let permit = fenced
            .prepare_attempt(&initial_decision(), Timestamp::new(101), Timestamp::new(101))
            .expect("prepare");

        assert_eq!(permit.fencing_epoch, 1);
        assert_eq!(permit.attempt_ordinal, 0);
        let recovered = fenced.journal().recover().expect("replay");
        assert!(recovered.ledger.attempts().is_empty());
        assert!(recovered.pending_prepared.is_some());
        fs::remove_file(path).ok();
    }
}
