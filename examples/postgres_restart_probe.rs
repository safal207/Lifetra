use lifetra::{
    ActionId, AuthorityTicket, BeadId, ExecutionMode, IdempotencyBinding,
    PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore, RecoveryWorkerId, Timestamp,
};

const ACTION_ID: &str = "action:postgres:restart-probe";
const IDEMPOTENCY_KEY: &str = "idem:postgres:restart-probe";
const OPERATION_REF: &str = "operation:postgres:restart-probe";

fn main() {
    let dsn = std::env::var("LIFETRA_POSTGRES_URL")
        .expect("set LIFETRA_POSTGRES_URL for the restart probe");
    let phase = std::env::var("LIFETRA_RESTART_PHASE").unwrap_or_else(|_| "verify".into());
    let store = PostgresUnifiedFencedStore::new(dsn);
    store.migrate().expect("migrate PostgreSQL schema");
    let action_id = ActionId::new(ACTION_ID).expect("stable action id");

    match phase.as_str() {
        "seed" => {
            store.delete_action(&action_id).ok();
            let ticket = AuthorityTicket {
                action_id: action_id.clone(),
                source_bead: BeadId::new("bead:postgres:restart-probe"),
                execution_mode: ExecutionMode::Automatic,
                authority_proof_refs: vec!["proof:postgres:restart-probe".into()],
                issued_at: Timestamp::new(10),
            };
            let binding =
                IdempotencyBinding::new(&ticket, IDEMPOTENCY_KEY).expect("idempotency binding");
            let runtime = PostgresUnifiedFencedRuntime::new(store);
            runtime
                .create_action(&ticket, &binding, OPERATION_REF)
                .expect("create durable restart probe");
            let token = runtime
                .acquire(
                    &action_id,
                    RecoveryWorkerId::new("worker:restart-probe").expect("worker"),
                    3_600,
                )
                .expect("acquire durable lease");
            let record = runtime.load(&action_id).expect("load seeded record");
            assert_eq!(record.binding.operation_ref, OPERATION_REF);
            assert_eq!(record.lease.as_ref().expect("lease").epoch, token.epoch);
            println!("seeded revision={} epoch={}", record.revision, token.epoch);
        }
        "verify" => {
            let runtime = PostgresUnifiedFencedRuntime::new(store.clone());
            let record = runtime
                .load(&action_id)
                .expect("record must survive PostgreSQL restart");
            assert_eq!(record.binding.action_id, action_id);
            assert_eq!(record.binding.idempotency_key, IDEMPOTENCY_KEY);
            assert_eq!(record.binding.operation_ref, OPERATION_REF);
            assert!(record.revision >= 1);
            assert_eq!(record.lease.as_ref().expect("durable lease").epoch, 1);
            println!(
                "verified revision={} epoch={}",
                record.revision,
                record.lease.as_ref().expect("lease").epoch
            );
            store.delete_action(&action_id).ok();
        }
        other => panic!("unsupported LIFETRA_RESTART_PHASE: {other}"),
    }
}
