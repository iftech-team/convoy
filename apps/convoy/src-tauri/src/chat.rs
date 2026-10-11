//! Chat sessions: the agent on JSON pipes rather than in a pty.
//!
//! The protocol lives in `convoy_core::chat`; this runs the process. One
//! thread reads stdout and hands each line to the session's driver, which
//! says what to show and what to write back. Another keeps the end of stderr,
//! which is the only explanation an agent that dies at once leaves. A third
//! waits for the exit and reports it exactly as a terminal's is reported, so
//! the queue, review and exit rules cannot tell the two apart.

use convoy_core::chat::{ChatEvent, Decision, Driver, Image};
use convoy_core::session::{mark_started, plan_chat_launch, ChatPlan};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::commands::Workspace;
use crate::pty::{Ended, Terminals};

/// How long a stop waits before it insists, as a terminal's does.
const GRACE: Duration = Duration::from_millis(1500);
/// How much of stderr is kept for an early death's explanation.
const STDERR_TAIL: usize = 4_000;

/// Where a chat's events and its ending go. Implemented by the window; the
/// tests implement it with a channel.
pub trait ChatSink: Send + Sync + 'static {
    fn event(&self, id: &str, event: &ChatEvent);
    fn ended(&self, id: &str, how: Ended);
}

/// Everything needed to start one chat.
pub struct ChatLaunch<'a> {
    pub id: &'a str,
    pub program: &'a str,
    pub args: &'a [String],
    pub cwd: &'a std::path::Path,
    /// The complete environment, as for a terminal.
    pub env: &'a [(String, String)],
    pub driver: Driver,
}

/// The parts of a running chat the reader thread and the commands share.
struct Handle {
    stdin: Mutex<Option<ChildStdin>>,
    driver: Mutex<Driver>,
    sink: Arc<dyn ChatSink>,
}

impl Handle {
    fn write(&self, lines: &[String]) -> Result<(), String> {
        if lines.is_empty() {
            return Ok(());
        }
        let mut stdin = self.stdin.lock().map_err(|_| "Chat unavailable.")?;
        let Some(stdin) = stdin.as_mut() else {
            return Err("The agent is stopping.".into());
        };
        for line in lines {
            stdin
                .write_all(line.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .map_err(|error| format!("The agent stopped reading: {error}"))?;
        }
        stdin.flush().map_err(|error| error.to_string())
    }

    fn emit(&self, id: &str, events: &[ChatEvent]) {
        for event in events {
            self.sink.event(id, event);
        }
    }
}

struct Chat {
    handle: Arc<Handle>,
    pid: u32,
    stopping: bool,
    hibernating: bool,
}

/// Every running chat, by session id.
#[derive(Default)]
pub struct Chats {
    sessions: Mutex<HashMap<String, Chat>>,
}

impl Chats {
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

    fn handle(&self, id: &str) -> Result<Arc<Handle>, String> {
        let sessions = self.sessions.lock().map_err(|_| "Chat unavailable.")?;
        sessions
            .get(id)
            .map(|chat| Arc::clone(&chat.handle))
            .ok_or_else(|| "Start or resume the target session first.".to_string())
    }

    /// Starts the agent and the threads around it. Returns its process id.
    pub fn start(
        self: &Arc<Self>,
        sink: Arc<dyn ChatSink>,
        launch: ChatLaunch<'_>,
    ) -> Result<u32, String> {
        let ChatLaunch {
            id,
            program,
            args,
            cwd,
            env,
            driver,
        } = launch;
        if self.running(id) {
            return Err("This session is already running.".into());
        }

        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            // Replaced rather than extended, as for a terminal: the agent must
            // not inherit this process's own conversation markers.
            .env_clear()
            .envs(env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            // Its own process group, so a stop reaches the tools it started.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            // A windowed program starting a console one would flash a console
            // window up and take the focus with it.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("The agent could not start: {error}"))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let handle = Arc::new(Handle {
            stdin: Mutex::new(child.stdin.take()),
            driver: Mutex::new(driver),
            sink: Arc::clone(&sink),
        });
        // Registered before any thread runs, so even an instant exit finds it.
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                id.to_string(),
                Chat {
                    handle: Arc::clone(&handle),
                    pid,
                    stopping: false,
                    hibernating: false,
                },
            );
        }

        let first = handle
            .driver
            .lock()
            .map(|mut driver| driver.start())
            .unwrap_or_default();
        // A failed write means the process is already gone; the exit says why.
        let _ = handle.write(&first);

        let (read_done, reading) = mpsc::channel::<()>();
        if let Some(stdout) = stdout {
            let handle = Arc::clone(&handle);
            let id = id.to_string();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut raw = Vec::new();
                loop {
                    raw.clear();
                    match reader.read_until(b'\n', &mut raw) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = String::from_utf8_lossy(&raw);
                    // A login shell may print its own greeting first.
                    if !line.trim_start().starts_with('{') {
                        continue;
                    }
                    let (events, replies) = match handle.driver.lock() {
                        Ok(mut driver) => {
                            let events = driver.feed(&line);
                            (events, driver.take_outgoing())
                        }
                        Err(_) => break,
                    };
                    let _ = handle.write(&replies);
                    handle.emit(&id, &events);
                }
                let _ = read_done.send(());
            });
        } else {
            let _ = read_done.send(());
        }

        let errors = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = stderr {
            let errors = Arc::clone(&errors);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match stderr.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let Ok(mut errors) = errors.lock() else { break };
                            errors.push_str(&String::from_utf8_lossy(&buffer[..read]));
                            if errors.len() > STDERR_TAIL * 2 {
                                let tail =
                                    convoy_core::json::tail(&errors, STDERR_TAIL).to_string();
                                *errors = tail;
                            }
                        }
                    }
                }
            });
        }

        let chats = Arc::clone(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            let code = child.wait().map(|status| exit_code(&status)).unwrap_or(1);
            // What the agent printed last is still worth showing. A tool it
            // left running may hold stdout open, so this is not waited out.
            let _ = reading.recv_timeout(Duration::from_secs(2));
            let how = chats.take(&id, code);
            let ready = handle
                .driver
                .lock()
                .map(|driver| driver.ready())
                .unwrap_or(true);
            if let Ended::Exited(code) = how {
                if code != 0 || !ready {
                    let said = errors
                        .lock()
                        .map(|errors| convoy_core::chat::event::clip_tail(errors.trim(), 2_000))
                        .unwrap_or_default();
                    let message = if said.is_empty() {
                        format!("The agent exited with code {code}.")
                    } else {
                        format!("The agent exited with code {code}:\n{said}")
                    };
                    sink.event(&id, &ChatEvent::Error { message });
                }
            }
            sink.ended(&id, how);
        });

        Ok(pid)
    }

    /// Removes the chat and says how it ended.
    fn take(&self, id: &str, code: i32) -> Ended {
        let chat = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id));
        match chat {
            Some(chat) if chat.hibernating => Ended::Hibernated,
            Some(chat) if chat.stopping => Ended::Stopped,
            _ => Ended::Exited(code),
        }
    }

    /// Sends a user turn, showing it and the working state first.
    pub fn send(&self, id: &str, text: &str, images: &[Image]) -> Result<(), String> {
        let handle = self.handle(id)?;
        let (lines, events) = handle
            .driver
            .lock()
            .map_err(|_| "Chat unavailable.")?
            .send(text, images);
        handle.emit(id, &events);
        handle.write(&lines)
    }

    pub fn answer(&self, id: &str, request: &str, decision: Decision) -> Result<(), String> {
        let handle = self.handle(id)?;
        let (line, resolved) = handle
            .driver
            .lock()
            .map_err(|_| "Chat unavailable.")?
            .answer(request, decision)
            .map_err(|error| error.to_string())?;
        handle.write(&[line])?;
        handle.emit(id, &[resolved]);
        Ok(())
    }

    /// Stops the current turn; the process stays for the next one.
    pub fn interrupt(&self, id: &str) -> Result<(), String> {
        let handle = self.handle(id)?;
        let lines = handle
            .driver
            .lock()
            .map_err(|_| "Chat unavailable.")?
            .interrupt();
        handle.write(&lines)
    }

    /// Stopping after an idle period since a finished turn. The process ends
    /// the same way; the bookkeeping does not.
    pub fn hibernate(self: &Arc<Self>, id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(chat) = sessions.get_mut(id) {
                chat.hibernating = true;
            }
        }
        self.stop(id);
    }

    /// Closes stdin and asks the process group to end, then insists after
    /// a grace period, as a terminal stop does.
    pub fn stop(self: &Arc<Self>, id: &str) {
        let (handle, pid) = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            let Some(chat) = sessions.get_mut(id) else {
                return;
            };
            if chat.stopping {
                return;
            }
            chat.stopping = true;
            (Arc::clone(&chat.handle), chat.pid)
        };
        // End of input is how both agents are told the conversation is over.
        if let Ok(mut stdin) = handle.stdin.lock() {
            stdin.take();
        }
        ask_to_stop(pid);
        let chats = Arc::clone(self);
        let id = id.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(GRACE);
            if chats.running(&id) {
                insist(pid);
            }
        });
    }
}

#[cfg(unix)]
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(1)
}

#[cfg(not(unix))]
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[cfg(unix)]
fn ask_to_stop(pid: u32) {
    // The whole group: the agent's own children go with it.
    unsafe {
        libc::killpg(pid as i32, libc::SIGTERM);
    }
}

/// Windows has no process groups to signal; closed input is the polite half.
#[cfg(not(unix))]
fn ask_to_stop(_pid: u32) {}

#[cfg(unix)]
fn insist(pid: u32) {
    unsafe {
        libc::killpg(pid as i32, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn insist(pid: u32) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Sessions with an agent process of either kind. One process per session:
/// a session runs in its terminal or in its chat, never both.
pub fn running_ids(terminals: &Terminals, chats: &Chats) -> Vec<String> {
    let mut ids = terminals.ids();
    ids.extend(chats.ids());
    ids
}

pub fn is_running(terminals: &Terminals, chats: &Chats, id: &str) -> bool {
    terminals.running(id) || chats.running(id)
}

// ---------------------------------------------------------------------------
// The window.
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct Payload<'a> {
    pub id: &'a str,
    pub event: &'a ChatEvent,
}

/// The window's end of one chat: events out, and the facts Convoy keeps —
/// the Codex thread id and a pull request link. The saved output is the
/// window's own snapshot of the chat, as for a terminal, so none is written
/// here to compete with it.
struct Window<R: Runtime> {
    app: AppHandle<R>,
    pull_request: Mutex<Option<String>>,
}

impl<R: Runtime> Window<R> {
    fn new(app: &AppHandle<R>) -> Self {
        Window {
            app: app.clone(),
            pull_request: Mutex::new(None),
        }
    }

    fn record(&self, id: &str, event: &ChatEvent) {
        let Some(workspace) = self.app.try_state::<Workspace>() else {
            return;
        };
        let mut changed = false;
        match event {
            ChatEvent::Session { provider_id } => {
                changed |= workspace
                    .act(|core| convoy_core::session::record_provider_id(core, id, provider_id))
                    .unwrap_or(false);
            }
            ChatEvent::TextDone { text, .. } | ChatEvent::ToolResult { output: text, .. }
                if text.contains("/pull/") || text.contains("/merge_requests/") =>
            {
                if let Some(url) = convoy_core::session::pull_request_url(text) {
                    let Ok(mut known) = self.pull_request.lock() else {
                        return;
                    };
                    if known.as_deref() != Some(url.as_str()) {
                        *known = Some(url.clone());
                        changed |= workspace
                            .act(|core| convoy_core::session::record_pull_request(core, id, &url))
                            .unwrap_or(false);
                    }
                }
            }
            _ => {}
        }
        if changed {
            let _ = self.app.emit("session:changed", id.to_string());
        }
    }
}

impl<R: Runtime> ChatSink for Window<R> {
    fn event(&self, id: &str, event: &ChatEvent) {
        let _ = self.app.emit("chat:event", Payload { id, event });
        self.record(id, event);
    }

    fn ended(&self, id: &str, how: Ended) {
        crate::pty::report_exit(&self.app, id, how);
    }
}

/// Starts a session's agent in chat mode, resuming when it has run before.
/// The same checks and preparation as a terminal start.
pub fn start_chat<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    workspace: &Workspace,
    terminals: &Terminals,
    chats: &Arc<Chats>,
) -> Result<(), String> {
    if chats.running(id) {
        return Ok(());
    }
    if terminals.running(id) {
        return Err("This session is running in the terminal. Stop it first.".into());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let storage = workspace.storage.clone();
    let ChatPlan {
        launch,
        driver,
        first_prompt,
    } = workspace.act(|core| plan_chat_launch(core, &storage, id, &executable))?;

    let env: Vec<(String, String)> = launch.env.clone().into_iter().collect();
    chats.start(
        Arc::new(Window::new(app)),
        ChatLaunch {
            id,
            program: &launch.spec.file,
            args: &launch.spec.args,
            cwd: &launch.directory,
            env: &env,
            driver,
        },
    )?;

    // Recorded only once the process exists, and stopped if recording fails:
    // an agent the workspace does not know about would be invisible.
    if let Err(error) = workspace.act(|core| mark_started(core, &launch)) {
        chats.stop(id);
        return Err(error);
    }
    if let Some(prompt) = first_prompt {
        chats.send(id, &prompt, &[])?;
    }
    Ok(())
}

#[tauri::command]
pub fn chat_start(
    app: AppHandle,
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    start_chat(&app, &id, &workspace, &terminals, &chats)
}

/// The conversation so far, from the agent's own transcript. Read off the
/// main thread: a long conversation is megabytes of JSON.
#[tauri::command]
pub async fn chat_history(
    id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<ChatEvent>, String> {
    let storage = workspace.storage.clone();
    let source = workspace.act(|core| convoy_core::chat::history::source(core, &storage, &id))?;
    let Some(source) = source else {
        return Ok(Vec::new());
    };
    tauri::async_runtime::spawn_blocking(move || convoy_core::chat::history::read(&source))
        .await
        .map_err(|error| error.to_string())
}

/// Sends a user turn, starting the agent first when it is not running.
#[tauri::command]
pub fn chat_send(
    app: AppHandle,
    id: String,
    text: String,
    images: Option<Vec<Image>>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let images = images.unwrap_or_default();
    if text.trim().is_empty() && images.is_empty() {
        return Err("Type a message first.".into());
    }
    if convoy_core::json::utf16_len(&text) > 100_000 {
        return Err("The message is too long.".into());
    }
    start_chat(&app, &id, &workspace, &terminals, &chats)?;
    chats.send(&id, &text, &images)
}

#[tauri::command]
pub fn chat_answer(
    id: String,
    request: String,
    decision: String,
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let decision = Decision::parse(&decision).ok_or("Unknown decision.")?;
    chats.answer(&id, &request, decision)
}

#[tauri::command]
pub fn chat_interrupt(id: String, chats: State<'_, Arc<Chats>>) -> Result<(), String> {
    chats.interrupt(&id)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use convoy_core::model::{Agent, PublishMode, TaskStatus};
    use convoy_core::planning::TaskInput;
    use convoy_core::{Storage, Workspace as CoreWorkspace};
    use tauri::Listener;

    /// A script that prints a recording's agent side, as the agent would.
    fn agent_script(dir: &std::path::Path, agent: &str, name: &str, exit: i32) -> String {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../linux/crates/convoy-core/tests/fixtures/chat")
            .join(agent)
            .join(format!("{name}.ndjson"));
        let lines: Vec<String> = std::fs::read_to_string(fixture)
            .unwrap()
            .lines()
            .filter_map(|line| {
                let record: serde_json::Value = serde_json::from_str(line).ok()?;
                (record["dir"] == "in").then(|| record["msg"].to_string())
            })
            .collect();
        let file = dir.join(format!("{agent}-{name}.ndjson"));
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        // Read the first line we send, then answer with the recording.
        format!(
            "read -r first; printf 'not json from a login shell\\n'; cat '{}'; exit {exit}",
            file.display()
        )
    }

    fn task_session(storage: &Storage, project: &std::path::Path) -> String {
        let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        core.add_project(project).unwrap();
        let project_id = core.state().projects[0].id.clone();
        core.save_task(TaskInput {
            id: None,
            project_id,
            spec_id: None,
            title: "Chat task".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::None,
            auto_review: false,
        })
        .unwrap();
        let task_id = core.state().tasks[0].id.clone();
        core.prepare_task(&task_id, None).unwrap();
        core.state().tasks[0].session_id.clone().unwrap()
    }

    /// The manager against a fake agent: the recording's events reach the
    /// window in order, and the exit goes through
    /// the same path a terminal's does — a clean one sends the task to review.
    #[test]
    fn a_chat_streams_events_and_its_exit_finishes_the_session() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let session_id = task_session(&storage, &project);

        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        let (events_in, events) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("chat:event", move |event| {
            let _ = events_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let (exits_in, exits) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("terminal:exit", move |event| {
            let _ = exits_in.send(serde_json::from_str(event.payload()).unwrap());
        });

        let chats = Arc::new(Chats::new());
        let script = agent_script(root.path(), "claude", "approval-allow", 0);
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        chats
            .start(
                Arc::new(Window::new(app.handle())),
                ChatLaunch {
                    id: &session_id,
                    program: "/bin/sh",
                    args: &["-c".to_string(), script],
                    cwd: &project,
                    env: &env,
                    driver: Driver::Claude(convoy_core::chat::claude::ClaudeStream::new("s")),
                },
            )
            .unwrap();
        assert!(chats.running(&session_id));

        let exit = exits
            .recv_timeout(Duration::from_secs(10))
            .expect("no exit event");
        assert_eq!(exit["id"], session_id.as_str());
        assert_eq!(exit["cause"], "exited");
        assert_eq!(exit["clean"], true);
        assert!(!chats.running(&session_id));

        let kinds: Vec<String> = events
            .try_iter()
            .map(|payload| {
                assert_eq!(payload["id"], session_id.as_str());
                payload["event"]["type"].as_str().unwrap().to_string()
            })
            .collect();
        for wanted in [
            "session",
            "turn_started",
            "tool",
            "approval",
            "text_done",
            "turn_ended",
        ] {
            assert!(
                kinds.iter().any(|kind| kind == wanted),
                "{wanted} missing: {kinds:?}"
            );
        }
        assert!(
            kinds.iter().position(|k| k == "approval")
                < kinds.iter().position(|k| k == "turn_ended")
        );
        assert!(!kinds.iter().any(|kind| kind == "error"), "{kinds:?}");

        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(core.state().tasks[0].status, TaskStatus::Review);
    }

    /// An agent that dies before its handshake explains itself through
    /// stderr, and the exit is a failure.
    #[test]
    fn an_early_death_is_reported_with_what_it_said() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let session_id = task_session(&storage, &project);

        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        let (events_in, events) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("chat:event", move |event| {
            let _ = events_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let (exits_in, exits) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("terminal:exit", move |event| {
            let _ = exits_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let chats = Arc::new(Chats::new());
        chats
            .start(
                Arc::new(Window::new(app.handle())),
                ChatLaunch {
                    id: &session_id,
                    program: "/bin/sh",
                    args: &[
                        "-c".to_string(),
                        "echo 'claude: command not found' >&2; exit 127".to_string(),
                    ],
                    cwd: &project,
                    env: &[],
                    driver: Driver::Claude(convoy_core::chat::claude::ClaudeStream::new("s")),
                },
            )
            .unwrap();
        let exit = exits.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(exit["code"], 127);
        assert_eq!(exit["clean"], false);
        let error = events
            .try_iter()
            .find(|payload| payload["event"]["type"] == "error")
            .expect("no error event");
        assert!(error["event"]["message"]
            .as_str()
            .unwrap()
            .contains("claude: command not found"));
        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(core.state().tasks[0].status, TaskStatus::Failed);
    }

    /// A stop ends a chat that would otherwise run for ever, and it counts as
    /// a stop, not an exit; a turn sent before then is written to the agent.
    #[test]
    fn a_stop_ends_a_waiting_agent_as_stopped() {
        let root = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        let (exits_in, exits) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("terminal:exit", move |event| {
            let _ = exits_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let (events_in, events) = mpsc::channel::<serde_json::Value>();
        app.handle().listen("chat:event", move |event| {
            let _ = events_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let received = root.path().join("received");
        let chats = Arc::new(Chats::new());
        chats
            .start(
                Arc::new(Window::new(app.handle())),
                ChatLaunch {
                    id: "s1",
                    program: "/bin/sh",
                    args: &[
                        "-c".to_string(),
                        format!("exec cat > '{}'", received.display()),
                    ],
                    cwd: root.path(),
                    env: &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
                    driver: Driver::Claude(convoy_core::chat::claude::ClaudeStream::new("s")),
                },
            )
            .unwrap();
        chats.send("s1", "hello", &[]).unwrap();
        // Stopped only once the turn reached the agent.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !std::fs::read_to_string(&received).is_ok_and(|text| text.contains("hello")) {
            assert!(
                std::time::Instant::now() < deadline,
                "the turn never arrived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        chats.stop("s1");
        let exit = exits.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(exit["cause"], "stopped");
        assert_eq!(exit["clean"], false);
        let kinds: Vec<serde_json::Value> = events
            .try_iter()
            .map(|p| p["event"]["type"].clone())
            .collect();
        assert_eq!(kinds, ["user", "turn_started"]);
        let written = std::fs::read_to_string(&received).unwrap();
        let lines: Vec<serde_json::Value> = written
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[0]["request"]["subtype"], "initialize");
        assert_eq!(lines[1]["message"]["content"], "hello");
        assert!(chats.send("s1", "again", &[]).is_err());
    }
}
