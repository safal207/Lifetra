use lifetra::{
    ActionId, AuthorityTicket, BeadId, ExecutionMode, FencedActuatorOutcome,
    FencedActuatorReceipt, FencedActuatorRequest, IdempotencyBinding,
    PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore, RecoveryWorkerId, RetryDecision,
    RetryReason, RetryVerdict, Timestamp, UnifiedRuntimeDirective,
};

fn main() {
    let Ok(dsn) = std::env::var("LIFETRA_POSTGRES_URL") else {
        println!("set LIFETRA_POSTGRES_URL to run the PostgreSQL example");
        return;
    };

    let store = PostgresUnifiedFencedStore::new(dsn);
    store.migrate().expect("migrate PostgreSQL schema");

    let action_id = ActionId::new(format!("action:postgres:demo:{}", std::process::id()))
        .expect("valid action id");
    store.delete_action(&action_id).ok();

    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:postgres:demo"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:postgres-demo".into()],
        issued_at: Timestamp::new(10),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:postgres:demo").expect("valid binding");
    let runtime = PostgresUnifiedFencedRuntime::new(store);
    runtime
        .create_action(&ticket, &binding, "operation:postgres:demo")
        .expect("create action record");

    let token = runtime
        .acquire(
            &action_id,
            RecoveryWorkerId::new("worker:postgres-demo").expect("worker"),
            30,
        )
        .expect("acquire DB-backed fencing epoch");
    let decision = RetryDecision {
        action_id: action_id.clone(),
        idempotency_key: binding.key.clone(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: ticket.authority_proof_refs.clone(),
    };
    let permit = runtime
        .prepare_attempt(&token, &decision)
        .expect("prepare under PostgreSQL lease");

    // A real adapter would obtain this receipt from the fenced external resource.
    let receipt = FencedActuatorReceipt {
        request: FencedActuatorRequest::from_permit(&permit, "operation:postgres:demo")
            .expect("fenced request"),
        observed_at: runtime.store().authoritative_now().expect("database time"),
        outcome: FencedActuatorOutcome::Applied,
        proof_ref: "proof:postgres-demo:applied".into(),
    };
    let committed = runtime
        .record_actuator_receipt(&receipt)
        .expect("commit external proof and projection atomically");
    let directive = runtime.directive(&action_id).expect("runtime directive");

    assert!(committed.projected);
    assert!(matches!(
        directive,
        UnifiedRuntimeDirective::CloseSucceeded { .. }
    ));

    println!("fencing_epoch={}", token.epoch);
    println!("record_revision={}", committed.record_revision);
    println!("directive={directive:?}");

    runtime.store().delete_action(&action_id).ok();
}
