//! The single place where Convoy starts another program.
//!
//! `convoy-core` stays synchronous and toolkit-free, so every external command
//! is described by a [`ProcessSpec`] and executed through a [`ProcessRunner`].
//! The GTK front end wraps the same runner in `gio::spawn_blocking`; tests
//! substitute their own implementation and assert on what would have run.

use crate::{bail, ConvoyError, Result};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub file: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Complete replacement environment; `None` inherits the parent's.
    pub env: Option<BTreeMap<String, String>>,
    pub timeout: Option<Duration>,
    /// Upper bound on each captured stream, mirroring `maxBuffer`.
    pub max_output: usize,
    pub stdin: Option<String>,
}

impl ProcessSpec {
    pub fn new(file: impl Into<String>, args: Vec<String>) -> Self {
        ProcessSpec {
            file: file.into(),
            args,
            cwd: None,
            env: None,
            timeout: Some(Duration::from_secs(60)),
            max_output: 2 * 1024 * 1024,
            stdin: None,
        }
    }

    pub fn cwd(mut self, directory: impl Into<PathBuf>) -> Self {
        self.cwd = Some(directory.into());
        self
    }

    pub fn env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = bytes;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Output {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }

    /// `error.stderr?.trim() || error.message` — the wording git errors reach
    /// the user with.
    pub fn failure(&self) -> ConvoyError {
        let trimmed = self.stderr.trim();
        if trimmed.is_empty() {
            ConvoyError::message(format!(
                "Command failed with exit code {}.",
                self.status
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ))
        } else {
            ConvoyError::message(trimmed)
        }
    }
}

pub trait ProcessRunner: Send + Sync {
    fn run(&self, spec: &ProcessSpec) -> Result<Output>;
}

/// `std::process::Command` with the timeout and output cap that `execFile`
/// provides and Rust does not.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdRunner;

impl ProcessRunner for StdRunner {
    fn run(&self, spec: &ProcessSpec) -> Result<Output> {
        let mut command = Command::new(&spec.file);
        command
            .args(&spec.args)
            .stdin(if spec.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = &spec.cwd {
            command.current_dir(directory);
        }
        if let Some(env) = &spec.env {
            command.env_clear().envs(env);
        }
        let mut child = command.spawn()?;

        if let Some(input) = &spec.stdin {
            if let Some(mut handle) = child.stdin.take() {
                use std::io::Write;
                let _ = handle.write_all(input.as_bytes());
            }
        }

        let cap = spec.max_output;
        let stdout = drain(child.stdout.take(), cap);
        let stderr = drain(child.stderr.take(), cap);

        let deadline = spec.timeout.map(|timeout| Instant::now() + timeout);
        let status = loop {
            match child.try_wait()? {
                Some(status) => break status,
                None => {
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        let _ = child.kill();
                        let _ = child.wait();
                        bail!("Command timed out.")
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };

        let stdout = stdout.recv().unwrap_or_else(|_| Ok(String::new()))?;
        let stderr = stderr.recv().unwrap_or_else(|_| Ok(String::new()))?;
        Ok(Output {
            status: status.code(),
            stdout,
            stderr,
        })
    }
}

/// Reads a stream on its own thread so a chatty child cannot deadlock on a
/// full pipe while we wait for it to exit.
fn drain<R: Read + Send + 'static>(
    stream: Option<R>,
    cap: usize,
) -> mpsc::Receiver<Result<String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<String> {
            let Some(mut stream) = stream else {
                return Ok(String::new());
            };
            let mut collected = Vec::new();
            let mut buffer = [0u8; 16 * 1024];
            loop {
                let read = stream.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                collected.extend_from_slice(&buffer[..read]);
                if collected.len() > cap {
                    bail!("Command output exceeded its size limit.")
                }
            }
            Ok(String::from_utf8_lossy(&collected).into_owned())
        })();
        let _ = sender.send(outcome);
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_status() {
        let result = StdRunner
            .run(&ProcessSpec::new(
                "/bin/sh",
                vec!["-c".into(), "printf hello; printf oops >&2; exit 3".into()],
            ))
            .unwrap();
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.stderr, "oops");
        assert_eq!(result.status, Some(3));
        assert!(!result.ok());
    }

    #[test]
    fn kills_a_command_that_outlives_its_timeout() {
        let error = StdRunner
            .run(
                &ProcessSpec::new("/bin/sh", vec!["-c".into(), "sleep 30".into()])
                    .timeout(Duration::from_millis(150)),
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "Command timed out.");
    }

    #[test]
    fn rejects_output_beyond_the_cap() {
        let error = StdRunner
            .run(
                &ProcessSpec::new("/bin/sh", vec!["-c".into(), "yes convoy | head -c 200000".into()])
                    .max_output(1024),
            )
            .unwrap_err();
        assert!(error.to_string().contains("size limit"));
    }
}
