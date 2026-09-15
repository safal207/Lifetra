use std::fs;

use lifetra::{
    ActionId, AuthorityTicket, BeadId, BeadScale, DurableActuatorReceiptBridge, DurableJournal,
    ExecutionMode, FencedActuatorController, FencedActuatorOutcome, FencedDurableJournal,
    IdempotencyBinding, InMemoryActuatorAuthority, InMemoryFencedActuator,
    InMemoryRecoveryLeaseStore, RecoveryDirective, RecoveryWorkerId, RetryDecision, RetryReason,
    RetryVerdict, Timestamp, TrajectoryBead,
};

fn main() {
    let nonce = std::process::id();
    let journal_path = std::env::temp_dir().join(format!("lifetra-bridge-{nonce}.journal"));
    let bridge_path = std::env::temp_dir().join(format!("lifetra-bridge-{nonce}.events"));
    fs::remove_file(&journal_path).ok();
    fs::remove_dir_all(&bridge_path).ok();

    let action_id = ActionId::new("action:payout:bridge:demo").expect("valid action id");
    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:payout:bridge:demo"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:bridge-demo".into()],
        issued_at: Timestamp::new(10),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:payout:bridge:demo").expect("valid binding");
    let journal = DurableJournal::create(&journal_path, ticket, binding).expect("create journal");
    let mut bridge = DurableActuatorReceiptBridge::create(
        &bridge_path,
        &journal,
        "operation:payout:bridge:demo",
    )
    .expect("create bridge");

    let lease_store = InMemoryRecoveryLeaseStore::default();
    let mut fenced = FencedDurableJournal::acquire(
        journal,
        lease_store.clone(),
        RecoveryWorkerId::new("worker-a").expect("worker A"),
        Timestamp::new(100),
        10,
    )
    .expect("worker A lease");
    let initial = RetryDecision {
        action_id: action_id.clone(),
        idempotency_key: "idem:payout:bridge:demo".into(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: vec!["proof:authority:bridge-demo".into()],
    };
    let permit = bridge
        .prepare(
            &mut fenced,
            &initial,
            Timestamp::new(101),
            Timestamp::new(101),
        )
        .expect("durable preparation");

    let actuator = InMemoryFencedActuator::default();
    actuator.set_time(Timestamp::new(102)).expect("actuator time");
    actuator
        .install_authority(InMemoryActuatorAuthority {
            action_id: action_id.clone(),
            owner: permit.owner.clone(),
            epoch: permit.fencing_epoch,
            expires_at: Timestamp::new(110),
        })
        .expect("actuator authority");
    let applied = FencedActuatorController
        .execute(&permit, bridge.binding().operation_ref.clone(), &actuator)
        .expect("apply effect");
    assert_eq!(applied.outcome, FencedActuatorOutcome::Applied);

    // Crash window: the effect exists externally, but `applied` is never persisted locally.
    drop(fenced);
    drop(bridge);

    // Recovery worker takes over with a newer fence.
    actuator.set_time(Timestamp::new(112)).expect("actuator time");
    actuator
        .install_authority(InMemoryActuatorAuthority {
            action_id: action_id.clone(),
            owner: RecoveryWorkerId::new("worker-b").expect("worker B"),
            epoch: 2,
            expires_at: Timestamp::new(140),
        })
        .expect("takeover authority");

    let journal = DurableJournal::open(&journal_path).expect("reopen journal");
    let mut bridge = DurableActuatorReceiptBridge::open(&bridge_path).expect("reopen bridge");
    let observation = bridge
        .reconcile_after_crash(&journal, &actuator, Timestamp::new(113))
        .expect("reconcile by durable operation identity");

    let mut fenced = FencedDurableJournal::acquire(
        journal,
        lease_store,
        RecoveryWorkerId::new("worker-b").expect("worker B"),
        Timestamp::new(111),
        30,
    )
    .expect("worker B lease");
    bridge
        .project_pending(&mut fenced, Timestamp::new(113))
        .expect("project recovered proof");
    assert_eq!(
        fenced.journal().recover().expect("recover").directive,
        RecoveryDirective::CloseSucceeded { ordinal: 0 }
    );

    // The same external proof can seed evidence for a later trajectory bead.
    let state = bridge.recover().expect("bridge state");
    let evidence = state
        .latest_success_evidence()
        .expect("success evidence")
        .as_supported_evidence()
        .expect("supported evidence");
    let next_bead = TrajectoryBead::new(
        BeadId::new("bead:after-payout"),
        BeadScale::Event,
        Timestamp::new(114),
        Timestamp::new(115),
    )
    .with_evidence(evidence);

    println!("recovered_outcome={:?}", observation.outcome);
    println!("journal_directive=CloseSucceeded");
    println!("next_bead_proofs={}", next_bead.supported_evidence_count());

    fs::remove_file(journal_path).ok();
    fs::remove_dir_all(bridge_path).ok();
}
