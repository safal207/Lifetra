use std::fs;
use std::path::PathBuf;

use lifetra::{
    ActionId, AuthorityTicket, BeadId, DurableJournal, ExecutionMode, IdempotencyBinding,
    ReconciliationOutcome, RecoveryDirective, RetryAuthority, RetryPolicy, RetryVerdict, Timestamp,
};

fn main() {
    let path = PathBuf::from("target/lifetra-durable-journal-example.log");
    if path.exists() {
        fs::remove_file(&path).expect("remove previous example journal");
    }

    let ticket = AuthorityTicket {
        action_id: ActionId::new("action:payout:durable:42").expect("valid action id"),
        source_bead: BeadId::new("bead:payout:durable:42"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:payout:42".into()],
        issued_at: Timestamp::new(100),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:payout:durable:42").expect("valid binding");
    let retry_authority = RetryAuthority::new(RetryPolicy::new(1, false, true));

    let mut journal = DurableJournal::create(&path, ticket, binding).expect("create journal");
    let initial_state = journal.recover().expect("recover initial state");
    let (trace, reconciliation, context) = initial_state
        .ledger
        .retry_inputs()
        .expect("initial retry inputs");
    let initial = retry_authority
        .evaluate(
            &trace,
            &initial_state.ledger.binding,
            reconciliation.as_ref(),
            context,
        )
        .expect("initial dispatch decision");
    assert_eq!(initial.verdict, RetryVerdict::InitialDispatchAllowed);

    journal
        .prepare_attempt(&initial, Timestamp::new(101))
        .expect("write-ahead prepare must be durable before dispatch");

    // External dispatch happens here. The local dispatch receipt is then made durable.
    journal
        .record_dispatch(0, Timestamp::new(102), "proof:dispatch:durable:0")
        .expect("durable dispatch receipt");

    // Simulate process restart.
    drop(journal);
    let mut journal = DurableJournal::open(&path).expect("reopen journal");
    let after_restart = journal.recover().expect("replay durable state");
    assert_eq!(
        after_restart.directive,
        RecoveryDirective::ReconcileDispatchedAttempt { ordinal: 0 }
    );

    // Provider reconciliation proves the first dispatch produced no effect.
    journal
        .record_reconciliation(
            0,
            Timestamp::new(110),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:provider:no-effect:0",
        )
        .expect("durable reconciliation evidence");

    let recovered = journal.recover().expect("recover reconciled state");
    assert_eq!(
        recovered.directive,
        RecoveryDirective::EvaluateRetry { ordinal: 0 }
    );
    let (trace, reconciliation, context) = recovered
        .ledger
        .retry_inputs()
        .expect("retry inputs from durable history");
    let retry = retry_authority
        .evaluate(
            &trace,
            &recovered.ledger.binding,
            reconciliation.as_ref(),
            context,
        )
        .expect("bounded retry decision");
    assert_eq!(
        retry.verdict,
        RetryVerdict::RedispatchAllowed { ordinal: 1 }
    );

    println!("action_id={}", retry.action_id.as_str());
    println!("recovery={:?}", recovered.directive);
    println!("retry={:?}", retry.verdict);

    drop(journal);
    fs::remove_file(path).expect("cleanup example journal");
}
