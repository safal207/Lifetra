use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{ExecutionMode, RetryAuthority, RetryPolicy, RetryReason};

use super::*;

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "lifetra-journal-{name}-{}-{nonce}.log",
        std::process::id()
    ))
}

fn ticket() -> AuthorityTicket {
    AuthorityTicket {
        action_id: ActionId::new("action:journal:1").expect("valid action id"),
        source_bead: BeadId::new("bead:journal:1"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:journal".into()],
        issued_at: Timestamp::new(10),
    }
}

fn create_journal(path: &Path) -> DurableJournal {
    let ticket = ticket();
    let binding = IdempotencyBinding::new(&ticket, "idem:journal:1").expect("valid binding");
    DurableJournal::create(path, ticket, binding).expect("journal should create")
}

fn initial_decision(journal: &DurableJournal) -> RetryDecision {
    RetryDecision {
        action_id: journal.ticket().action_id.clone(),
        idempotency_key: journal.binding().key.clone(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: journal.ticket().authority_proof_refs.clone(),
    }
}

#[test]
fn prepared_without_dispatch_recovers_to_reconcile_not_blind_send() {
    let path = temp_path("prepared");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare should fsync");
    drop(journal);

    let reopened = DurableJournal::open(&path).expect("journal should reopen");
    let recovered = reopened.recover().expect("journal should replay");
    assert_eq!(recovered.ledger.attempts().len(), 0);
    assert_eq!(
        recovered.directive,
        RecoveryDirective::ReconcilePreparedAttempt { ordinal: 0 }
    );
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn reopened_prepared_attempt_cannot_be_marked_dispatched_without_reconciliation() {
    let path = temp_path("prepared-guard");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare should fsync");
    drop(journal);

    let mut reopened = DurableJournal::open(&path).expect("journal should reopen");
    assert_eq!(
        reopened.record_dispatch(0, Timestamp::new(12), "proof:dispatch:0"),
        Err(JournalError::RecoveredPreparedRequiresReconciliation { ordinal: 0 })
    );
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn durable_dispatch_rebuilds_attempt_ledger_after_restart() {
    let path = temp_path("dispatch");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare");
    journal
        .record_dispatch(0, Timestamp::new(12), "proof:dispatch:0")
        .expect("dispatch receipt");
    drop(journal);

    let reopened = DurableJournal::open(&path).expect("open");
    let recovered = reopened.recover().expect("recover");
    assert_eq!(recovered.ledger.attempts().len(), 1);
    assert_eq!(recovered.ledger.attempts()[0].id.ordinal, 0);
    assert_eq!(recovered.ledger.binding.key, "idem:journal:1");
    assert_eq!(
        recovered.directive,
        RecoveryDirective::ReconcileDispatchedAttempt { ordinal: 0 }
    );
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn confirmed_success_recovers_closed() {
    let path = temp_path("success");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare");
    journal
        .record_dispatch(0, Timestamp::new(12), "proof:dispatch:0")
        .expect("dispatch");
    journal
        .record_external_outcome(
            0,
            Timestamp::new(13),
            ExecutionOutcome::Succeeded,
            "proof:success:0",
        )
        .expect("external outcome");
    drop(journal);

    let reopened = DurableJournal::open(&path).expect("open");
    assert_eq!(
        reopened.recover().expect("recover").directive,
        RecoveryDirective::CloseSucceeded { ordinal: 0 }
    );
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn no_effect_recovery_feeds_existing_retry_authority() {
    let path = temp_path("retry");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare");
    journal
        .record_dispatch(0, Timestamp::new(12), "proof:dispatch:0")
        .expect("dispatch");
    journal
        .record_reconciliation(
            0,
            Timestamp::new(13),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:no-effect:0",
        )
        .expect("reconciliation");
    drop(journal);

    let mut reopened = DurableJournal::open(&path).expect("open");
    let recovered = reopened.recover().expect("recover");
    assert_eq!(
        recovered.directive,
        RecoveryDirective::EvaluateRetry { ordinal: 0 }
    );
    let (trace, reconciliation, context) = recovered
        .ledger
        .retry_inputs()
        .expect("retry inputs should rebuild");
    let retry = RetryAuthority::new(RetryPolicy::new(1, false, true))
        .evaluate(
            &trace,
            &recovered.ledger.binding,
            reconciliation.as_ref(),
            context,
        )
        .expect("retry decision");
    assert_eq!(
        retry.verdict,
        RetryVerdict::RedispatchAllowed { ordinal: 1 }
    );
    reopened
        .prepare_attempt(&retry, Timestamp::new(14))
        .expect("second attempt should prepare durably");
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn incomplete_tail_is_removed_and_keeps_recovery_fail_closed() {
    let path = temp_path("partial-tail");
    let mut journal = create_journal(&path);
    let initial = initial_decision(&journal);
    journal
        .prepare_attempt(&initial, Timestamp::new(11))
        .expect("prepare");
    drop(journal);

    let mut raw = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("append test corruption");
    raw.write_all(b"2\tdeadbeef\tLJ1\tDISP\t0")
        .expect("write partial tail");
    raw.sync_all().expect("sync partial tail");
    drop(raw);

    let reopened = DurableJournal::open(&path).expect("open should repair partial tail");
    let recovered = reopened.recover().expect("recover");
    assert!(recovered.repaired_truncated_tail);
    assert_eq!(
        recovered.directive,
        RecoveryDirective::ReconcilePreparedAttempt { ordinal: 0 }
    );
    drop(reopened);
    fs::remove_file(path).expect("cleanup");
}

#[test]
fn complete_checksum_corruption_is_not_silently_ignored() {
    let path = temp_path("checksum");
    let journal = create_journal(&path);
    drop(journal);

    let mut bytes = fs::read(&path).expect("read journal");
    let first_tab = bytes.iter().position(|byte| *byte == b'\t').expect("tab");
    let checksum_start = first_tab + 1;
    bytes[checksum_start] = if bytes[checksum_start] == b'0' {
        b'1'
    } else {
        b'0'
    };
    fs::write(&path, bytes).expect("corrupt checksum");

    assert!(matches!(
        DurableJournal::open(&path),
        Err(JournalError::InvalidChecksum { sequence: 0 })
    ));
    fs::remove_file(path).expect("cleanup");
}
