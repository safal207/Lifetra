use lifetra::{
    classify_postgres_sqlstate, ActionId, AuthorityTicket, BeadId, ExecutionMode,
    IdempotencyBinding, PostgresCommitResolution, PostgresFailureDisposition, PostgresFailurePhase,
    PostgresTransactionRetryPolicy, PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore,
    RecoveryWorkerId, Timestamp, UnifiedFencedStore, UnifiedLeaseAuthority,
};

fn main() {
    assert_eq!(
        classify_postgres_sqlstate(Some("40001"), PostgresFailurePhase::Write),
        PostgresFailureDisposition::RetryableAbortedTransaction
    );
    assert_eq!(
        classify_postgres_sqlstate(None, PostgresFailurePhase::Commit),
        PostgresFailureDisposition::CommitOutcomeUnknown
    );

    let Ok(dsn) = std::env::var("LIFETRA_POSTGRES_URL") else {
        println!("set LIFETRA_POSTGRES_URL to run the PostgreSQL recovery example");
        return;
    };

    let store = PostgresUnifiedFencedStore::new(dsn)
        .with_retry_policy(PostgresTransactionRetryPolicy::new(3));
    store.migrate().expect("migrate PostgreSQL schema");

    let action_id = ActionId::new(format!("action:postgres:recovery:{}", std::process::id()))
        .expect("valid action id");
    store.delete_action(&action_id).ok();
    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:postgres:recovery"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:postgres-recovery".into()],
        issued_at: Timestamp::new(10),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:postgres:recovery").expect("valid binding");
    let runtime = PostgresUnifiedFencedRuntime::new(store);
    runtime
        .create_action(&ticket, &binding, "operation:postgres:recovery")
        .expect("create action record");

    let store = runtime.store();
    let current = store.load(&action_id).expect("load").expect("row");
    let mut replacement = current.clone();
    replacement.revision += 1;
    replacement.lease = Some(UnifiedLeaseAuthority {
        owner: RecoveryWorkerId::new("worker:postgres-recovery").expect("worker"),
        epoch: 1,
        revision: 0,
        expires_at: Timestamp::new(i64::MAX / 4),
    });

    // Pretend the caller loses only the COMMIT acknowledgement. We intentionally
    // ignore the successful return and reconstruct the outcome from durable state.
    store
        .compare_and_swap(&action_id, Some(current.revision), replacement.clone())
        .expect("commit replacement");
    let resolution = store
        .resolve_commit_outcome(&action_id, Some(current.revision), &replacement)
        .expect("resolve ambiguous acknowledgement");
    assert_eq!(resolution, PostgresCommitResolution::Applied);

    println!("retry_policy={:?}", store.retry_policy());
    println!("commit_resolution={resolution:?}");

    store.delete_action(&action_id).ok();
}
