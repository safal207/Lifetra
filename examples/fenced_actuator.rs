use lifetra::{
    ActionId, FencedActuatorController, FencedActuatorOutcome, FencedActuatorRejection,
    FencedAttemptPermit, InMemoryActuatorAuthority, InMemoryFencedActuator, RecoveryWorkerId,
    Timestamp,
};

fn main() {
    let action_id = ActionId::new("action:payout:fenced-actuator:demo").expect("valid action id");
    let worker_a = RecoveryWorkerId::new("worker-a").expect("valid worker");
    let worker_b = RecoveryWorkerId::new("worker-b").expect("valid worker");
    let stale_permit = FencedAttemptPermit {
        action_id: action_id.clone(),
        owner: worker_a.clone(),
        fencing_epoch: 1,
        attempt_ordinal: 0,
        idempotency_key: "idem:payout:fenced-actuator:demo".into(),
    };

    let actuator = InMemoryFencedActuator::default();
    actuator.set_time(Timestamp::new(100)).expect("sink time");
    actuator
        .install_authority(InMemoryActuatorAuthority {
            action_id: action_id.clone(),
            owner: worker_a,
            epoch: 1,
            expires_at: Timestamp::new(110),
        })
        .expect("epoch one authority");

    // Worker A already holds a permit, but stalls before reaching the actuator.
    // Recovery later installs worker B as the authoritative epoch 2 owner.
    actuator.set_time(Timestamp::new(112)).expect("sink time");
    actuator
        .install_authority(InMemoryActuatorAuthority {
            action_id: action_id.clone(),
            owner: worker_b.clone(),
            epoch: 2,
            expires_at: Timestamp::new(140),
        })
        .expect("epoch two authority");

    // The old worker finally sends its previously issued permit. Downstream
    // fencing rejects it before a new effect is inserted.
    let stale_receipt = FencedActuatorController
        .execute(&stale_permit, "operation:payout:42", &actuator)
        .expect("stale request should return rejection evidence");
    assert_eq!(
        stale_receipt.outcome,
        FencedActuatorOutcome::Rejected(FencedActuatorRejection::StaleEpoch {
            current_epoch: 2,
        })
    );
    assert_eq!(actuator.applied_count().expect("effect count"), 0);

    // Current worker B can apply the same logical operation under the newer epoch.
    let current_permit = FencedAttemptPermit {
        action_id,
        owner: worker_b,
        fencing_epoch: 2,
        attempt_ordinal: 1,
        idempotency_key: "idem:payout:fenced-actuator:demo".into(),
    };
    let applied = FencedActuatorController
        .execute(&current_permit, "operation:payout:42", &actuator)
        .expect("current request should execute");
    assert_eq!(applied.outcome, FencedActuatorOutcome::Applied);
    assert_eq!(actuator.applied_count().expect("effect count"), 1);

    // A duplicate of the same logical effect is observed, not applied again.
    let duplicate = FencedActuatorController
        .execute(&current_permit, "operation:payout:42", &actuator)
        .expect("duplicate should be observable");
    assert_eq!(
        duplicate.outcome,
        FencedActuatorOutcome::AlreadyApplied {
            applied_epoch: 2,
            applied_attempt_ordinal: 1,
        }
    );
    assert_eq!(actuator.applied_count().expect("effect count"), 1);

    println!("stale_outcome={:?}", stale_receipt.outcome);
    println!("current_outcome={:?}", applied.outcome);
    println!("duplicate_outcome={:?}", duplicate.outcome);
    println!("effects_applied={}", actuator.applied_count().expect("count"));
}
