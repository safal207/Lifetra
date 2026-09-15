use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use lifetra::{
    ActionId, AuthorityTicket, BeadId, ExecutionMode, IdempotencyBinding,
    PostgresTransactionRetryPolicy, PostgresUnifiedFencedRuntime, PostgresUnifiedFencedStore,
    RecoveryWorkerId, Timestamp, UnifiedActionRecord, UnifiedFencedStore, UnifiedLeaseAuthority,
};

// PostgreSQL frontend Simple Query frame for exactly `COMMIT\0`:
// Q + int32(len = 4 + 7) + payload.
const FRONTEND_COMMIT_FRAME: &[u8] = b"Q\x00\x00\x00\x0bCOMMIT\x00";
// PostgreSQL backend CommandComplete frame for exactly `COMMIT\0`:
// C + int32(len = 4 + 7) + tag.
const BACKEND_COMMIT_COMPLETE_FRAME: &[u8] = b"C\x00\x00\x00\x0bCOMMIT\x00";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyFaultMode {
    DropBeforeCommitOnce,
    DropCommitAckOnce,
}

struct PostgresFaultProxy {
    listen_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    injected: Arc<AtomicUsize>,
    join: Option<JoinHandle<()>>,
}

impl PostgresFaultProxy {
    fn start(target: String, mode: ProxyFaultMode) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let listen_addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let injected = Arc::new(AtomicUsize::new(0));
        let fault_available = Arc::new(AtomicBool::new(true));

        let stop_thread = Arc::clone(&stop);
        let injected_thread = Arc::clone(&injected);
        let fault_thread = Arc::clone(&fault_available);
        let join = thread::spawn(move || {
            while !stop_thread.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let target = target.clone();
                        let fault_available = Arc::clone(&fault_thread);
                        let injected = Arc::clone(&injected_thread);
                        thread::spawn(move || {
                            let _ =
                                handle_connection(client, &target, mode, fault_available, injected);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            listen_addr,
            stop,
            injected,
            join: Some(join),
        })
    }

    fn dsn(&self, original: &str) -> Option<String> {
        let authority = dsn_authority(original)?;
        Some(original.replacen(&authority, &self.listen_addr.to_string(), 1))
    }

    fn injected_count(&self) -> usize {
        self.injected.load(Ordering::Acquire)
    }
}

impl Drop for PostgresFaultProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.listen_addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_connection(
    client: TcpStream,
    target: &str,
    mode: ProxyFaultMode,
    fault_available: Arc<AtomicBool>,
    injected: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let server = TcpStream::connect(target)?;
    client.set_nodelay(true)?;
    server.set_nodelay(true)?;

    let mut client_read = client.try_clone()?;
    let mut client_write = client;
    let mut server_read = server.try_clone()?;
    let mut server_write = server;
    let commit_request_forwarded = Arc::new(AtomicBool::new(false));
    let commit_request_for_client_to_server = Arc::clone(&commit_request_forwarded);
    let fault_for_client_to_server = Arc::clone(&fault_available);
    let injected_client_to_server = Arc::clone(&injected);

    let client_to_server = thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        let mut commit_request_tail = Vec::new();
        let mut update_tail = Vec::new();
        loop {
            let count = match client_read.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => break,
            };
            let bytes = &buffer[..count];

            if mode == ProxyFaultMode::DropBeforeCommitOnce
                && contains_pattern(&mut update_tail, bytes, b"UPDATE ")
                && fault_for_client_to_server
                    .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                injected_client_to_server.fetch_add(1, Ordering::AcqRel);
                let _ = client_read.shutdown(Shutdown::Both);
                let _ = server_write.shutdown(Shutdown::Both);
                break;
            }

            if server_write.write_all(bytes).is_err() {
                break;
            }

            // Do not arm the acknowledgement cut on an arbitrary ASCII `COMMIT`
            // substring. Arm only after the exact frontend Simple Query COMMIT frame
            // has been forwarded to PostgreSQL.
            if mode == ProxyFaultMode::DropCommitAckOnce
                && contains_pattern(&mut commit_request_tail, bytes, FRONTEND_COMMIT_FRAME)
            {
                commit_request_for_client_to_server.store(true, Ordering::Release);
            }
        }
    });

    let fault_for_server_to_client = Arc::clone(&fault_available);
    let injected_server_to_client = Arc::clone(&injected);
    let mut commit_complete_tail = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = match server_read.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => break,
        };
        let bytes = &buffer[..count];

        // A connection cut is a genuine lost COMMIT acknowledgement only after:
        //   1) the exact COMMIT request was forwarded, and
        //   2) PostgreSQL emitted the exact CommandComplete("COMMIT") frame.
        // Consuming that frame from the server socket establishes the server-side
        // commit boundary before deliberately withholding it from the client.
        let commit_complete = mode == ProxyFaultMode::DropCommitAckOnce
            && commit_request_forwarded.load(Ordering::Acquire)
            && contains_pattern(
                &mut commit_complete_tail,
                bytes,
                BACKEND_COMMIT_COMPLETE_FRAME,
            );

        if commit_complete
            && fault_for_server_to_client
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            injected_server_to_client.fetch_add(1, Ordering::AcqRel);
            let _ = server_read.shutdown(Shutdown::Both);
            let _ = client_write.shutdown(Shutdown::Both);
            break;
        }

        if client_write.write_all(bytes).is_err() {
            break;
        }
    }

    let _ = client_to_server.join();
    Ok(())
}

fn contains_pattern(tail: &mut Vec<u8>, chunk: &[u8], pattern: &[u8]) -> bool {
    let mut combined = Vec::with_capacity(tail.len() + chunk.len());
    combined.extend_from_slice(tail);
    combined.extend_from_slice(chunk);
    let found = combined
        .windows(pattern.len())
        .any(|window| window == pattern);
    let keep = pattern.len().saturating_sub(1).min(combined.len());
    tail.clear();
    tail.extend_from_slice(&combined[combined.len() - keep..]);
    found
}

fn dsn_authority(dsn: &str) -> Option<String> {
    let (_, after_at) = dsn.rsplit_once('@')?;
    Some(after_at.split('/').next()?.to_owned())
}

fn target_from_dsn(dsn: &str) -> Option<String> {
    let authority = dsn_authority(dsn)?;
    if authority.contains(':') {
        Some(authority)
    } else {
        Some(format!("{authority}:5432"))
    }
}

fn test_dsn() -> Option<String> {
    std::env::var("LIFETRA_TEST_POSTGRES_URL").ok()
}

fn seed_action(
    dsn: &str,
    suffix: &str,
) -> (
    PostgresUnifiedFencedStore,
    AuthorityTicket,
    UnifiedActionRecord,
) {
    let store = PostgresUnifiedFencedStore::new(dsn.to_owned());
    store.migrate().expect("migration");
    let action_id = ActionId::new(format!(
        "action:postgres-network:{suffix}:{}",
        std::process::id()
    ))
    .expect("action id");
    store.delete_action(&action_id).ok();
    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead: BeadId::new("bead:postgres-network"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:postgres-network:authority".into()],
        issued_at: Timestamp::new(10),
    };
    let binding =
        IdempotencyBinding::new(&ticket, format!("idem:{suffix}")).expect("idempotency binding");
    let runtime = PostgresUnifiedFencedRuntime::new(store.clone());
    runtime
        .create_action(&ticket, &binding, format!("operation:{suffix}"))
        .expect("create action");
    let current = store.load(&action_id).expect("load").expect("seed row");
    (store, ticket, current)
}

fn lease_replacement(current: &UnifiedActionRecord, worker: &str) -> UnifiedActionRecord {
    let mut replacement = current.clone();
    replacement.revision += 1;
    replacement.lease = Some(UnifiedLeaseAuthority {
        owner: RecoveryWorkerId::new(worker).expect("worker"),
        epoch: 1,
        revision: 0,
        expires_at: Timestamp::new(i64::MAX / 4),
    });
    replacement
}

#[test]
fn real_commit_ack_loss_reconciles_durable_replacement() {
    let Some(dsn) = test_dsn() else {
        return;
    };
    let Some(target) = target_from_dsn(&dsn) else {
        return;
    };
    let (direct, ticket, current) = seed_action(&dsn, "commit-ack-drop");
    let replacement = lease_replacement(&current, "worker-commit-ack-drop");
    let proxy =
        PostgresFaultProxy::start(target, ProxyFaultMode::DropCommitAckOnce).expect("fault proxy");
    let proxy_dsn = proxy.dsn(&dsn).expect("proxy dsn");
    let proxied = PostgresUnifiedFencedStore::new(proxy_dsn)
        .with_retry_policy(PostgresTransactionRetryPolicy::new(2));

    assert!(proxied
        .compare_and_swap(
            &ticket.action_id,
            Some(current.revision),
            replacement.clone(),
        )
        .expect("commit acknowledgement loss should reconcile"));
    assert_eq!(proxy.injected_count(), 1);
    assert_eq!(
        direct
            .load(&ticket.action_id)
            .expect("direct load")
            .expect("durable row"),
        replacement
    );
    direct.delete_action(&ticket.action_id).ok();
}

#[test]
fn real_pre_commit_connection_loss_retries_fresh_transaction() {
    let Some(dsn) = test_dsn() else {
        return;
    };
    let Some(target) = target_from_dsn(&dsn) else {
        return;
    };
    let (direct, ticket, current) = seed_action(&dsn, "pre-commit-drop");
    let replacement = lease_replacement(&current, "worker-pre-commit-drop");
    let proxy = PostgresFaultProxy::start(target, ProxyFaultMode::DropBeforeCommitOnce)
        .expect("fault proxy");
    let proxy_dsn = proxy.dsn(&dsn).expect("proxy dsn");
    let proxied = PostgresUnifiedFencedStore::new(proxy_dsn)
        .with_retry_policy(PostgresTransactionRetryPolicy::new(3));

    assert!(proxied
        .compare_and_swap(
            &ticket.action_id,
            Some(current.revision),
            replacement.clone(),
        )
        .expect("pre-COMMIT connection loss should retry"));
    assert_eq!(proxy.injected_count(), 1);
    assert_eq!(
        direct
            .load(&ticket.action_id)
            .expect("direct load")
            .expect("durable row"),
        replacement
    );
    direct.delete_action(&ticket.action_id).ok();
}
