//! Port of `codexLimits()` — reading Codex usage through the app server.
//!
//! Only `initialize` and `account/rateLimits/read` are sent. No model request
//! is made, and nothing but validated quota windows leaves this module.

use super::{launch::LaunchSpec, provider_spec, rate_windows, RateWindow};
use crate::workspace::model::Agent;
use crate::{bail, ConvoyError, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(25);
const MAX_OUTPUT: usize = 2 * 1024 * 1024;

/// What a single response line means. Split out from the I/O loop so the
/// protocol can be tested without starting a process.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Not one of our requests — keep reading.
    Ignore,
    /// Send these JSON-RPC requests, then keep reading.
    Send(Vec<String>),
    Done(Vec<RateWindow>),
    Failed(&'static str),
}

pub fn initialize() -> String {
    json!({
        "id": 1,
        "method": "initialize",
        "params": { "clientInfo": { "name": "convoy", "version": "0.1.0" } }
    })
    .to_string()
}

pub fn handle_line(line: &str, now: f64) -> Step {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Step::Ignore;
    };
    let id = message.get("id").and_then(Value::as_i64);
    if !matches!(id, Some(1) | Some(2)) {
        return Step::Ignore;
    }
    if message.get("error").is_some() {
        return Step::Failed("Usage unavailable for this account. Check the CLI sign-in.");
    }
    match id {
        Some(1) => Step::Send(vec![
            json!({ "method": "initialized" }).to_string(),
            json!({ "id": 2, "method": "account/rateLimits/read" }).to_string(),
        ]),
        Some(2) => Step::Done(rate_windows(
            message.get("result").unwrap_or(&Value::Null),
            now,
        )),
        _ => Step::Ignore,
    }
}

pub fn spec(env: &BTreeMap<String, String>) -> LaunchSpec {
    provider_spec(
        Agent::Codex,
        &[
            "app-server".to_string(),
            "--listen".to_string(),
            "stdio://".to_string(),
        ],
        env,
    )
}

/// Runs the exchange against the installed Codex CLI.
pub fn codex_limits(
    env: &BTreeMap<String, String>,
    cwd: &Path,
    now: f64,
) -> Result<Vec<RateWindow>> {
    let spec = spec(env);
    let mut child = Command::new(&spec.file)
        .args(&spec.args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or_else(|| {
        ConvoyError::message("Codex could not read usage. Check installation and sign-in.")
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        ConvoyError::message("Codex could not read usage. Check installation and sign-in.")
    })?;

    let (sender, receiver) = mpsc::channel::<std::result::Result<String, &'static str>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut total = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    total += read;
                    if total > MAX_OUTPUT {
                        let _ = sender.send(Err("Usage response exceeded the size limit."));
                        break;
                    }
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let finish = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };

    writeln!(stdin, "{}", initialize())?;
    stdin.flush()?;

    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            finish(&mut child);
            bail!("Codex usage request timed out.")
        }
        match receiver.recv_timeout(remaining) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                finish(&mut child);
                bail!("Codex usage request timed out.")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                finish(&mut child);
                bail!("Codex could not read usage. Check installation and sign-in.")
            }
            Ok(Err(message)) => {
                finish(&mut child);
                bail!("{message}")
            }
            Ok(Ok(line)) => match handle_line(&line, now) {
                Step::Ignore => {}
                Step::Send(requests) => {
                    for request in requests {
                        writeln!(stdin, "{request}")?;
                    }
                    stdin.flush()?;
                }
                Step::Failed(message) => {
                    finish(&mut child);
                    bail!("{message}")
                }
                Step::Done(windows) => {
                    drop(stdin);
                    finish(&mut child);
                    return Ok(windows);
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors the Electron test: initialization and one rate-limit read, and
    /// nothing else.
    #[test]
    fn performs_only_initialization_and_rate_limit_read() {
        let mut sent = vec![initialize()];
        let mut windows = None;
        for line in [
            r#"{"id":1,"result":{}}"#,
            r#"{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":25}}}}"#,
        ] {
            match handle_line(line, 0.0) {
                Step::Send(requests) => sent.extend(requests),
                Step::Done(result) => windows = Some(result),
                other => panic!("unexpected step: {other:?}"),
            }
        }
        let methods: Vec<String> = sent
            .iter()
            .map(|request| {
                serde_json::from_str::<Value>(request).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            methods,
            ["initialize", "initialized", "account/rateLimits/read"]
        );
        assert_eq!(windows.unwrap()[0].percent, 25.0);
    }

    #[test]
    fn reports_a_signed_out_account_without_guessing_zero() {
        assert_eq!(
            handle_line(r#"{"id":2,"error":{"code":-32000}}"#, 0.0),
            Step::Failed("Usage unavailable for this account. Check the CLI sign-in.")
        );
        assert_eq!(handle_line("not json", 0.0), Step::Ignore);
        assert_eq!(handle_line(r#"{"id":7,"result":{}}"#, 0.0), Step::Ignore);
    }
}
