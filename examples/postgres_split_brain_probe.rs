use lifetra::{
    ActionId, AuthorityTicket, BeadId, ExecutionMode, IdempotencyBinding,
    PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore, RecoveryWorkerId, RetryDecision,
    RetryReason, RetryVerdict, Timestamp, UnifiedActionRecord, UnifiedFencingToken,
};

const ACTION_ID: &str = "action:postgres:split-brain-probe";
const IDEMPOTENCY_KEY: &str = "idem:postgres:split-brain-probe";
const OPERATION_REF: &str = "operation:postgres:split-brain-probe";
const OWNER: &str = "worker:postgres-split-brain";

fn main() {
    let dsn = std::env::var("LIFETRA_POSTGRES_URL")
        .expect("set LIFETRA_POSTGRES_URL for the split-brain probe");
    let phase = std::env::var("LIFETRA_SPLIT_BRAIN_PHASE").unwrap_or_else(|_| "verify".into());
    let store = PostgresUnifiedFencedStore::new(dsn);
    let runtime = PostgresUnifiedFencedRuntime::new(store.clone());
    let action_id = ActionId::new(ACTION_ID).expect("stable action id");

    match phase.as_str() {
        "seed" => seed(&store, &runtime, &action_id),
        "standby" => print_verified("standby", &runtime, &action_id, 2, 0),
        "promoted" => renew_continuity("promoted", &runtime, &action_id, 2, 0, 3, 1),
        "rejoined" => print_verified("rejoined", &runtime, &action_id, 3, 1),
        "leader-after-rejoin" => {
            renew_continuity("leader-after-rejoin", &runtime, &action_id, 3, 1, 4, 2)
        }
        "rejoined-after-sync" => {
            print_verified("rejoined-after-sync", &runtime, &action_id, 4, 2)
        }
        "leader" => print_verified("leader", &runtime, &action_id, 4, 2),
        "cleanup" => {
            store.delete_action(&action_id).ok();
            println!("cleaned split-brain probe");
        }
        other => panic!("unsupported LIFETRA_SPLIT_BRAIN_PHASE: {other}"),
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
        source_bead: BeadId::new("bead:postgres:split-brain-probe"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:postgres:split-brain-authority".into()],
        issued_at: Timestamp::new(10),
    };
    let binding = IdempotencyBinding::new(&ticket, IDEMPOTENCY_KEY).expect("idempotency binding");
    runtime
        .create_action(&ticket, &binding, OPERATION_REF)
        .expect("create replicated split-brain action");

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
    assert_eq!(record.revision, 2);
    assert_eq!(record.lease.as_ref().expect("lease").revision, 0);
    println!(
        "seed endpoint revision={} epoch={} attempt={}",
        record.revision, token.epoch, permit.attempt_ordinal
    );
}

fn renew_continuity(
    label: &str,
    runtime: &PostgresUnifiedFencedRuntime,
    action_id: &ActionId,
    before_revision: u64,
    before_lease_revision: u64,
    after_revision: u64,
    after_lease_revision: u64,
) {
    let before = verified_record(runtime, action_id);
    assert_eq!(before.revision, before_revision);
    let lease = before.lease.as_ref().expect("replicated lease");
    assert_eq!(lease.revision, before_lease_revision);
    let token = UnifiedFencingToken {
        action_id: action_id.clone(),
        owner: lease.owner.clone(),
        epoch: lease.epoch,
    };
    let renewed = runtime
        .renew(&token, 3_600)
        .expect("leader must continue fenced authority state");
    let after = verified_record(runtime, action_id);

    assert_eq!(renewed.epoch, 1);
    assert_eq!(renewed.revision, after_lease_revision);
    assert_eq!(after.revision, after_revision);
    assert_eq!(after.lease.as_ref().expect("renewed lease"), &renewed);
    println!(
        "{label} revision={} epoch={} lease_revision={}",
        after.revision, renewed.epoch, renewed.revision
    );
}

fn print_verified(
    label: &str,
    runtime: &PostgresUnifiedFencedRuntime,
    action_id: &ActionId,
    expected_revision: u64,
    expected_lease_revision: u64,
) {
    let record = verified_record(runtime, action_id);
    let lease = record.lease.as_ref().expect("lease");
    assert_eq!(record.revision, expected_revision);
    assert_eq!(lease.revision, expected_lease_revision);
    println!(
        "{label} revision={} epoch={} lease_revision={}",
        record.revision, lease.epoch, lease.revision
    );
}

fn verified_record(
    runtime: &PostgresUnifiedFencedRuntime,
    action_id: &ActionId,
) -> UnifiedActionRecord {
    let record = runtime
        .load(action_id)
        .expect("split-brain action must be readable");
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
