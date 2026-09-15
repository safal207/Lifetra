use super::*;

pub(super) struct ParsedRecords {
    pub events: Vec<JournalEvent>,
    pub next_sequence: u64,
    pub valid_len: usize,
    pub truncated_tail: bool,
}

pub(super) fn parse_records(bytes: &[u8]) -> Result<ParsedRecords, JournalError> {
    if bytes.is_empty() {
        return Err(JournalError::EmptyJournal);
    }

    let truncated_tail = !bytes.ends_with(b"\n");
    let valid_len = if truncated_tail {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    } else {
        bytes.len()
    };
    if valid_len == 0 {
        return Err(JournalError::EmptyJournal);
    }

    let mut events = Vec::new();
    let mut expected_sequence = 0_u64;
    for raw_line in bytes[..valid_len].split(|byte| *byte == b'\n') {
        if raw_line.is_empty() {
            continue;
        }
        let line = std::str::from_utf8(raw_line).map_err(|_| JournalError::InvalidUtf8)?;
        let mut parts = line.splitn(3, '\t');
        let sequence = parse_u64(parts.next())?;
        let checksum_hex = parts.next().ok_or(JournalError::InvalidRecord)?;
        let payload = parts.next().ok_or(JournalError::InvalidRecord)?;

        if sequence != expected_sequence {
            return Err(JournalError::SequenceGap {
                expected: expected_sequence,
                received: sequence,
            });
        }
        let recorded_checksum =
            u64::from_str_radix(checksum_hex, 16).map_err(|_| JournalError::InvalidRecord)?;
        if recorded_checksum != checksum(sequence, payload) {
            return Err(JournalError::InvalidChecksum { sequence });
        }

        events.push(decode_event(payload)?);
        expected_sequence += 1;
    }

    Ok(ParsedRecords {
        events,
        next_sequence: expected_sequence,
        valid_len,
        truncated_tail,
    })
}

pub(super) fn checksum(sequence: u64, payload: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in format!("{sequence}\t{payload}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(super) fn encode_event(event: &JournalEvent) -> String {
    match event {
        JournalEvent::Binding { ticket, binding } => {
            let mode = match ticket.execution_mode {
                ExecutionMode::Automatic => "A",
                ExecutionMode::ApprovedManual => "M",
            };
            let mut fields = vec![
                JOURNAL_VERSION.to_owned(),
                "BIND".to_owned(),
                encode_string(ticket.action_id.as_str()),
                encode_string(ticket.source_bead.as_str()),
                mode.to_owned(),
                ticket.issued_at.epoch_seconds().to_string(),
                encode_string(&binding.key),
                ticket.authority_proof_refs.len().to_string(),
            ];
            fields.extend(
                ticket
                    .authority_proof_refs
                    .iter()
                    .map(|proof| encode_string(proof)),
            );
            fields.join("\t")
        }
        JournalEvent::Prepared(prepared) => {
            let mut fields = vec![
                JOURNAL_VERSION.to_owned(),
                "PREP".to_owned(),
                encode_string(prepared.id.action_id.as_str()),
                prepared.id.ordinal.to_string(),
                prepared.prepared_at.epoch_seconds().to_string(),
                prepared.authorization_proof_refs.len().to_string(),
            ];
            fields.extend(
                prepared
                    .authorization_proof_refs
                    .iter()
                    .map(|proof| encode_string(proof)),
            );
            fields.join("\t")
        }
        JournalEvent::Dispatch {
            ordinal,
            dispatched_at,
            proof_ref,
        } => [
            JOURNAL_VERSION.to_owned(),
            "DISP".to_owned(),
            ordinal.to_string(),
            dispatched_at.epoch_seconds().to_string(),
            encode_string(proof_ref),
        ]
        .join("\t"),
        JournalEvent::Reconciliation {
            ordinal,
            observed_at,
            outcome,
            proof_ref,
        } => [
            JOURNAL_VERSION.to_owned(),
            "RECON".to_owned(),
            ordinal.to_string(),
            observed_at.epoch_seconds().to_string(),
            encode_reconciliation(*outcome).to_owned(),
            encode_string(proof_ref),
        ]
        .join("\t"),
        JournalEvent::External {
            ordinal,
            observed_at,
            outcome,
            proof_ref,
        } => [
            JOURNAL_VERSION.to_owned(),
            "EXT".to_owned(),
            ordinal.to_string(),
            observed_at.epoch_seconds().to_string(),
            encode_external(*outcome).to_owned(),
            encode_string(proof_ref),
        ]
        .join("\t"),
    }
}

fn decode_event(payload: &str) -> Result<JournalEvent, JournalError> {
    let mut fields = payload.split('\t');
    if fields.next() != Some(JOURNAL_VERSION) {
        return Err(JournalError::InvalidRecord);
    }
    let kind = fields.next().ok_or(JournalError::InvalidRecord)?;
    let remaining: Vec<&str> = fields.collect();

    match kind {
        "BIND" => decode_binding(&remaining),
        "PREP" => decode_prepared(&remaining),
        "DISP" => {
            if remaining.len() != 3 {
                return Err(JournalError::InvalidRecord);
            }
            Ok(JournalEvent::Dispatch {
                ordinal: parse_u32(Some(remaining[0]))?,
                dispatched_at: Timestamp::new(parse_i64(Some(remaining[1]))?),
                proof_ref: decode_string(remaining[2])?,
            })
        }
        "RECON" => {
            if remaining.len() != 4 {
                return Err(JournalError::InvalidRecord);
            }
            Ok(JournalEvent::Reconciliation {
                ordinal: parse_u32(Some(remaining[0]))?,
                observed_at: Timestamp::new(parse_i64(Some(remaining[1]))?),
                outcome: decode_reconciliation(remaining[2])?,
                proof_ref: decode_string(remaining[3])?,
            })
        }
        "EXT" => {
            if remaining.len() != 4 {
                return Err(JournalError::InvalidRecord);
            }
            Ok(JournalEvent::External {
                ordinal: parse_u32(Some(remaining[0]))?,
                observed_at: Timestamp::new(parse_i64(Some(remaining[1]))?),
                outcome: decode_external(remaining[2])?,
                proof_ref: decode_string(remaining[3])?,
            })
        }
        _ => Err(JournalError::InvalidRecord),
    }
}

fn decode_binding(fields: &[&str]) -> Result<JournalEvent, JournalError> {
    if fields.len() < 6 {
        return Err(JournalError::InvalidRecord);
    }
    let proof_count = fields[5]
        .parse::<usize>()
        .map_err(|_| JournalError::InvalidNumber)?;
    if fields.len() != 6 + proof_count {
        return Err(JournalError::InvalidRecord);
    }

    let action_raw = decode_string(fields[0])?;
    let action_id = ActionId::new(action_raw).map_err(|_| JournalError::InvalidActionId)?;
    let source_bead = BeadId::new(decode_string(fields[1])?);
    let execution_mode = match fields[2] {
        "A" => ExecutionMode::Automatic,
        "M" => ExecutionMode::ApprovedManual,
        _ => return Err(JournalError::InvalidExecutionMode),
    };
    let issued_at = Timestamp::new(parse_i64(Some(fields[3]))?);
    let key = decode_string(fields[4])?;
    if key.trim().is_empty() {
        return Err(JournalError::EmptyIdempotencyKey);
    }
    let proof_refs = fields[6..]
        .iter()
        .map(|value| decode_string(value))
        .collect::<Result<Vec<_>, _>>()?;

    let ticket = AuthorityTicket {
        action_id: action_id.clone(),
        source_bead,
        execution_mode,
        authority_proof_refs: proof_refs,
        issued_at,
    };
    let binding = IdempotencyBinding { action_id, key };
    Ok(JournalEvent::Binding { ticket, binding })
}

fn decode_prepared(fields: &[&str]) -> Result<JournalEvent, JournalError> {
    if fields.len() < 4 {
        return Err(JournalError::InvalidRecord);
    }
    let proof_count = fields[3]
        .parse::<usize>()
        .map_err(|_| JournalError::InvalidNumber)?;
    if fields.len() != 4 + proof_count {
        return Err(JournalError::InvalidRecord);
    }
    let action_raw = decode_string(fields[0])?;
    let action_id = ActionId::new(action_raw).map_err(|_| JournalError::InvalidActionId)?;
    let proofs = fields[4..]
        .iter()
        .map(|value| decode_string(value))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(JournalEvent::Prepared(PreparedAttempt {
        id: AttemptId::new(action_id, parse_u32(Some(fields[1]))?),
        prepared_at: Timestamp::new(parse_i64(Some(fields[2]))?),
        authorization_proof_refs: proofs,
    }))
}

fn encode_reconciliation(outcome: ReconciliationOutcome) -> &'static str {
    match outcome {
        ReconciliationOutcome::EffectSucceeded => "S",
        ReconciliationOutcome::EffectFailed => "F",
        ReconciliationOutcome::NoEffectConfirmed => "N",
        ReconciliationOutcome::StillUnknown => "U",
    }
}

fn decode_reconciliation(value: &str) -> Result<ReconciliationOutcome, JournalError> {
    match value {
        "S" => Ok(ReconciliationOutcome::EffectSucceeded),
        "F" => Ok(ReconciliationOutcome::EffectFailed),
        "N" => Ok(ReconciliationOutcome::NoEffectConfirmed),
        "U" => Ok(ReconciliationOutcome::StillUnknown),
        _ => Err(JournalError::InvalidReconciliationOutcome),
    }
}

fn encode_external(outcome: ExecutionOutcome) -> &'static str {
    match outcome {
        ExecutionOutcome::Succeeded => "S",
        ExecutionOutcome::Failed => "F",
    }
}

fn decode_external(value: &str) -> Result<ExecutionOutcome, JournalError> {
    match value {
        "S" => Ok(ExecutionOutcome::Succeeded),
        "F" => Ok(ExecutionOutcome::Failed),
        _ => Err(JournalError::InvalidExecutionOutcome),
    }
}

fn encode_string(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_string(value: &str) -> Result<String, JournalError> {
    if value.len() % 2 != 0 {
        return Err(JournalError::InvalidHex);
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(chunk).map_err(|_| JournalError::InvalidHex)?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(|_| JournalError::InvalidHex)?);
    }
    String::from_utf8(bytes).map_err(|_| JournalError::InvalidUtf8)
}

fn parse_u64(value: Option<&str>) -> Result<u64, JournalError> {
    value
        .ok_or(JournalError::InvalidRecord)?
        .parse::<u64>()
        .map_err(|_| JournalError::InvalidNumber)
}

fn parse_u32(value: Option<&str>) -> Result<u32, JournalError> {
    value
        .ok_or(JournalError::InvalidRecord)?
        .parse::<u32>()
        .map_err(|_| JournalError::InvalidNumber)
}

fn parse_i64(value: Option<&str>) -> Result<i64, JournalError> {
    value
        .ok_or(JournalError::InvalidRecord)?
        .parse::<i64>()
        .map_err(|_| JournalError::InvalidNumber)
}
