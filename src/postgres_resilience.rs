use postgres::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresFailurePhase {
    Connect,
    Begin,
    Read,
    Write,
    Commit,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresFailureDisposition {
    /// PostgreSQL guarantees the transaction was aborted. Re-running from the
    /// previous durable revision is safe.
    RetryableAbortedTransaction,
    /// The failure happened before COMMIT was sent/observed. The open
    /// transaction cannot commit after the connection is gone, so a fresh
    /// transaction may safely retry.
    RetryableBeforeCommit,
    /// COMMIT may have reached PostgreSQL but its acknowledgement was lost.
    /// Reload/reconcile state before deciding whether another write is safe.
    CommitOutcomeUnknown,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostgresTransactionRetryPolicy {
    pub max_attempts: u32,
}

impl PostgresTransactionRetryPolicy {
    pub const fn new(max_attempts: u32) -> Self {
        Self { max_attempts }
    }

    pub const fn normalized(self) -> Self {
        if self.max_attempts == 0 {
            Self { max_attempts: 1 }
        } else {
            self
        }
    }
}

impl Default for PostgresTransactionRetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 3 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostgresCommitResolution {
    Applied,
    NotApplied,
    Contended,
    Unknown { observed_revision: Option<u64> },
}

pub fn classify_postgres_failure(
    error: &Error,
    phase: PostgresFailurePhase,
) -> PostgresFailureDisposition {
    let code = error.code().map(|code| code.code());

    // PostgreSQL documents serialization failures and deadlocks as aborted
    // transactions. The client must retry the transaction from the beginning.
    if matches!(code, Some("40001" | "40P01")) {
        return PostgresFailureDisposition::RetryableAbortedTransaction;
    }

    let connection_like = code.is_none()
        || code.is_some_and(|code| {
            code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03")
        });

    if connection_like {
        return if phase == PostgresFailurePhase::Commit {
            PostgresFailureDisposition::CommitOutcomeUnknown
        } else {
            PostgresFailureDisposition::RetryableBeforeCommit
        };
    }

    PostgresFailureDisposition::Fatal
}
