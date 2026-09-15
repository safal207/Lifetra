use lifetra::{
    ActionId, AuthorityTicket, BeadId, BeadScale, ExecutionMode, FencedActuatorController,
    IdempotencyBinding, InMemoryActuatorAuthority, InMemoryFencedActuator,
    InMemoryUnifiedFencedStore, RecoveryWorkerId, RetryDecision, RetryReason, RetryVerdict,
    Timestamp, TrajectoryBead, UnifiedFencedRuntime, UnifiedRuntimeDirective,
};

fn main() {
    let action_id = ActionId::new("action:unified-store:demo").expect("action id");
    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:unified-store:demo"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:unified-store-demo".into()],
        issued_at: Timestamp::new(10),
    };
    let binding = IdempotencyBinding::new(&ticket, "idem:unified-store:demo").expect("binding");
    let runtime = UnifiedFencedRuntime::new(InMemoryUnifiedFencedStore::default());
    runtime
        .create_action(
            &ticket,
            &binding,
            "operation:payout:unified-store-demo",
        )
        .expect("create action record");

    let token = runtime
        .acquire(
            &action_id,
            RecoveryWorkerId::new("worker-a").expect("worker"),
            Timestamp::new(100),
            30,
        )
        .expect("acquire lease");
    let decision = RetryDecision {
        action_id: action_id.clone(),
        idempotency_key: binding.key.clone(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: ticket.authority_proof_refs.clone(),
    };
    let permit = runtime
        .prepare_attempt(
            &token,
            &decision,
            Timestamp::new(101),
            Timestamp::new(101),
        )
        .expect("prepare attempt");

    let actuator = InMemoryFencedActuator::default();
    actuator.set_time(Timestamp::new(102)).expect("time");
    actuator
        .install_authority(InMemoryActuatorAuthority {
            action_id: action_id.clone(),
            owner: permit.owner.clone(),
            epoch: permit.fencing_epoch,
            expires_at: Timestamp::new(130),
        })
        .expect("install actuator authority");
    let receipt = FencedActuatorController
        .execute(
            &permit,
            "operation:payout:unified-store-demo",
            &actuator,
        )
        .expect("apply fenced effect");

    let commit = runtime
        .record_actuator_receipt(&receipt)
        .expect("commit receipt and projection marker");
    assert!(commit.projected);
    assert_eq!(
        runtime.directive(&action_id).expect("directive"),
        UnifiedRuntimeDirective::CloseSucceeded {
            ordinal: 0,
            proof_ref: receipt.proof_ref.clone(),
        }
    );

    let evidence = runtime
        .projected_supported_evidence(&action_id)
        .expect("projected evidence");
    let next_bead = TrajectoryBead::new(
        BeadId::new("bead:after-unified-effect"),
        BeadScale::Event,
        Timestamp::new(103),
        Timestamp::new(104),
    )
    .with_evidence(evidence[0].clone());

    println!("record_revision={}", commit.record_revision);
    println!("directive=CloseSucceeded");
    println!("next_bead_proofs={}", next_bead.supported_evidence_count());
}
