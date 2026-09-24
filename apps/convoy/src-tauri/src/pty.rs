//! The terminal.
//!
//! `portable-pty` gives a real pty on Unix and ConPTY on Windows, so one
//! implementation serves both platforms — the thing that made a native toolkit
//! impossible here, since no cross-platform terminal widget covers Windows.
//!
//! Output is pushed to the web view as events rather than polled: an agent can
//! emit thousands of lines a second, and a poll would either lag behind it or
//! spin.

use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime};

/// One running agent: the pty it owns, the handle to write into it, and the
/// process-group leader used to stop the whole tree.
struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
    stopping: bool,
}

#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
}

/// Everything needed to start one agent. Grouped rather than passed loose:
/// seven positional arguments of which three are strings invites the kind of
/// mistake a compiler cannot catch.
pub struct Launch<'a> {
    pub id: &'a str,
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a std::path::Path,
    /// The complete environment. The agent must not inherit this process's own
    /// conversation markers.
    pub env: &'a [(String, String)],
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Serialize)]
pub struct Output {
    pub id: String,
    pub data: String,
}

#[derive(Clone, Serialize)]
pub struct Exit {
    pub id: String,
    pub code: i32,
    /// True when the user asked for it, so the UI does not report a failure.
    pub stopped: bool,
}

impl Terminals {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn running(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .is_ok_and(|sessions| sessions.contains_key(id))
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions
            .lock()
            .map(|sessions| sessions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Starts a command in a new pty and streams its output to the front end.
    // Generic over the runtime so the tests can drive it with Tauri's mock
    // one and still go through the real event channel.
    pub fn start<R: Runtime>(
        self: &Arc<Self>,
        app: &AppHandle<R>,
        launch: Launch<'_>,
    ) -> Result<u32, String> {
        let Launch {
            id,
            program,
            args,
            cwd,
            env,
            cols,
            rows,
        } = launch;
        if self.running(id) {
            return Err("This session is already running.".into());
        }

        let pty = NativePtySystem::default();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;
        let master = pair.master;
        let slave = pair.slave;

        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        // The environment is replaced rather than extended: the agent must not
        // inherit this process's own conversation markers.
        command.env_clear();
        for (key, value) in env {
            command.env(key, value);
        }

        let mut child = slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        let pid = child.process_id();

        // The slave has to go now that the child owns its own copy. Holding it
        // keeps the pty open forever: the master never reaches EOF, the reader
        // never returns, and the child is never reaped — so the exit code that
        // decides a task's fate never arrives.
        drop(slave);

        let writer = master.take_writer().map_err(|error| error.to_string())?;
        let mut reader = master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;

        // Reading blocks, so it gets its own thread; the same thread reaps the
        // child, which is how the exit code is obtained.
        let handle = app.clone();
        let session_id = id.to_string();
        let terminals = Arc::clone(self);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let data = String::from_utf8_lossy(&buffer[..read]).into_owned();
                        let _ = handle.emit(
                            "terminal:data",
                            Output {
                                id: session_id.clone(),
                                data,
                            },
                        );
                    }
                }
            }
            let code = child
                .wait()
                .map(|status| status.exit_code() as i32)
                .unwrap_or(1);
            let stopped = terminals.take(&session_id);
            let _ = handle.emit(
                "terminal:exit",
                Exit {
                    id: session_id,
                    code,
                    stopped,
                },
            );
        });

        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                id.to_string(),
                Session {
                    master,
                    writer,
                    pid,
                    stopping: false,
                },
            );
        }
        Ok(pid.unwrap_or(0))
    }

    /// Removes a finished session and reports whether its exit was asked for.
    fn take(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id))
            .map(|session| session.stopping)
            .unwrap_or(false)
    }

    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|_| "Terminal unavailable.")?;
        let Some(session) = sessions.get_mut(id) else {
            return Err("Start or resume the target session first.".into());
        };
        session
            .writer
            .write_all(data.as_bytes())
            .map_err(|error| error.to_string())?;
        session.writer.flush().map_err(|error| error.to_string())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock().map_err(|_| "Terminal unavailable.")?;
        let Some(session) = sessions.get(id) else {
            return Ok(());
        };
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())
    }

    /// Stops the process group, so the agent's own children go with it, then
    /// insists after a grace period. Takes `Arc<Self>` because the follow-up
    /// runs on its own thread and has to keep the registry alive.
    pub fn stop(self: &Arc<Self>, id: &str) {
        let pid = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            let Some(session) = sessions.get_mut(id) else {
                return;
            };
            if session.stopping {
                return;
            }
            session.stopping = true;
            session.pid
        };
        let Some(pid) = pid else { return };
        signal_group(pid, Signal::Hangup);

        let terminals = Arc::clone(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            if terminals.running(&id) {
                signal_group(pid, Signal::Kill);
            }
        });
    }
}

enum Signal {
    Hangup,
    Kill,
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: Signal) {
    let number = match signal {
        Signal::Hangup => libc::SIGHUP,
        Signal::Kill => libc::SIGKILL,
    };
    // Negative pid is the process group: the agent spawns its own children,
    // and signalling only the leader would leave them running.
    unsafe {
        libc::killpg(pid as i32, number);
    }
}

#[cfg(windows)]
fn signal_group(pid: u32, _signal: Signal) {
    // ConPTY has no process groups; closing the pty ends the job object, and
    // this is the blunt fallback for a child that ignores it.
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tauri::Listener;

    /// The whole terminal path without a window: a command runs, its output
    /// comes back, input reaches it, and the exit code survives.
    ///
    /// `tauri::test` gives a real `AppHandle`, so the events go through the
    /// same channel the front end listens on.
    #[test]
    fn a_command_runs_and_its_output_and_exit_code_come_back() {
        let app = tauri::test::mock_app();
        let terminals = Arc::new(Terminals::new());
        let (sender, received) = std::sync::mpsc::channel::<String>();
        let (exits, exited) = std::sync::mpsc::channel::<i32>();

        app.handle().listen("terminal:data", move |event| {
            if let Ok(output) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                if let Some(data) = output.get("data").and_then(|v| v.as_str()) {
                    let _ = sender.send(data.to_string());
                }
            }
        });
        app.handle().listen("terminal:exit", move |event| {
            if let Ok(exit) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                if let Some(code) = exit.get("code").and_then(|v| v.as_i64()) {
                    let _ = exits.send(code as i32);
                }
            }
        });

        let env: Vec<(String, String)> = vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("TERM".into(), "xterm-256color".into()),
        ];
        terminals
            .start(
                app.handle(),
                Launch {
                    id: "probe",
                    program: "/bin/sh",
                    args: &[
                        "-c".to_string(),
                        "read line; printf 'GOT:%s\\n' \"$line\"; exit 7".to_string(),
                    ],
                    cwd: std::path::Path::new("/"),
                    env: &env,
                    cols: 80,
                    rows: 24,
                },
            )
            .expect("spawn");

        assert!(terminals.running("probe"));
        std::thread::sleep(Duration::from_millis(200));
        terminals.write("probe", "hello\n").expect("write");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = String::new();
        while Instant::now() < deadline && !seen.contains("GOT:hello") {
            if let Ok(chunk) = received.recv_timeout(Duration::from_millis(200)) {
                seen.push_str(&chunk);
            }
        }
        assert!(seen.contains("GOT:hello"), "output was {seen:?}");

        let code = exited
            .recv_timeout(Duration::from_secs(5))
            .expect("no exit event");
        assert_eq!(code, 7, "the exit code decides a task's fate");
        assert!(!terminals.running("probe"), "the session was not cleared");
    }

    /// Stopping signals the process group, so an agent's own children go with
    /// it rather than surviving as orphans.
    #[test]
    fn stopping_takes_the_whole_process_tree() {
        let app = tauri::test::mock_app();
        let terminals = Arc::new(Terminals::new());
        let (sender, received) = std::sync::mpsc::channel::<String>();
        app.handle().listen("terminal:data", move |event| {
            if let Ok(output) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                if let Some(data) = output.get("data").and_then(|v| v.as_str()) {
                    let _ = sender.send(data.to_string());
                }
            }
        });

        let env = vec![(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )];
        terminals
            .start(
                app.handle(),
                Launch {
                    id: "tree",
                    program: "/bin/sh",
                    args: &[
                        "-c".to_string(),
                        "sleep 300 & printf 'CHILD:%s\\n' $!; sleep 300".to_string(),
                    ],
                    cwd: std::path::Path::new("/"),
                    env: &env,
                    cols: 80,
                    rows: 24,
                },
            )
            .expect("spawn");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = String::new();
        while Instant::now() < deadline && !seen.contains("CHILD:") {
            if let Ok(chunk) = received.recv_timeout(Duration::from_millis(200)) {
                seen.push_str(&chunk);
            }
        }
        let child: i32 = seen
            .split("CHILD:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| panic!("no child pid in {seen:?}"));

        terminals.stop("tree");
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            !std::path::Path::new(&format!("/proc/{child}")).exists(),
            "grandchild {child} survived the stop"
        );
    }
}
