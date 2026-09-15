use std::fs;

use lifetra::{
    ActionId, AuthorityTicket, BeadId, DurableJournal, ExecutionMode, FencedDurableJournal,
    FencedJournalError, IdempotencyBinding, InMemoryRecoveryLeaseStore, RecoveryLeaseBlock,
    RecoveryLeaseManager, RecoveryWorkerId, RetryDecision, RetryReason, RetryVerdict, Timestamp,
};

fn main() {
    let path = std::env::temp_dir().join(format!(
        "lifetra-recovery-fencing-{}.journal",
        std::process::id()
    ));
    fs::remove_file(&path).ok();

    let action_id = ActionId::new("action:recovery:fencing:demo").expect("valid action id");
    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:recovery:fencing:demo"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:fencing-demo".into()],
        issued_at: Timestamp::new(10),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:recovery:fencing:demo").expect("valid binding");
    let initial = RetryDecision {
        action_id: action_id.clone(),
        idempotency_key: binding.key.clone(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: ticket.authority_proof_refs.clone(),
    };

    let journal = DurableJournal::create(&path, ticket, binding).expect("journal should create");
    let store = InMemoryRecoveryLeaseStore::default();
    let worker_a = RecoveryWorkerId::new("worker-a").expect("valid worker");
    let mut fenced =
        FencedDurableJournal::acquire(journal, store.clone(), worker_a, Timestamp::new(100), 10)
            .expect("worker A should acquire epoch 1");

    let permit = fenced
        .prepare_attempt(&initial, Timestamp::new(101), Timestamp::new(101))
        .expect("current worker may prepare");
    assert_eq!(permit.fencing_epoch, 1);

    // Worker A stalls. After expiry, worker B acquires a strictly newer epoch.
    let worker_b = RecoveryWorkerId::new("worker-b").expect("valid worker");
    let takeover = RecoveryLeaseManager::new(store)
        .acquire(&action_id, worker_b, Timestamp::new(111), 20)
        .expect("worker B should acquire after expiry");
    assert_eq!(takeover.epoch, 2);

    // Worker A wakes up with its old permit. Its local journal mutation is fenced.
    let stale = fenced.record_dispatch(
        &permit,
        Timestamp::new(112),
        "proof:dispatch:stale-worker",
        Timestamp::new(112),
    );
    assert!(matches!(
        stale,
        Err(FencedJournalError::Lease(RecoveryLeaseBlock::StaleFence {
            presented_epoch: 1,
            current_epoch: 2,
        }))
    ));

    println!("worker_a_epoch={}", permit.fencing_epoch);
    println!("worker_b_epoch={}", takeover.epoch);
    println!("stale_worker_dispatch_blocked=true");

    fs::remove_file(path).ok();
}
