mod support;

use std::io::{self, Read, Write};

use support::{evaluate_request, StationRequest};

fn run() -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("read station request from stdin: {error}"))?;

    let request: StationRequest = serde_json::from_str(&input)
        .map_err(|error| format!("decode station request JSON: {error}"))?;
    let decision = evaluate_request(request)?;

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, &decision)
        .map_err(|error| format!("encode station decision JSON: {error}"))?;
    writeln!(handle).map_err(|error| format!("flush station decision JSON: {error}"))?;

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("metro station error: {error}");
        std::process::exit(1);
    }
}
