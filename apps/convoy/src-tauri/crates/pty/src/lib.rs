//! The terminal, with nothing above it.
//!
//! This crate knows how to run an agent in a pty and how to stop it, and
//! nothing about windows, workspaces or tasks. That is not tidiness for its
//! own sake: a test binary that links a web view cannot even be loaded on
//! Windows, so the one place where Windows behaviour differs most — ConPTY —
//! could not be tested on the platform it exists for.
//!
//! Output is pushed rather than polled. An agent can emit thousands of lines a
//! second, and a poll would either lag behind it or spin.

use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// How a session ended. The three are not interchangeable: a stop is a
/// failure, hibernation is not, and only an exit carries a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Exited(i32),
    /// The user pressed Stop.
    Stopped,
    /// Stopped because it had been idle since it reported a finished turn.
    Hibernated,
}

/// Where a session's output and its ending go. Implemented by the app; the
/// tests implement it with a channel.
pub trait Sink: Send + Sync + 'static {
    fn data(&self, id: &str, data: &str);
    fn ended(&self, id: &str, how: Ended);
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

/// One running agent: the pty it owns, the handle to write into it, and the
/// process-group leader used to stop the whole tree.
struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
    stopping: bool,
    hibernating: bool,
}

#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
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

    /// Starts a command in a new pty and streams its output to `sink`.
    /// Returns the child's process id.
    pub fn start(self: &Arc<Self>, sink: Arc<dyn Sink>, launch: Launch<'_>) -> Result<u32, String> {
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
        let session_id = id.to_string();
        let terminals = Arc::clone(self);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => sink.data(&session_id, &String::from_utf8_lossy(&buffer[..read])),
                }
            }
            let code = child
                .wait()
                .map(|status| status.exit_code() as i32)
                .unwrap_or(1);
            sink.ended(&session_id, terminals.take(&session_id, code));
        });

        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                id.to_string(),
                Session {
                    master,
                    writer,
                    pid,
                    stopping: false,
                    hibernating: false,
                },
            );
        }
        Ok(pid.unwrap_or(0))
    }

    /// Removes the session and says how it ended, which is what decides the
    /// fate of the task behind it.
    fn take(&self, id: &str, code: i32) -> Ended {
        let session = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id));
        match session {
            Some(session) if session.hibernating => Ended::Hibernated,
            Some(session) if session.stopping => Ended::Stopped,
            _ => Ended::Exited(code),
        }
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

    /// Stopping because the agent has been idle since it reported a finished
    /// turn. The process ends the same way; the bookkeeping does not.
    pub fn hibernate(self: &Arc<Self>, id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.get_mut(id) {
                session.hibernating = true;
            }
        }
        self.stop(id);
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
fn signal_group(pid: u32, signal: Signal) {
    use std::os::windows::process::CommandExt;

    // ConPTY has no process groups, so the tree is walked by `taskkill /T`.
    // Without `/F` it asks; with it, it does not. The two steps of `stop` map
    // onto that, so an agent still gets its moment to save state.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T"]);
    if matches!(signal, Signal::Kill) {
        command.arg("/F");
    }
    // Convoy has no console of its own, so taskkill would flash one up.
    let _ = command.creation_flags(CREATE_NO_WINDOW).status();
}
