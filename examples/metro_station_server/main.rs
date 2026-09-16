#[path = "../metro_station/support.rs"]
mod support;

use std::io::{self, BufRead, Write};

use serde::Serialize;
use support::{evaluate_request, StationRequest};

const ERROR_PROTOCOL: &str = "lifetra.station.error.v0.1";

#[derive(Debug, Serialize)]
struct StationError {
    protocol: &'static str,
    error: String,
}

fn evaluate_line(line: &str) -> Result<String, String> {
    let request: StationRequest = serde_json::from_str(line)
        .map_err(|error| format!("decode station request JSON: {error}"))?;
    let decision = evaluate_request(request)?;
    serde_json::to_string(&decision)
        .map_err(|error| format!("encode station decision JSON: {error}"))
}

fn encode_error(error: String) -> String {
    serde_json::to_string(&StationError {
        protocol: ERROR_PROTOCOL,
        error,
    })
    .unwrap_or_else(|_| {
        "{\"protocol\":\"lifetra.station.error.v0.1\",\"error\":\"encode failure\"}".to_string()
    })
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|error| format!("read station request line: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match evaluate_line(&line) {
            Ok(decision) => decision,
            Err(error) => encode_error(error),
        };

        writeln!(output, "{response}")
            .map_err(|error| format!("write station response line: {error}"))?;
        output
            .flush()
            .map_err(|error| format!("flush station response line: {error}"))?;
    }

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("persistent metro station error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_line_returns_typed_error_without_panicking() {
        let error = evaluate_line("not-json").expect_err("malformed input must fail closed");
        let encoded = encode_error(error);

        assert!(encoded.contains(ERROR_PROTOCOL));
        assert!(encoded.contains("decode station request JSON"));
    }
}
