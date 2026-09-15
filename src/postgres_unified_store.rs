use postgres::{Client, NoTls, Row};

use lifetra_core::Timestamp;

use crate::postgres_resilience::{
    classify_postgres_failure, PostgresCommitResolution, PostgresFailureDisposition,
    PostgresFailurePhase, PostgresTransactionRetryPolicy,
};
use crate::{
    ActionId, AuthorityTicket, FencedActuatorReceipt, FencedAttemptPermit, IdempotencyBinding,
    RecoveryWorkerId, RetryDecision, UnifiedActionRecord, UnifiedDispatchEvidence,
    UnifiedEffectEvidence, UnifiedEffectOutcome, UnifiedEvidenceCommit, UnifiedEvidenceSource,
    UnifiedFencedBlock, UnifiedFencedRuntime, UnifiedFencedStore, UnifiedFencingToken,
    UnifiedLeaseAuthority, UnifiedOperationBinding, UnifiedPreparedAttempt,
    UnifiedProjectionMarker, UnifiedReconciliationObservation, UnifiedRuntimeDirective,
};

const CODEC_VERSION: &str = "PGU1";
const DEFAULT_TABLE: &str = "lifetra_unified_actions";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostgresUnifiedStoreError {
    Postgres(String),
    InvalidTableName,
    InvalidRecord,
    InvalidNumber,
    InvalidHex,
    InvalidUtf8,
    InvalidActionId,
    InvalidWorkerId,
    RevisionOutOfRange,
    InvalidTransition,
    DatabaseLeaseExpired {
        epoch: u64,
    },
    DatabaseLeaseNotExpired {
        epoch: u64,
    },
    TransactionRetryExhausted {
        attempts: u32,
        last_error: String,
    },
    CommitOutcomeUnknown {
        expected_revision: Option<u64>,
        observed_revision: Option<u64>,
        last_error: String,
    },
}

impl From<postgres::Error> for PostgresUnifiedStoreError {
    fn from(value: postgres::Error) -> Self {
        Self::Postgres(value.to_string())
    }
}

enum PostgresCasAttemptError {
    Postgres {
        phase: PostgresFailurePhase,
        error: postgres::Error,
    },
    Store(PostgresUnifiedStoreError),
}

/// PostgreSQL implementation of `UnifiedFencedStore`.
///
/// Every CAS opens a real PostgreSQL transaction, locks the action row with
/// `SELECT ... FOR UPDATE`, reads `clock_timestamp()` inside that transaction,
/// validates the control transition against database time, and commits the whole
/// replacement record in one update. No process-local mutex participates in the
/// multi-worker safety contract.
#[derive(Debug, Clone)]
pub struct PostgresUnifiedFencedStore {
    connection_string: String,
    table: String,
    retry_policy: PostgresTransactionRetryPolicy,
}

impl PostgresUnifiedFencedStore {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            table: DEFAULT_TABLE.to_owned(),
            retry_policy: PostgresTransactionRetryPolicy::default(),
        }
    }

    pub fn with_table(
        connection_string: impl Into<String>,
        table: impl Into<String>,
    ) -> Result<Self, PostgresUnifiedStoreError> {
        let table = table.into();
        validate_identifier(&table)?;
        Ok(Self {
            connection_string: connection_string.into(),
            table,
            retry_policy: PostgresTransactionRetryPolicy::default(),
        })
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn with_retry_policy(mut self, policy: PostgresTransactionRetryPolicy) -> Self {
        self.retry_policy = policy.normalized();
        self
    }

    pub fn retry_policy(&self) -> PostgresTransactionRetryPolicy {
        self.retry_policy
    }

    pub fn resolve_commit_outcome(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: &UnifiedActionRecord,
    ) -> Result<PostgresCommitResolution, PostgresUnifiedStoreError> {
        let observed = <Self as UnifiedFencedStore>::load(self, action_id)?;
        match (expected_revision, observed) {
            (None, None) => Ok(PostgresCommitResolution::NotApplied),
            (None, Some(record)) if record == *replacement => Ok(PostgresCommitResolution::Applied),
            (None, Some(_)) => Ok(PostgresCommitResolution::Contended),
            (Some(expected), Some(record)) if record == *replacement => {
                Ok(PostgresCommitResolution::Applied)
            }
            (Some(expected), Some(record)) if record.revision == expected => {
                Ok(PostgresCommitResolution::NotApplied)
            }
            (Some(_), Some(record)) if record.revision == replacement.revision => {
                Ok(PostgresCommitResolution::Contended)
            }
            (Some(_), Some(record)) if record.revision > replacement.revision => {
                Ok(PostgresCommitResolution::Unknown {
                    observed_revision: Some(record.revision),
                })
            }
            (_, Some(record)) => Ok(PostgresCommitResolution::Unknown {
                observed_revision: Some(record.revision),
            }),
            (Some(_), None) => Ok(PostgresCommitResolution::Unknown {
                observed_revision: None,
            }),
        }
    }

    pub fn compare_and_swap_resilient(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: UnifiedActionRecord,
    ) -> Result<bool, PostgresUnifiedStoreError> {
        let max_attempts = self.retry_policy.normalized().max_attempts;
        let mut last_error = String::new();

        for attempt in 1..=max_attempts {
            match self.compare_and_swap_once(action_id, expected_revision, &replacement) {
                Ok(applied) => return Ok(applied),
                Err(PostgresCasAttemptError::Store(error)) => return Err(error),
                Err(PostgresCasAttemptError::Postgres { phase, error }) => {
                    last_error = error.to_string();
                    match classify_postgres_failure(&error, phase) {
                        PostgresFailureDisposition::RetryableAbortedTransaction
                        | PostgresFailureDisposition::RetryableBeforeCommit => {
                            if attempt < max_attempts {
                                std::thread::yield_now();
                                continue;
                            }
                            return Err(PostgresUnifiedStoreError::TransactionRetryExhausted {
                                attempts: attempt,
                                last_error,
                            });
                        }
                        PostgresFailureDisposition::CommitOutcomeUnknown => {
                            match self.resolve_commit_outcome(
                                action_id,
                                expected_revision,
                                &replacement,
                            ) {
                                Ok(PostgresCommitResolution::Applied) => return Ok(true),
                                Ok(PostgresCommitResolution::Contended) => return Ok(false),
                                Ok(PostgresCommitResolution::NotApplied)
                                    if attempt < max_attempts =>
                                {
                                    std::thread::yield_now();
                                    continue;
                                }
                                Ok(PostgresCommitResolution::NotApplied) => {
                                    return Err(
                                        PostgresUnifiedStoreError::TransactionRetryExhausted {
                                            attempts: attempt,
                                            last_error,
                                        },
                                    );
                                }
                                Ok(PostgresCommitResolution::Unknown { observed_revision }) => {
                                    return Err(PostgresUnifiedStoreError::CommitOutcomeUnknown {
                                        expected_revision,
                                        observed_revision,
                                        last_error,
                                    });
                                }
                                Err(reconcile_error) => {
                                    return Err(PostgresUnifiedStoreError::CommitOutcomeUnknown {
                                        expected_revision,
                                        observed_revision: None,
                                        last_error: format!(
                                            "{last_error}; reconciliation failed: {reconcile_error:?}"
                                        ),
                                    });
                                }
                            }
                        }
                        PostgresFailureDisposition::Fatal => {
                            return Err(PostgresUnifiedStoreError::Postgres(last_error));
                        }
                    }
                }
            }
        }

        Err(PostgresUnifiedStoreError::TransactionRetryExhausted {
            attempts: max_attempts,
            last_error,
        })
    }

    fn compare_and_swap_once(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: &UnifiedActionRecord,
    ) -> Result<bool, PostgresCasAttemptError> {
        if replacement.binding.action_id != *action_id {
            return Err(PostgresCasAttemptError::Store(
                PostgresUnifiedStoreError::InvalidTransition,
            ));
        }
        let replacement_revision =
            i64_revision(replacement.revision).map_err(PostgresCasAttemptError::Store)?;
        let payload = encode_record(replacement);
        let mut client = Client::connect(&self.connection_string, NoTls).map_err(|error| {
            PostgresCasAttemptError::Postgres {
                phase: PostgresFailurePhase::Connect,
                error,
            }
        })?;
        let mut tx = client
            .transaction()
            .map_err(|error| PostgresCasAttemptError::Postgres {
                phase: PostgresFailurePhase::Begin,
                error,
            })?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .map_err(|error| PostgresCasAttemptError::Postgres {
                phase: PostgresFailurePhase::Begin,
                error,
            })?;

        match expected_revision {
            None => {
                if replacement.revision != 0 {
                    return Err(PostgresCasAttemptError::Store(
                        PostgresUnifiedStoreError::InvalidTransition,
                    ));
                }
                let changed = tx
                    .execute(
                        &format!(
                            "INSERT INTO {} (action_id, revision, payload, updated_at) \
                             VALUES ($1, $2, $3, clock_timestamp()) \
                             ON CONFLICT (action_id) DO NOTHING",
                            quoted_identifier(&self.table)
                        ),
                        &[&action_id.as_str(), &replacement_revision, &payload],
                    )
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Write,
                        error,
                    })?;
                if changed != 1 {
                    return Ok(false);
                }
                tx.commit()
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Commit,
                        error,
                    })?;
                Ok(true)
            }
            Some(expected) => {
                let row = tx
                    .query_opt(
                        &format!(
                            "SELECT revision, payload FROM {} WHERE action_id = $1 FOR UPDATE",
                            quoted_identifier(&self.table)
                        ),
                        &[&action_id.as_str()],
                    )
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Read,
                        error,
                    })?;
                let Some(row) = row else {
                    return Ok(false);
                };
                let current = decode_row(row).map_err(PostgresCasAttemptError::Store)?;
                if current.revision != expected {
                    return Ok(false);
                }
                let now_row = tx
                    .query_one(
                        "SELECT FLOOR(EXTRACT(EPOCH FROM clock_timestamp()))::BIGINT",
                        &[],
                    )
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Read,
                        error,
                    })?;
                let db_now = Timestamp::new(now_row.get::<_, i64>(0));
                validate_database_transition(&current, replacement, db_now)
                    .map_err(PostgresCasAttemptError::Store)?;

                let changed = tx
                    .execute(
                        &format!(
                            "UPDATE {} SET revision = $2, payload = $3, updated_at = clock_timestamp() \
                             WHERE action_id = $1 AND revision = $4",
                            quoted_identifier(&self.table)
                        ),
                        &[
                            &action_id.as_str(),
                            &replacement_revision,
                            &payload,
                            &i64_revision(expected).map_err(PostgresCasAttemptError::Store)?,
                        ],
                    )
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Write,
                        error,
                    })?;
                if changed != 1 {
                    return Ok(false);
                }
                tx.commit()
                    .map_err(|error| PostgresCasAttemptError::Postgres {
                        phase: PostgresFailurePhase::Commit,
                        error,
                    })?;
                Ok(true)
            }
        }
    }

    pub fn migrate(&self) -> Result<(), PostgresUnifiedStoreError> {
        let mut client = self.connect()?;
        let mut tx = client.transaction()?;
        // PostgreSQL can still race in system catalogs when multiple sessions run
        // CREATE TABLE IF NOT EXISTS for the same new relation concurrently. Serialize
        // this tiny DDL boundary across Lifetra starters; the lock is released at commit.
        let migration_lock: i64 = 0x4c49_4645_5452_4101;
        tx.query_one("SELECT pg_advisory_xact_lock($1)", &[&migration_lock])?;
        tx.batch_execute(&format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                action_id TEXT PRIMARY KEY,\
                revision BIGINT NOT NULL CHECK (revision >= 0),\
                payload TEXT NOT NULL,\
                updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()\
            )",
            quoted_identifier(&self.table)
        ))?;
        tx.commit()?;
        Ok(())
    }

    pub fn authoritative_now(&self) -> Result<Timestamp, PostgresUnifiedStoreError> {
        let mut client = self.connect()?;
        let row = client.query_one(
            "SELECT FLOOR(EXTRACT(EPOCH FROM clock_timestamp()))::BIGINT",
            &[],
        )?;
        Ok(Timestamp::new(row.get::<_, i64>(0)))
    }

    pub fn delete_action(&self, action_id: &ActionId) -> Result<(), PostgresUnifiedStoreError> {
        let mut client = self.connect()?;
        client.execute(
            &format!(
                "DELETE FROM {} WHERE action_id = $1",
                quoted_identifier(&self.table)
            ),
            &[&action_id.as_str()],
        )?;
        Ok(())
    }

    fn connect(&self) -> Result<Client, PostgresUnifiedStoreError> {
        Ok(Client::connect(&self.connection_string, NoTls)?)
    }
}

impl UnifiedFencedStore for PostgresUnifiedFencedStore {
    type Error = PostgresUnifiedStoreError;

    fn load(&self, action_id: &ActionId) -> Result<Option<UnifiedActionRecord>, Self::Error> {
        let mut client = self.connect()?;
        let row = client.query_opt(
            &format!(
                "SELECT revision, payload FROM {} WHERE action_id = $1",
                quoted_identifier(&self.table)
            ),
            &[&action_id.as_str()],
        )?;
        row.map(decode_row).transpose()
    }

    fn compare_and_swap(
        &self,
        action_id: &ActionId,
        expected_revision: Option<u64>,
        replacement: UnifiedActionRecord,
    ) -> Result<bool, Self::Error> {
        self.compare_and_swap_resilient(action_id, expected_revision, replacement)
    }
}

/// Production-oriented wrapper that sources all lease/control timestamps from
/// PostgreSQL. The store still revalidates lease state with `clock_timestamp()`
/// inside the CAS transaction, so a lease that expires between the initial time
/// read and commit cannot authorize a new prepare/dispatch transition.
#[derive(Debug, Clone)]
pub struct PostgresUnifiedFencedRuntime {
    runtime: UnifiedFencedRuntime<PostgresUnifiedFencedStore>,
}

impl PostgresUnifiedFencedRuntime {
    pub fn new(store: PostgresUnifiedFencedStore) -> Self {
        Self {
            runtime: UnifiedFencedRuntime::new(store),
        }
    }

    pub fn store(&self) -> &PostgresUnifiedFencedStore {
        self.runtime.store()
    }

    pub fn create_action(
        &self,
        ticket: &AuthorityTicket,
        binding: &IdempotencyBinding,
        operation_ref: impl Into<String>,
    ) -> Result<UnifiedActionRecord, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        self.runtime.create_action(ticket, binding, operation_ref)
    }

    pub fn load(
        &self,
        action_id: &ActionId,
    ) -> Result<UnifiedActionRecord, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        self.runtime.load(action_id)
    }

    pub fn acquire(
        &self,
        action_id: &ActionId,
        owner: RecoveryWorkerId,
        ttl_seconds: i64,
    ) -> Result<UnifiedFencingToken, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        const DB_TIME_RETRIES: usize = 3;

        for attempt in 0..DB_TIME_RETRIES {
            let now = self
                .store()
                .authoritative_now()
                .map_err(UnifiedFencedBlock::Store)?;
            match self
                .runtime
                .acquire(action_id, owner.clone(), now, ttl_seconds)
            {
                Err(UnifiedFencedBlock::Store(
                    PostgresUnifiedStoreError::DatabaseLeaseExpired { .. },
                )) if attempt + 1 < DB_TIME_RETRIES => continue,
                result => return result,
            }
        }
        unreachable!("DB_TIME_RETRIES is non-zero")
    }

    pub fn renew(
        &self,
        token: &UnifiedFencingToken,
        ttl_seconds: i64,
    ) -> Result<UnifiedLeaseAuthority, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        let now = self
            .store()
            .authoritative_now()
            .map_err(UnifiedFencedBlock::Store)?;
        self.runtime.renew(token, now, ttl_seconds)
    }

    pub fn prepare_attempt(
        &self,
        token: &UnifiedFencingToken,
        decision: &RetryDecision,
    ) -> Result<FencedAttemptPermit, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        let now = self
            .store()
            .authoritative_now()
            .map_err(UnifiedFencedBlock::Store)?;
        self.runtime.prepare_attempt(token, decision, now, now)
    }

    pub fn record_dispatch(
        &self,
        token: &UnifiedFencingToken,
        permit: &FencedAttemptPermit,
        proof_ref: impl Into<String>,
    ) -> Result<(), UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        let now = self
            .store()
            .authoritative_now()
            .map_err(UnifiedFencedBlock::Store)?;
        self.runtime
            .record_dispatch(token, permit, now, proof_ref, now)
    }

    pub fn record_actuator_receipt(
        &self,
        receipt: &FencedActuatorReceipt,
    ) -> Result<UnifiedEvidenceCommit, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        self.runtime.record_actuator_receipt(receipt)
    }

    pub fn record_reconciliation(
        &self,
        observation: &UnifiedReconciliationObservation,
    ) -> Result<UnifiedEvidenceCommit, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        self.runtime.record_reconciliation(observation)
    }

    pub fn directive(
        &self,
        action_id: &ActionId,
    ) -> Result<UnifiedRuntimeDirective, UnifiedFencedBlock<PostgresUnifiedStoreError>> {
        self.runtime.directive(action_id)
    }
}

fn validate_database_transition(
    current: &UnifiedActionRecord,
    replacement: &UnifiedActionRecord,
    db_now: Timestamp,
) -> Result<(), PostgresUnifiedStoreError> {
    if current.binding != replacement.binding
        || replacement.revision != current.revision.saturating_add(1)
    {
        return Err(PostgresUnifiedStoreError::InvalidTransition);
    }

    let lease_changed = current.lease != replacement.lease;
    let attempts_changed = current.attempts != replacement.attempts;
    let evidence_changed = current.evidence != replacement.evidence;
    let projections_changed = current.projections != replacement.projections;

    if lease_changed {
        if attempts_changed || evidence_changed || projections_changed {
            return Err(PostgresUnifiedStoreError::InvalidTransition);
        }
        return validate_lease_transition(
            current.lease.as_ref(),
            replacement.lease.as_ref(),
            db_now,
        );
    }

    if attempts_changed {
        if evidence_changed || projections_changed {
            return Err(PostgresUnifiedStoreError::InvalidTransition);
        }
        let lease = current
            .lease
            .as_ref()
            .ok_or(PostgresUnifiedStoreError::InvalidTransition)?;
        if lease.expires_at <= db_now {
            return Err(PostgresUnifiedStoreError::DatabaseLeaseExpired { epoch: lease.epoch });
        }
        return Ok(());
    }

    if evidence_changed || projections_changed {
        return Ok(());
    }

    Err(PostgresUnifiedStoreError::InvalidTransition)
}

fn validate_lease_transition(
    current: Option<&UnifiedLeaseAuthority>,
    replacement: Option<&UnifiedLeaseAuthority>,
    db_now: Timestamp,
) -> Result<(), PostgresUnifiedStoreError> {
    match (current, replacement) {
        (None, Some(next)) => {
            if next.epoch != 1 || next.revision != 0 {
                return Err(PostgresUnifiedStoreError::InvalidTransition);
            }
            if next.expires_at <= db_now {
                return Err(PostgresUnifiedStoreError::DatabaseLeaseExpired { epoch: next.epoch });
            }
            Ok(())
        }
        (Some(old), Some(next)) if old.epoch == next.epoch => {
            if old.owner != next.owner || next.revision != old.revision.saturating_add(1) {
                return Err(PostgresUnifiedStoreError::InvalidTransition);
            }
            if old.expires_at <= db_now {
                return Err(PostgresUnifiedStoreError::DatabaseLeaseExpired { epoch: old.epoch });
            }
            if next.expires_at <= db_now {
                return Err(PostgresUnifiedStoreError::InvalidTransition);
            }
            Ok(())
        }
        (Some(old), Some(next)) if next.epoch == old.epoch.saturating_add(1) => {
            if old.expires_at > db_now {
                return Err(PostgresUnifiedStoreError::DatabaseLeaseNotExpired {
                    epoch: old.epoch,
                });
            }
            if next.revision != 0 {
                return Err(PostgresUnifiedStoreError::InvalidTransition);
            }
            if next.expires_at <= db_now {
                return Err(PostgresUnifiedStoreError::DatabaseLeaseExpired { epoch: next.epoch });
            }
            Ok(())
        }
        _ => Err(PostgresUnifiedStoreError::InvalidTransition),
    }
}

fn validate_identifier(value: &str) -> Result<(), PostgresUnifiedStoreError> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(PostgresUnifiedStoreError::InvalidTableName);
    };
    if !(first == '_' || first.is_ascii_alphabetic())
        || !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return Err(PostgresUnifiedStoreError::InvalidTableName);
    }
    Ok(())
}

fn quoted_identifier(value: &str) -> String {
    format!("\"{value}\"")
}

fn i64_revision(value: u64) -> Result<i64, PostgresUnifiedStoreError> {
    i64::try_from(value).map_err(|_| PostgresUnifiedStoreError::RevisionOutOfRange)
}

fn decode_row(row: Row) -> Result<UnifiedActionRecord, PostgresUnifiedStoreError> {
    let db_revision: i64 = row.get(0);
    let payload: String = row.get(1);
    let record = decode_record(&payload)?;
    if db_revision < 0 || record.revision != db_revision as u64 {
        return Err(PostgresUnifiedStoreError::InvalidRecord);
    }
    Ok(record)
}

fn encode_record(record: &UnifiedActionRecord) -> String {
    let mut lines = Vec::new();
    lines.push(
        [
            CODEC_VERSION.to_owned(),
            "BIND".to_owned(),
            hex(record.binding.action_id.as_str()),
            hex(&record.binding.idempotency_key),
            hex(&record.binding.operation_ref),
            record.revision.to_string(),
        ]
        .join("\t"),
    );
    if let Some(lease) = &record.lease {
        lines.push(
            [
                CODEC_VERSION.to_owned(),
                "LEASE".to_owned(),
                hex(lease.owner.as_str()),
                lease.epoch.to_string(),
                lease.revision.to_string(),
                lease.expires_at.epoch_seconds().to_string(),
            ]
            .join("\t"),
        );
    }
    for attempt in &record.attempts {
        let (dispatch_at, dispatch_proof) = attempt.dispatch.as_ref().map_or_else(
            || ("-".to_owned(), "-".to_owned()),
            |dispatch| {
                (
                    dispatch.dispatched_at.epoch_seconds().to_string(),
                    hex(&dispatch.proof_ref),
                )
            },
        );
        lines.push(
            [
                CODEC_VERSION.to_owned(),
                "ATT".to_owned(),
                hex(attempt.permit.owner.as_str()),
                attempt.permit.fencing_epoch.to_string(),
                attempt.permit.attempt_ordinal.to_string(),
                attempt.prepared_at.epoch_seconds().to_string(),
                encode_vec(&attempt.authorization_proof_refs),
                dispatch_at,
                dispatch_proof,
            ]
            .join("\t"),
        );
    }
    for evidence in &record.evidence {
        lines.push(
            [
                CODEC_VERSION.to_owned(),
                "EVID".to_owned(),
                evidence.attempt_ordinal.to_string(),
                evidence.observed_at.epoch_seconds().to_string(),
                effect_code(evidence.outcome).to_owned(),
                evidence_source_code(evidence.source).to_owned(),
                hex(&evidence.proof_ref),
            ]
            .join("\t"),
        );
    }
    for projection in &record.projections {
        lines.push(
            [
                CODEC_VERSION.to_owned(),
                "PROJ".to_owned(),
                projection.attempt_ordinal.to_string(),
                projection.projected_at.epoch_seconds().to_string(),
                hex(&projection.proof_ref),
            ]
            .join("\t"),
        );
    }
    lines.join("\n")
}

fn decode_record(payload: &str) -> Result<UnifiedActionRecord, PostgresUnifiedStoreError> {
    let mut lines = payload.lines();
    let bind = lines
        .next()
        .ok_or(PostgresUnifiedStoreError::InvalidRecord)?;
    let fields: Vec<&str> = bind.split('\t').collect();
    if fields.len() != 6 || fields[0] != CODEC_VERSION || fields[1] != "BIND" {
        return Err(PostgresUnifiedStoreError::InvalidRecord);
    }
    let action_id =
        ActionId::new(unhex(fields[2])?).map_err(|_| PostgresUnifiedStoreError::InvalidActionId)?;
    let idempotency_key = unhex(fields[3])?;
    let operation_ref = unhex(fields[4])?;
    let revision = parse_u64(fields[5])?;
    let mut record = UnifiedActionRecord {
        binding: UnifiedOperationBinding {
            action_id: action_id.clone(),
            idempotency_key: idempotency_key.clone(),
            operation_ref,
        },
        revision,
        lease: None,
        attempts: Vec::new(),
        evidence: Vec::new(),
        projections: Vec::new(),
    };

    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 2 || fields[0] != CODEC_VERSION {
            return Err(PostgresUnifiedStoreError::InvalidRecord);
        }
        match fields[1] {
            "LEASE" if fields.len() == 6 => {
                if record.lease.is_some() {
                    return Err(PostgresUnifiedStoreError::InvalidRecord);
                }
                record.lease = Some(UnifiedLeaseAuthority {
                    owner: RecoveryWorkerId::new(unhex(fields[2])?)
                        .map_err(|_| PostgresUnifiedStoreError::InvalidWorkerId)?,
                    epoch: parse_u64(fields[3])?,
                    revision: parse_u64(fields[4])?,
                    expires_at: Timestamp::new(parse_i64(fields[5])?),
                });
            }
            "ATT" if fields.len() == 9 => {
                let dispatch = if fields[7] == "-" && fields[8] == "-" {
                    None
                } else {
                    Some(UnifiedDispatchEvidence {
                        dispatched_at: Timestamp::new(parse_i64(fields[7])?),
                        proof_ref: unhex(fields[8])?,
                    })
                };
                record.attempts.push(UnifiedPreparedAttempt {
                    permit: FencedAttemptPermit {
                        action_id: action_id.clone(),
                        owner: RecoveryWorkerId::new(unhex(fields[2])?)
                            .map_err(|_| PostgresUnifiedStoreError::InvalidWorkerId)?,
                        fencing_epoch: parse_u64(fields[3])?,
                        attempt_ordinal: parse_u32(fields[4])?,
                        idempotency_key: idempotency_key.clone(),
                    },
                    prepared_at: Timestamp::new(parse_i64(fields[5])?),
                    authorization_proof_refs: decode_vec(fields[6])?,
                    dispatch,
                });
            }
            "EVID" if fields.len() == 7 => record.evidence.push(UnifiedEffectEvidence {
                attempt_ordinal: parse_u32(fields[2])?,
                observed_at: Timestamp::new(parse_i64(fields[3])?),
                outcome: parse_effect(fields[4])?,
                source: parse_evidence_source(fields[5])?,
                proof_ref: unhex(fields[6])?,
            }),
            "PROJ" if fields.len() == 5 => record.projections.push(UnifiedProjectionMarker {
                attempt_ordinal: parse_u32(fields[2])?,
                projected_at: Timestamp::new(parse_i64(fields[3])?),
                proof_ref: unhex(fields[4])?,
            }),
            _ => return Err(PostgresUnifiedStoreError::InvalidRecord),
        }
    }
    Ok(record)
}

fn effect_code(outcome: UnifiedEffectOutcome) -> &'static str {
    match outcome {
        UnifiedEffectOutcome::EffectSucceeded => "S",
        UnifiedEffectOutcome::EffectFailed => "F",
        UnifiedEffectOutcome::NoEffectConfirmed => "N",
        UnifiedEffectOutcome::StillUnknown => "U",
        UnifiedEffectOutcome::IdentityConflict => "I",
    }
}

fn parse_effect(value: &str) -> Result<UnifiedEffectOutcome, PostgresUnifiedStoreError> {
    match value {
        "S" => Ok(UnifiedEffectOutcome::EffectSucceeded),
        "F" => Ok(UnifiedEffectOutcome::EffectFailed),
        "N" => Ok(UnifiedEffectOutcome::NoEffectConfirmed),
        "U" => Ok(UnifiedEffectOutcome::StillUnknown),
        "I" => Ok(UnifiedEffectOutcome::IdentityConflict),
        _ => Err(PostgresUnifiedStoreError::InvalidRecord),
    }
}

fn evidence_source_code(source: UnifiedEvidenceSource) -> &'static str {
    match source {
        UnifiedEvidenceSource::ActuatorReceipt => "A",
        UnifiedEvidenceSource::Reconciliation => "R",
    }
}

fn parse_evidence_source(value: &str) -> Result<UnifiedEvidenceSource, PostgresUnifiedStoreError> {
    match value {
        "A" => Ok(UnifiedEvidenceSource::ActuatorReceipt),
        "R" => Ok(UnifiedEvidenceSource::Reconciliation),
        _ => Err(PostgresUnifiedStoreError::InvalidRecord),
    }
}

fn encode_vec(values: &[String]) -> String {
    values
        .iter()
        .map(|value| hex(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_vec(value: &str) -> Result<Vec<String>, PostgresUnifiedStoreError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value.split(',').map(unhex).collect()
}

fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unhex(value: &str) -> Result<String, PostgresUnifiedStoreError> {
    if !value.len().is_multiple_of(2) {
        return Err(PostgresUnifiedStoreError::InvalidHex);
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        let pair = std::str::from_utf8(pair).map_err(|_| PostgresUnifiedStoreError::InvalidHex)?;
        bytes
            .push(u8::from_str_radix(pair, 16).map_err(|_| PostgresUnifiedStoreError::InvalidHex)?);
    }
    String::from_utf8(bytes).map_err(|_| PostgresUnifiedStoreError::InvalidUtf8)
}

fn parse_u64(value: &str) -> Result<u64, PostgresUnifiedStoreError> {
    value
        .parse::<u64>()
        .map_err(|_| PostgresUnifiedStoreError::InvalidNumber)
}

fn parse_u32(value: &str) -> Result<u32, PostgresUnifiedStoreError> {
    value
        .parse::<u32>()
        .map_err(|_| PostgresUnifiedStoreError::InvalidNumber)
}

fn parse_i64(value: &str) -> Result<i64, PostgresUnifiedStoreError> {
    value
        .parse::<i64>()
        .map_err(|_| PostgresUnifiedStoreError::InvalidNumber)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use lifetra_bead::BeadId;

    use crate::{
        ExecutionMode, FencedActuatorOutcome, FencedActuatorRequest, ReconciliationOutcome,
        RetryReason, RetryVerdict, UnifiedFencedStore,
    };

    use super::*;

    fn dsn() -> Option<String> {
        std::env::var("LIFETRA_TEST_POSTGRES_URL").ok()
    }

    fn action(suffix: &str) -> ActionId {
        ActionId::new(format!("action:postgres:{suffix}:{}", std::process::id())).expect("action")
    }

    fn ticket(action_id: ActionId) -> AuthorityTicket {
        AuthorityTicket {
            action_id,
            source_bead: BeadId::new("bead:postgres"),
            execution_mode: ExecutionMode::Automatic,
            authority_proof_refs: vec!["proof:authority:postgres".into()],
            issued_at: Timestamp::new(10),
        }
    }

    fn initial(ticket: &AuthorityTicket, binding: &IdempotencyBinding) -> RetryDecision {
        RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: binding.key.clone(),
            verdict: RetryVerdict::InitialDispatchAllowed,
            reasons: vec![RetryReason::AuthorizedNotDispatched],
            proof_refs: ticket.authority_proof_refs.clone(),
        }
    }

    fn runtime(
        suffix: &str,
    ) -> Option<(
        PostgresUnifiedFencedRuntime,
        AuthorityTicket,
        IdempotencyBinding,
    )> {
        let dsn = dsn()?;
        let store = PostgresUnifiedFencedStore::new(dsn);
        store.migrate().expect("migration");
        let ticket = ticket(action(suffix));
        store.delete_action(&ticket.action_id).expect("cleanup");
        let binding =
            IdempotencyBinding::new(&ticket, format!("idem:postgres:{suffix}")).expect("binding");
        let runtime = PostgresUnifiedFencedRuntime::new(store);
        runtime
            .create_action(&ticket, &binding, format!("operation:postgres:{suffix}"))
            .expect("create");
        Some((runtime, ticket, binding))
    }

    #[test]
    fn postgres_round_trip_and_authoritative_time() {
        let Some((runtime, ticket, _)) = runtime("roundtrip") else {
            return;
        };
        let now = runtime.store().authoritative_now().expect("db time");
        assert!(now.epoch_seconds() > 1_700_000_000);
        let record = runtime.load(&ticket.action_id).expect("load");
        assert_eq!(record.binding.action_id, ticket.action_id);
        runtime.store().delete_action(&ticket.action_id).ok();
    }

    #[test]
    fn independent_connections_contend_on_one_revision() {
        let Some((runtime, ticket, _)) = runtime("contention") else {
            return;
        };
        let store_a = runtime.store().clone();
        let store_b = runtime.store().clone();
        let current = store_a.load(&ticket.action_id).expect("load").expect("row");
        let mut a = current.clone();
        let mut b = current.clone();
        a.revision += 1;
        b.revision += 1;
        a.lease = Some(UnifiedLeaseAuthority {
            owner: RecoveryWorkerId::new("worker-a").expect("worker"),
            epoch: 1,
            revision: 0,
            expires_at: Timestamp::new(i64::MAX / 4),
        });
        b.lease = Some(UnifiedLeaseAuthority {
            owner: RecoveryWorkerId::new("worker-b").expect("worker"),
            epoch: 1,
            revision: 0,
            expires_at: Timestamp::new(i64::MAX / 4),
        });
        let barrier = Arc::new(Barrier::new(3));
        let action_a = ticket.action_id.clone();
        let action_b = ticket.action_id.clone();
        let ba = barrier.clone();
        let bb = barrier.clone();
        let left = thread::spawn(move || {
            ba.wait();
            store_a.compare_and_swap(&action_a, Some(current.revision), a)
        });
        let right = thread::spawn(move || {
            bb.wait();
            store_b.compare_and_swap(&action_b, Some(current.revision), b)
        });
        barrier.wait();
        let results = [
            left.join().expect("left").expect("left cas"),
            right.join().expect("right").expect("right cas"),
        ];
        assert_eq!(results.iter().filter(|result| **result).count(), 1);
        runtime.store().delete_action(&ticket.action_id).ok();
    }

    #[test]
    fn db_time_rejects_execution_mutation_after_expiry() {
        use std::time::Duration;

        let Some((runtime, ticket, binding)) = runtime("expiry") else {
            return;
        };
        let token = runtime
            .acquire(
                &ticket.action_id,
                RecoveryWorkerId::new("expired-worker").expect("worker"),
                2,
            )
            .expect("lease");
        let store = runtime.store().clone();
        let leased = store.load(&ticket.action_id).expect("load").expect("row");
        let expires_at = leased.lease.as_ref().expect("lease record").expires_at;
        while store.authoritative_now().expect("db time") < expires_at {
            thread::sleep(Duration::from_millis(25));
        }

        let current = store.load(&ticket.action_id).expect("reload").expect("row");
        let prepared_at = store.authoritative_now().expect("db time");
        let mut illegal = current.clone();
        illegal.revision += 1;
        illegal.attempts.push(UnifiedPreparedAttempt {
            permit: FencedAttemptPermit {
                action_id: ticket.action_id.clone(),
                owner: token.owner,
                fencing_epoch: token.epoch,
                attempt_ordinal: 0,
                idempotency_key: binding.key,
            },
            prepared_at,
            authorization_proof_refs: vec!["proof:authority:postgres".into()],
            dispatch: None,
        });
        assert!(matches!(
            store.compare_and_swap(&ticket.action_id, Some(current.revision), illegal),
            Err(PostgresUnifiedStoreError::DatabaseLeaseExpired { epoch: 1 })
        ));
        runtime.store().delete_action(&ticket.action_id).ok();
    }

    #[test]
    fn late_receipt_is_evidence_even_after_takeover() {
        let Some((runtime, ticket, binding)) = runtime("late-evidence") else {
            return;
        };
        let token = runtime
            .acquire(
                &ticket.action_id,
                RecoveryWorkerId::new("worker-a").expect("worker"),
                30,
            )
            .expect("lease");
        let permit = runtime
            .prepare_attempt(&token, &initial(&ticket, &binding))
            .expect("prepare");
        let receipt = FencedActuatorReceipt {
            request: FencedActuatorRequest::from_permit(
                &permit,
                "operation:postgres:late-evidence",
            )
            .expect("request"),
            observed_at: runtime.store().authoritative_now().expect("time"),
            outcome: FencedActuatorOutcome::Applied,
            proof_ref: "proof:postgres:late-success".into(),
        };
        runtime.record_actuator_receipt(&receipt).expect("evidence");
        assert!(matches!(
            runtime.directive(&ticket.action_id).expect("directive"),
            UnifiedRuntimeDirective::CloseSucceeded { .. }
        ));
        runtime.store().delete_action(&ticket.action_id).ok();
    }

    #[test]
    fn no_effect_proof_survives_postgres_round_trip_for_retry() {
        let Some((runtime, ticket, binding)) = runtime("retry") else {
            return;
        };
        let token = runtime
            .acquire(
                &ticket.action_id,
                RecoveryWorkerId::new("worker-a").expect("worker"),
                60,
            )
            .expect("lease");
        runtime
            .prepare_attempt(&token, &initial(&ticket, &binding))
            .expect("prepare");
        let now = runtime.store().authoritative_now().expect("time");
        runtime
            .record_reconciliation(&UnifiedReconciliationObservation {
                action_id: ticket.action_id.clone(),
                idempotency_key: binding.key.clone(),
                operation_ref: "operation:postgres:retry".into(),
                attempt_ordinal: 0,
                observed_at: now,
                outcome: ReconciliationOutcome::NoEffectConfirmed,
                proof_ref: "proof:postgres:no-effect".into(),
            })
            .expect("reconciliation");
        let retry = RetryDecision {
            action_id: ticket.action_id.clone(),
            idempotency_key: binding.key,
            verdict: RetryVerdict::RedispatchAllowed { ordinal: 1 },
            reasons: Vec::new(),
            proof_refs: vec!["proof:postgres:no-effect".into()],
        };
        let permit = runtime.prepare_attempt(&token, &retry).expect("retry");
        assert_eq!(permit.attempt_ordinal, 1);
        runtime.store().delete_action(&ticket.action_id).ok();
    }
}
