#[path = "../metro_station/support.rs"]
mod support;

use std::collections::HashSet;
use std::env;
use std::io::{self, BufRead, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use support::{evaluate_request, StationDecision, StationRequest};

const REQUEST_PROTOCOL: &str = "lifetra.station.request-envelope.v0.2";
const RESPONSE_PROTOCOL: &str = "lifetra.station.response-envelope.v0.2";
const ERROR_PROTOCOL: &str = "lifetra.station.error.v0.2";
const DEFAULT_QUEUE_CAPACITY: usize = 1024;
const MAX_WORKERS: usize = 64;

#[derive(Debug, Deserialize)]
struct RequestEnvelope {
    protocol: String,
    request_id: String,
    request: StationRequest,
}

#[derive(Debug, Serialize)]
struct ResponseEnvelope {
    protocol: &'static str,
    request_id: String,
    decision: StationDecision,
}

#[derive(Debug, Serialize)]
struct StationError {
    protocol: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    error: String,
}

struct Job {
    request_id: String,
    request: StationRequest,
}

fn parse_workers() -> Result<usize, String> {
    match env::var("LIFETRA_METRO_WORKERS") {
        Ok(raw) => {
            let workers = raw
                .parse::<usize>()
                .map_err(|error| format!("invalid LIFETRA_METRO_WORKERS: {error}"))?;
            if workers == 0 || workers > MAX_WORKERS {
                return Err(format!(
                    "LIFETRA_METRO_WORKERS must be between 1 and {MAX_WORKERS}"
                ));
            }
            Ok(workers)
        }
        Err(env::VarError::NotPresent) => Ok(thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .clamp(1, MAX_WORKERS)),
        Err(error) => Err(format!("read LIFETRA_METRO_WORKERS: {error}")),
    }
}

fn decode_envelope(line: &str) -> Result<RequestEnvelope, StationError> {
    let value: Value = serde_json::from_str(line).map_err(|error| StationError {
        protocol: ERROR_PROTOCOL,
        request_id: None,
        error: format!("decode request envelope JSON: {error}"),
    })?;

    let request_id = value
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let protocol = value
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    if protocol != REQUEST_PROTOCOL {
        return Err(StationError {
            protocol: ERROR_PROTOCOL,
            request_id: non_empty_id(&request_id),
            error: format!("unsupported request envelope protocol {protocol:?}"),
        });
    }
    if request_id.is_empty() {
        return Err(StationError {
            protocol: ERROR_PROTOCOL,
            request_id: None,
            error: "request_id is required".into(),
        });
    }

    serde_json::from_value(value).map_err(|error| StationError {
        protocol: ERROR_PROTOCOL,
        request_id: Some(request_id.clone()),
        error: format!("decode station request payload: {error}"),
    })
}

fn reserve_request_id(
    seen_request_ids: &mut HashSet<String>,
    request_id: &str,
) -> Result<(), StationError> {
    if seen_request_ids.insert(request_id.to_owned()) {
        Ok(())
    } else {
        Err(StationError {
            protocol: ERROR_PROTOCOL,
            request_id: Some(request_id.to_owned()),
            error: "request_id was already used by this station process".into(),
        })
    }
}

fn non_empty_id(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn encode_error(error: StationError) -> String {
    serde_json::to_string(&error).unwrap_or_else(|_| {
        "{\"protocol\":\"lifetra.station.error.v0.2\",\"error\":\"encode failure\"}"
            .to_string()
    })
}

fn evaluate(job: Job) -> String {
    match evaluate_request(job.request) {
        Ok(decision) => serde_json::to_string(&ResponseEnvelope {
            protocol: RESPONSE_PROTOCOL,
            request_id: job.request_id,
            decision,
        })
        .unwrap_or_else(|error| {
            encode_error(StationError {
                protocol: ERROR_PROTOCOL,
                request_id: None,
                error: format!("encode response envelope JSON: {error}"),
            })
        }),
        Err(error) => encode_error(StationError {
            protocol: ERROR_PROTOCOL,
            request_id: Some(job.request_id),
            error,
        }),
    }
}

fn worker_loop(
    jobs: Arc<Mutex<mpsc::Receiver<Job>>>,
    responses: mpsc::Sender<String>,
) -> Result<(), String> {
    loop {
        let job = {
            let receiver = jobs
                .lock()
                .map_err(|_| "Metro job receiver lock was poisoned".to_string())?;
            receiver.recv()
        };

        let job = match job {
            Ok(job) => job,
            Err(_) => return Ok(()),
        };

        if responses.send(evaluate(job)).is_err() {
            return Ok(());
        }
    }
}

fn writer_loop(responses: mpsc::Receiver<String>) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();

    for response in responses {
        writeln!(output, "{response}")
            .map_err(|error| format!("write Metro response line: {error}"))?;
        output
            .flush()
            .map_err(|error| format!("flush Metro response line: {error}"))?;
    }

    Ok(())
}

fn run() -> Result<(), String> {
    let worker_count = parse_workers()?;
    let (job_tx, job_rx) = mpsc::sync_channel::<Job>(DEFAULT_QUEUE_CAPACITY);
    let (response_tx, response_rx) = mpsc::channel::<String>();
    let shared_jobs = Arc::new(Mutex::new(job_rx));
    let mut seen_request_ids = HashSet::new();

    let writer = thread::spawn(move || writer_loop(response_rx));
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let jobs = Arc::clone(&shared_jobs);
        let responses = response_tx.clone();
        workers.push(thread::spawn(move || worker_loop(jobs, responses)));
    }

    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| format!("read Metro request line: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }

        match decode_envelope(&line) {
            Ok(envelope) => {
                if let Err(error) = reserve_request_id(&mut seen_request_ids, &envelope.request_id) {
                    response_tx
                        .send(encode_error(error))
                        .map_err(|_| "Metro response writer closed unexpectedly".to_string())?;
                    continue;
                }
                job_tx
                    .send(Job {
                        request_id: envelope.request_id,
                        request: envelope.request,
                    })
                    .map_err(|_| "Metro job queue closed unexpectedly".to_string())?;
            }
            Err(error) => response_tx
                .send(encode_error(error))
                .map_err(|_| "Metro response writer closed unexpectedly".to_string())?,
        }
    }

    drop(job_tx);

    for worker in workers {
        worker
            .join()
            .map_err(|_| "Metro worker thread panicked".to_string())??;
    }
    drop(response_tx);
    writer
        .join()
        .map_err(|_| "Metro writer thread panicked".to_string())??;

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("multiplex Metro station error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_requires_request_id_before_payload_decode() {
        let line = r#"{"protocol":"lifetra.station.request-envelope.v0.2","request_id":"","request":{}}"#;
        let error = decode_envelope(line).expect_err("empty request_id must fail closed");
        assert_eq!(error.protocol, ERROR_PROTOCOL);
        assert!(error.error.contains("request_id"));
    }

    #[test]
    fn invalid_payload_error_preserves_request_id() {
        let line = r#"{"protocol":"lifetra.station.request-envelope.v0.2","request_id":"req-1","request":{}}"#;
        let error = decode_envelope(line).expect_err("invalid request payload must fail closed");
        assert_eq!(error.request_id.as_deref(), Some("req-1"));
        assert!(error.error.contains("station request payload"));
    }

    #[test]
    fn duplicate_request_id_is_rejected_for_station_lifetime() {
        let mut seen = HashSet::new();
        reserve_request_id(&mut seen, "req-1").expect("first request id must be accepted");
        let error = reserve_request_id(&mut seen, "req-1")
            .expect_err("duplicate request id must fail closed");
        assert_eq!(error.request_id.as_deref(), Some("req-1"));
        assert!(error.error.contains("already used"));
    }

    #[test]
    fn malformed_json_returns_unbound_typed_error() {
        let error = decode_envelope("not-json").expect_err("malformed JSON must fail closed");
        assert_eq!(error.protocol, ERROR_PROTOCOL);
        assert!(error.request_id.is_none());
    }
}
