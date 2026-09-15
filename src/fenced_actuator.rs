use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lifetra_core::Timestamp;

use crate::{ActionId, FencedAttemptPermit, RecoveryWorkerId};

/// Provider-facing request for one side effect under a fencing epoch.
///
/// `operation_ref` must remain stable for the lifetime of the logical action.
/// It can be a digest, immutable command ID, canonical payload reference, or
/// another provider-specific identity that prevents a newer worker from changing
/// the meaning of an existing `ActionId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedActuatorRequest {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub fencing_epoch: u64,
    pub attempt_ordinal: u32,
    pub idempotency_key: String,
    pub operation_ref: String,
}

impl FencedActuatorRequest {
    pub fn from_permit(
        permit: &FencedAttemptPermit,
        operation_ref: impl Into<String>,
    ) -> Result<Self, FencedActuatorConfigBlock> {
        let operation_ref = operation_ref.into();
        if operation_ref.trim().is_empty() {
            return Err(FencedActuatorConfigBlock::EmptyOperationRef);
        }

        Ok(Self {
            action_id: permit.action_id.clone(),
            owner: permit.owner.clone(),
            fencing_epoch: permit.fencing_epoch,
            attempt_ordinal: permit.attempt_ordinal,
            idempotency_key: permit.idempotency_key.clone(),
            operation_ref,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FencedActuatorRejection {
    StaleEpoch { current_epoch: u64 },
    FutureEpoch { current_epoch: u64 },
    WrongOwner,
    LeaseExpired { current_epoch: u64 },
    IdentityConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FencedActuatorOutcome {
    /// This request atomically passed the downstream fence and applied the effect.
    Applied,
    /// The same logical effect was already applied; this request did not apply it again.
    AlreadyApplied {
        applied_epoch: u64,
        applied_attempt_ordinal: u32,
    },
    /// The downstream authority rejected this request before applying the effect.
    Rejected(FencedActuatorRejection),
}

/// Inspectable downstream evidence returned by a fenced actuator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FencedActuatorReceipt {
    pub request: FencedActuatorRequest,
    pub observed_at: Timestamp,
    pub outcome: FencedActuatorOutcome,
    pub proof_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FencedActuatorConfigBlock {
    EmptyOperationRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FencedActuatorError<E> {
    Config(FencedActuatorConfigBlock),
    Adapter(E),
    ReceiptIdentityMismatch,
    EmptyProof,
}

impl<E> From<FencedActuatorConfigBlock> for FencedActuatorError<E> {
    fn from(value: FencedActuatorConfigBlock) -> Self {
        Self::Config(value)
    }
}

/// Downstream side-effect boundary that enforces fencing.
///
/// Implementations MUST make the authoritative fence comparison and the side
/// effect one atomic operation (or use an external primitive with equivalent
/// semantics). A read-current-epoch call followed by a separate side effect is
/// still vulnerable to TOCTOU races and does not satisfy this contract.
pub trait FencedActuatorAdapter {
    type Error;

    fn execute(
        &self,
        request: &FencedActuatorRequest,
    ) -> Result<FencedActuatorReceipt, Self::Error>;
}

/// Provider-neutral controller that binds a local fenced permit to a stable
/// operation identity and validates the returned receipt.
///
/// It deliberately does not turn an actuator receipt into a local
/// `DispatchReceipt`. Network dispatch evidence and downstream effect evidence
/// remain separate proof boundaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FencedActuatorController;

impl FencedActuatorController {
    pub fn execute<A: FencedActuatorAdapter>(
        &self,
        permit: &FencedAttemptPermit,
        operation_ref: impl Into<String>,
        actuator: &A,
    ) -> Result<FencedActuatorReceipt, FencedActuatorError<A::Error>> {
        let request = FencedActuatorRequest::from_permit(permit, operation_ref)?;
        let receipt = actuator
            .execute(&request)
            .map_err(FencedActuatorError::Adapter)?;

        if receipt.request != request {
            return Err(FencedActuatorError::ReceiptIdentityMismatch);
        }
        if receipt.proof_ref.trim().is_empty() {
            return Err(FencedActuatorError::EmptyProof);
        }

        Ok(receipt)
    }
}

/// Authoritative fencing state used by the process-local actuator below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemoryActuatorAuthority {
    pub action_id: ActionId,
    pub owner: RecoveryWorkerId,
    pub epoch: u64,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemoryAppliedEffect {
    pub action_id: ActionId,
    pub idempotency_key: String,
    pub operation_ref: String,
    pub applied_epoch: u64,
    pub applied_attempt_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InMemoryFencedActuatorError {
    Poisoned,
    AuthorityMissing,
    AuthorityRegression,
    ConflictingOwnerAtSameEpoch,
}

#[derive(Debug)]
struct InMemoryFencedActuatorState {
    now: Timestamp,
    authorities: HashMap<String, InMemoryActuatorAuthority>,
    effects: HashMap<String, InMemoryAppliedEffect>,
    proof_sequence: u64,
}

impl Default for InMemoryFencedActuatorState {
    fn default() -> Self {
        Self {
            now: Timestamp::new(0),
            authorities: HashMap::new(),
            effects: HashMap::new(),
            proof_sequence: 0,
        }
    }
}

/// Process-local reference implementation of the downstream fencing contract.
///
/// Authority comparison and effect insertion share one mutex critical section,
/// so the test implementation has an atomic compare+apply boundary inside one
/// process. It is NOT a production distributed actuator.
#[derive(Debug, Clone, Default)]
pub struct InMemoryFencedActuator {
    inner: Arc<Mutex<InMemoryFencedActuatorState>>,
}

impl InMemoryFencedActuator {
    pub fn set_time(&self, now: Timestamp) -> Result<(), InMemoryFencedActuatorError> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| InMemoryFencedActuatorError::Poisoned)?;
        state.now = now;
        Ok(())
    }

    pub fn install_authority(
        &self,
        authority: InMemoryActuatorAuthority,
    ) -> Result<(), InMemoryFencedActuatorError> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| InMemoryFencedActuatorError::Poisoned)?;
        if let Some(current) = state.authorities.get(authority.action_id.as_str()) {
            if authority.epoch < current.epoch {
                return Err(InMemoryFencedActuatorError::AuthorityRegression);
            }
            if authority.epoch == current.epoch && authority.owner != current.owner {
                return Err(InMemoryFencedActuatorError::ConflictingOwnerAtSameEpoch);
            }
        }
        state
            .authorities
            .insert(authority.action_id.as_str().to_owned(), authority);
        Ok(())
    }

    pub fn applied_effect(
        &self,
        action_id: &ActionId,
    ) -> Result<Option<InMemoryAppliedEffect>, InMemoryFencedActuatorError> {
        let state = self
            .inner
            .lock()
            .map_err(|_| InMemoryFencedActuatorError::Poisoned)?;
        Ok(state.effects.get(action_id.as_str()).cloned())
    }

    pub fn applied_count(&self) -> Result<usize, InMemoryFencedActuatorError> {
        let state = self
            .inner
            .lock()
            .map_err(|_| InMemoryFencedActuatorError::Poisoned)?;
        Ok(state.effects.len())
    }
}

impl FencedActuatorAdapter for InMemoryFencedActuator {
    type Error = InMemoryFencedActuatorError;

    fn execute(
        &self,
        request: &FencedActuatorRequest,
    ) -> Result<FencedActuatorReceipt, Self::Error> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| InMemoryFencedActuatorError::Poisoned)?;
        let authority = state
            .authorities
            .get(request.action_id.as_str())
            .cloned()
            .ok_or(InMemoryFencedActuatorError::AuthorityMissing)?;

        let outcome = if request.fencing_epoch < authority.epoch {
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch {
                current_epoch: authority.epoch,
            })
        } else if request.fencing_epoch > authority.epoch {
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::FutureEpoch {
                current_epoch: authority.epoch,
            })
        } else if request.owner != authority.owner {
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::WrongOwner)
        } else if authority.expires_at <= state.now {
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::LeaseExpired {
                current_epoch: authority.epoch,
            })
        } else if let Some(existing) = state.effects.get(request.action_id.as_str()) {
            if existing.idempotency_key != request.idempotency_key
                || existing.operation_ref != request.operation_ref
            {
                FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict)
            } else {
                FencedActuatorOutcome::AlreadyApplied {
                    applied_epoch: existing.applied_epoch,
                    applied_attempt_ordinal: existing.applied_attempt_ordinal,
                }
            }
        } else {
            state.effects.insert(
                request.action_id.as_str().to_owned(),
                InMemoryAppliedEffect {
                    action_id: request.action_id.clone(),
                    idempotency_key: request.idempotency_key.clone(),
                    operation_ref: request.operation_ref.clone(),
                    applied_epoch: request.fencing_epoch,
                    applied_attempt_ordinal: request.attempt_ordinal,
                },
            );
            FencedActuatorOutcome::Applied
        };

        state.proof_sequence = state.proof_sequence.saturating_add(1);
        let proof_ref = format!(
            "proof:in-memory-fenced-actuator:{}:{}",
            request.action_id.as_str(),
            state.proof_sequence
        );

        Ok(FencedActuatorReceipt {
            request: request.clone(),
            observed_at: state.now,
            outcome,
            proof_ref,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::RecoveryWorkerId;

    use super::*;

    fn action() -> ActionId {
        ActionId::new("action:actuator:1").expect("valid action")
    }

    fn owner(value: &str) -> RecoveryWorkerId {
        RecoveryWorkerId::new(value).expect("valid owner")
    }

    fn permit(epoch: u64, owner: &str, ordinal: u32) -> FencedAttemptPermit {
        FencedAttemptPermit {
            action_id: action(),
            owner: self::owner(owner),
            fencing_epoch: epoch,
            attempt_ordinal: ordinal,
            idempotency_key: "idem:actuator:1".into(),
        }
    }

    fn authority(epoch: u64, owner: &str, expires_at: i64) -> InMemoryActuatorAuthority {
        InMemoryActuatorAuthority {
            action_id: action(),
            owner: self::owner(owner),
            epoch,
            expires_at: Timestamp::new(expires_at),
        }
    }

    #[test]
    fn request_preserves_permit_identity_and_operation_reference() {
        let request = FencedActuatorRequest::from_permit(&permit(7, "worker-a", 2), "op:digest:1")
            .expect("request");

        assert_eq!(request.action_id, action());
        assert_eq!(request.owner, owner("worker-a"));
        assert_eq!(request.fencing_epoch, 7);
        assert_eq!(request.attempt_ordinal, 2);
        assert_eq!(request.idempotency_key, "idem:actuator:1");
        assert_eq!(request.operation_ref, "op:digest:1");
    }

    #[test]
    fn current_epoch_applies_effect_once() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 130))
            .expect("authority");
        let controller = FencedActuatorController;

        let receipt = controller
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("apply");
        assert_eq!(receipt.outcome, FencedActuatorOutcome::Applied);
        assert_eq!(actuator.applied_count().expect("count"), 1);
    }

    #[test]
    fn duplicate_same_identity_is_not_applied_twice() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 130))
            .expect("authority");
        let controller = FencedActuatorController;
        controller
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("first apply");

        let duplicate = controller
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("duplicate observation");
        assert_eq!(
            duplicate.outcome,
            FencedActuatorOutcome::AlreadyApplied {
                applied_epoch: 1,
                applied_attempt_ordinal: 0,
            }
        );
        assert_eq!(actuator.applied_count().expect("count"), 1);
    }

    #[test]
    fn stale_epoch_is_rejected_after_new_authority_is_installed() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(112)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 110))
            .expect("old authority");
        let stale_permit = permit(1, "worker-a", 0);
        actuator
            .install_authority(authority(2, "worker-b", 140))
            .expect("takeover authority");

        let receipt = FencedActuatorController
            .execute(&stale_permit, "op:digest:1", &actuator)
            .expect("stale rejection is a receipt");
        assert_eq!(
            receipt.outcome,
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch {
                current_epoch: 2,
            })
        );
        assert_eq!(actuator.applied_count().expect("count"), 0);
    }

    #[test]
    fn future_unissued_epoch_is_rejected() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(2, "worker-b", 140))
            .expect("authority");

        let receipt = FencedActuatorController
            .execute(&permit(3, "worker-c", 0), "op:digest:1", &actuator)
            .expect("future rejection is a receipt");
        assert_eq!(
            receipt.outcome,
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::FutureEpoch {
                current_epoch: 2,
            })
        );
        assert_eq!(actuator.applied_count().expect("count"), 0);
    }

    #[test]
    fn higher_epoch_same_identity_observes_prior_effect_without_reapplying() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 110))
            .expect("authority one");
        let controller = FencedActuatorController;
        controller
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("first apply");

        actuator.set_time(Timestamp::new(112)).expect("time");
        actuator
            .install_authority(authority(2, "worker-b", 140))
            .expect("authority two");
        let replay = controller
            .execute(&permit(2, "worker-b", 1), "op:digest:1", &actuator)
            .expect("replay observation");

        assert_eq!(
            replay.outcome,
            FencedActuatorOutcome::AlreadyApplied {
                applied_epoch: 1,
                applied_attempt_ordinal: 0,
            }
        );
        assert_eq!(actuator.applied_count().expect("count"), 1);
    }

    #[test]
    fn operation_identity_cannot_change_under_same_action() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 110))
            .expect("authority one");
        let controller = FencedActuatorController;
        controller
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("first apply");

        actuator.set_time(Timestamp::new(112)).expect("time");
        actuator
            .install_authority(authority(2, "worker-b", 140))
            .expect("authority two");
        let changed = controller
            .execute(&permit(2, "worker-b", 1), "op:digest:DIFFERENT", &actuator)
            .expect("identity rejection is a receipt");

        assert_eq!(
            changed.outcome,
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::IdentityConflict)
        );
        assert_eq!(actuator.applied_count().expect("count"), 1);
    }

    #[test]
    fn wrong_owner_and_expired_authority_are_rejected_before_effect() {
        let actuator = InMemoryFencedActuator::default();
        actuator.set_time(Timestamp::new(100)).expect("time");
        actuator
            .install_authority(authority(1, "worker-a", 105))
            .expect("authority");

        let wrong_owner = FencedActuatorController
            .execute(&permit(1, "worker-b", 0), "op:digest:1", &actuator)
            .expect("wrong owner receipt");
        assert_eq!(
            wrong_owner.outcome,
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::WrongOwner)
        );

        actuator.set_time(Timestamp::new(105)).expect("time");
        let expired = FencedActuatorController
            .execute(&permit(1, "worker-a", 0), "op:digest:1", &actuator)
            .expect("expired receipt");
        assert_eq!(
            expired.outcome,
            FencedActuatorOutcome::Rejected(FencedActuatorRejection::LeaseExpired {
                current_epoch: 1,
            })
        );
        assert_eq!(actuator.applied_count().expect("count"), 0);
    }

    #[derive(Debug)]
    struct BadReceiptAdapter;

    impl FencedActuatorAdapter for BadReceiptAdapter {
        type Error = ();

        fn execute(
            &self,
            request: &FencedActuatorRequest,
        ) -> Result<FencedActuatorReceipt, Self::Error> {
            let mut wrong = request.clone();
            wrong.attempt_ordinal += 1;
            Ok(FencedActuatorReceipt {
                request: wrong,
                observed_at: Timestamp::new(1),
                outcome: FencedActuatorOutcome::Applied,
                proof_ref: "proof:bad".into(),
            })
        }
    }

    #[test]
    fn controller_rejects_receipt_identity_mismatch() {
        let result = FencedActuatorController.execute(
            &permit(1, "worker-a", 0),
            "op:digest:1",
            &BadReceiptAdapter,
        );

        assert_eq!(result, Err(FencedActuatorError::ReceiptIdentityMismatch));
    }
}
