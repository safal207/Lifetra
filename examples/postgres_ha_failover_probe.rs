use lifetra::{
    ActionId, AuthorityTicket, BeadId, ExecutionMode, IdempotencyBinding,
    PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore, RecoveryWorkerId, RetryDecision,
    RetryReason, RetryVerdict, Timestamp, UnifiedActionRecord, UnifiedFencingToken,
};

const ACTION_ID: &str = "action:postgres:ha-failover-probe";
const IDEMPOTENCY_KEY: &str = "idem:postgres:ha-failover-probe";
const OPERATION_REF: &str = "operation:postgres:ha-failover-probe";
const OWNER: &str = "worker:postgres-ha";

fn main() {
    let dsn = std::env::var("LIFETRA_POSTGRES_URL")
        .expect("set LIFETRA_POSTGRES_URL for the HA failover probe");
    let phase = std::env::var("LIFETRA_HA_PHASE").unwrap_or_else(|_| "verify".into());
    let store = PostgresUnifiedFencedStore::new(dsn);
    let runtime = PostgresUnifiedFencedRuntime::new(store.clone());
    let action_id = ActionId::new(ACTION_ID).expect("stable action id");

    match phase.as_str() {
        "seed" => seed(&store, &runtime, &action_id),
        "standby" => {
            let record = verified_record(&runtime, &action_id);
            println!(
                "standby revision={} epoch={} lease_revision={}",
                record.revision,
                record.lease.as_ref().expect("lease").epoch,
                record.lease.as_ref().expect("lease").revision
            );
        }
        "promoted" => {
            let before = verified_record(&runtime, &action_id);
            let lease = before.lease.as_ref().expect("replicated lease");
            let token = UnifiedFencingToken {
                action_id: action_id.clone(),
                owner: lease.owner.clone(),
                epoch: lease.epoch,
            };
            let renewed = runtime
                .renew(&token, 3_600)
                .expect("promoted standby must continue fenced state mutation");
            let after = verified_record(&runtime, &action_id);

            assert_eq!(renewed.epoch, 1);
            assert_eq!(renewed.revision, lease.revision + 1);
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(after.lease.as_ref().expect("renewed lease"), &renewed);
            assert_eq!(after.attempts.len(), 1);

            println!(
                "promoted revision={} epoch={} lease_revision={}",
                after.revision, renewed.epoch, renewed.revision
            );
            store.delete_action(&action_id).ok();
        }
        other => panic!("unsupported LIFETRA_HA_PHASE: {other}"),
    }
}

fn seed(
    store: &PostgresUnifiedFencedStore,
    runtime: &PostgresUnifiedFencedRuntime,
    action_id: &ActionId,
) {
    store.migrate().expect("migrate PostgreSQL schema");
    store.delete_action(action_id).ok();

    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:postgres:ha-failover-probe"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:postgres:ha-failover-authority".into()],
        issued_at: Timestamp::new(10),
    };
    let binding = IdempotencyBinding::new(&ticket, IDEMPOTENCY_KEY).expect("idempotency binding");
    runtime
        .create_action(&ticket, &binding, OPERATION_REF)
        .expect("create replicated action");

    let token = runtime
        .acquire(
            action_id,
            RecoveryWorkerId::new(OWNER).expect("worker"),
            3_600,
        )
        .expect("acquire replicated lease");
    let decision = RetryDecision {
        action_id: action_id.clone(),
        idempotency_key: IDEMPOTENCY_KEY.into(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: ticket.authority_proof_refs.clone(),
    };
    let permit = runtime
        .prepare_attempt(&token, &decision)
        .expect("prepare replicated attempt");
    assert_eq!(permit.attempt_ordinal, 0);

    let record = verified_record(runtime, action_id);
    assert_eq!(record.attempts.len(), 1);
    println!(
        "primary revision={} epoch={} attempt={}",
        record.revision, token.epoch, permit.attempt_ordinal
    );
}

fn verified_record(
    runtime: &PostgresUnifiedFencedRuntime,
    action_id: &ActionId,
) -> UnifiedActionRecord {
    let record = runtime
        .load(action_id)
        .expect("replicated action must be readable");
    assert_eq!(record.binding.action_id, *action_id);
    assert_eq!(record.binding.idempotency_key, IDEMPOTENCY_KEY);
    assert_eq!(record.binding.operation_ref, OPERATION_REF);
    let lease = record.lease.as_ref().expect("replicated lease");
    assert_eq!(lease.owner.as_str(), OWNER);
    assert_eq!(lease.epoch, 1);
    assert_eq!(record.attempts.len(), 1);
    assert_eq!(record.attempts[0].permit.attempt_ordinal, 0);
    record
}
