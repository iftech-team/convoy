//! Chat sessions: the agent on JSON pipes rather than in a pty.
//!
//! The protocol lives in `convoy_core::chat`; this runs the process. One
//! thread reads stdout and hands each line to the session's driver, which
//! says what to show and what to write back. Another keeps the end of stderr,
//! which is the only explanation an agent that dies at once leaves. A third
//! waits for the exit and reports it exactly as a terminal's is reported, so
//! the queue, review and exit rules cannot tell the two apart.

use convoy_core::chat::{ChatEvent, Decision, Driver, Image, PendingApprovals};
use convoy_core::session::{mark_started, plan_chat_launch_with, ChatPlan};
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
    /// What the agent is waiting on, for a window that was not listening.
    pending: Mutex<PendingApprovals>,
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

    /// Every event goes through here, so the waiting approvals can never
    /// disagree with what the window was told.
    fn emit(&self, id: &str, events: &[ChatEvent]) {
        if let Ok(mut pending) = self.pending.lock() {
            let now = now_ms();
            for event in events {
                pending.observe(event, now);
            }
        }
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
    /// The last list of models each session's agent reported, kept after it
    /// stops so a window opened later can offer them.
    models: Mutex<HashMap<String, ChatEvent>>,
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
            pending: Mutex::new(PendingApprovals::default()),
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
            let chats = Arc::clone(self);
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
                    if let Some(listed) = events
                        .iter()
                        .rev()
                        .find(|event| matches!(event, ChatEvent::Models { .. }))
                    {
                        chats.remember_models(&id, listed.clone());
                    }
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

    /// Every running chat's waiting approvals, oldest first.
    pub fn approvals(&self) -> Vec<WaitingApproval> {
        let handles: Vec<(String, Arc<Handle>)> = match self.sessions.lock() {
            Ok(sessions) => sessions
                .iter()
                .map(|(id, chat)| (id.clone(), Arc::clone(&chat.handle)))
                .collect(),
            Err(_) => return Vec::new(),
        };
        let mut list: Vec<WaitingApproval> = handles
            .into_iter()
            .filter_map(|(id, handle)| {
                let pending = handle.pending.lock().ok()?;
                Some(
                    pending
                        .list()
                        .iter()
                        .map(|item| WaitingApproval {
                            id: id.clone(),
                            at: item.at,
                            event: item.event.clone(),
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .flatten()
            .collect();
        list.sort_by_key(|item| item.at);
        list
    }

    /// Removes the chat and says how it ended. Nothing it asked can be
    /// answered any more.
    fn take(&self, id: &str, code: i32) -> Ended {
        let chat = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(id));
        if let Some(chat) = &chat {
            if let Ok(mut pending) = chat.handle.pending.lock() {
                pending.clear();
            }
        }
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

    fn remember_models(&self, id: &str, event: ChatEvent) {
        if let Ok(mut models) = self.models.lock() {
            models.insert(id.to_string(), event);
        }
    }

    /// The last list of models the session's agent reported, if any.
    pub fn models(&self, id: &str) -> Option<ChatEvent> {
        self.models.lock().ok()?.get(id).cloned()
    }

    /// Switches a running chat's model. Nothing to do when none runs: the
    /// next launch reads the session.
    pub fn set_model(&self, id: &str, model: Option<&str>) -> Result<(), String> {
        let Ok(handle) = self.handle(id) else {
            return Ok(());
        };
        let lines = handle
            .driver
            .lock()
            .map_err(|_| "Chat unavailable.")?
            .set_model(model);
        handle.write(&lines)
    }

    /// Switches a running chat's effort level, as [`Chats::set_model`].
    pub fn set_effort(&self, id: &str, effort: Option<&str>) -> Result<(), String> {
        let Ok(handle) = self.handle(id) else {
            return Ok(());
        };
        let lines = handle
            .driver
            .lock()
            .map_err(|_| "Chat unavailable.")?
            .set_effort(effort);
        handle.write(&lines)
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

/// One approval a running chat waits on, as `chat_approvals` returns it.
#[derive(Debug, Clone, Serialize)]
pub struct WaitingApproval {
    /// The Convoy session.
    pub id: String,
    /// When it arrived, in milliseconds since the Unix epoch.
    pub at: u64,
    /// The `approval` event the window was sent.
    pub event: ChatEvent,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
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
        crate::helpers::chat_event(&self.app, id, event);
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
    // A manager is told how to start helpers, with a secret of its own.
    let helpers = app.try_state::<crate::helpers::Helpers>();
    let manager = helpers
        .as_ref()
        .and_then(|helpers| helpers.launch_for(workspace, id));
    let planned = workspace
        .act(|core| plan_chat_launch_with(core, &storage, id, &executable, manager.as_ref()));
    let ChatPlan {
        launch,
        driver,
        first_prompt,
    } = match planned {
        Ok(plan) => plan,
        Err(error) => {
            if let Some(helpers) = &helpers {
                helpers.revoke(id);
            }
            return Err(error);
        }
    };

    let env: Vec<(String, String)> = launch.env.clone().into_iter().collect();
    #[allow(unused_mut)]
    let (mut program, mut args) = (launch.spec.file.clone(), launch.spec.args.clone());
    #[cfg(test)]
    if let Some(fake) = app.try_state::<FakeAgent>() {
        if let Ok(fake) = fake.0.lock() {
            (program, args) = fake.clone();
        }
    }
    if let Err(error) = chats.start(
        Arc::new(Window::new(app)),
        ChatLaunch {
            id,
            program: &program,
            args: &args,
            cwd: &launch.directory,
            env: &env,
            driver,
        },
    ) {
        if let Some(helpers) = &helpers {
            helpers.revoke(id);
        }
        return Err(error);
    }

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

/// A stand-in for the agent a chat launch would run, for tests that go
/// through [`start_chat`] itself.
#[cfg(test)]
#[derive(Default)]
pub struct FakeAgent(pub Mutex<(String, Vec<String>)>);

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

/// Every approval a running chat waits on, across sessions, so a reloaded
/// window — or one that never opened the session — still knows them.
#[tauri::command]
pub fn chat_approvals(chats: State<'_, Arc<Chats>>) -> Vec<WaitingApproval> {
    chats.approvals()
}

#[tauri::command]
pub fn chat_interrupt(id: String, chats: State<'_, Arc<Chats>>) -> Result<(), String> {
    chats.interrupt(&id)
}

/// The models the session's agent last listed, with the session's own
/// choice; none before its chat has run in this window's lifetime.
#[tauri::command]
pub fn chat_models(
    id: String,
    workspace: State<'_, Workspace>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Option<ChatEvent>, String> {
    let Some(ChatEvent::Models { models, .. }) = chats.models(&id) else {
        return Ok(None);
    };
    let (model, effort) = workspace.with(|core| {
        core.session(&id)
            .map(|session| (session.model.clone(), session.effort.clone()))
            .map_err(|error| error.to_string())
    })?;
    Ok(Some(ChatEvent::Models {
        models,
        model: model.filter(|model| !model.is_empty()),
        effort,
    }))
}

/// Chooses the session's model: kept for every later launch, and switched at
/// once in a running chat. None is the agent's default.
#[tauri::command]
pub fn chat_set_model(
    app: AppHandle,
    id: String,
    model: Option<String>,
    workspace: State<'_, Workspace>,
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let model = model.filter(|model| !model.is_empty());
    workspace.act(|core| core.set_session_model(&id, model.as_deref()).map(|_| ()))?;
    chats.set_model(&id, model.as_deref())?;
    let _ = app.emit("session:changed", id);
    Ok(())
}

/// Chooses how hard the session's agent thinks, as [`chat_set_model`].
#[tauri::command]
pub fn chat_set_effort(
    app: AppHandle,
    id: String,
    effort: Option<String>,
    workspace: State<'_, Workspace>,
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let effort = effort.filter(|effort| !effort.is_empty());
    workspace.act(|core| core.set_session_effort(&id, effort.as_deref()).map(|_| ()))?;
    chats.set_effort(&id, effort.as_deref())?;
    let _ = app.emit("session:changed", id);
    Ok(())
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

    /// The models the agent lists are kept for a window that asks later, and
    /// a switch reaches the running agent as a control request.
    #[test]
    fn the_listed_models_are_kept_and_a_switch_is_written() {
        let root = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        let received = root.path().join("received");
        let init = serde_json::json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": "init", "response": {
                "models": [{ "value": "opus", "displayName": "Opus", "supportsEffort": true, "supportedEffortLevels": ["low", "high"] }]
            } }
        });
        let chats = Arc::new(Chats::new());
        chats
            .start(
                Arc::new(Window::new(app.handle())),
                ChatLaunch {
                    id: "s1",
                    program: "/bin/sh",
                    args: &[
                        "-c".to_string(),
                        format!(
                            "printf '%s\\n' '{init}'; exec cat > '{}'",
                            received.display()
                        ),
                    ],
                    cwd: root.path(),
                    env: &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
                    driver: Driver::Claude(convoy_core::chat::claude::ClaudeStream::new("s")),
                },
            )
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while chats.models("s1").is_none() {
            assert!(std::time::Instant::now() < deadline, "no models");
            std::thread::sleep(Duration::from_millis(20));
        }
        let Some(ChatEvent::Models { models, .. }) = chats.models("s1") else {
            unreachable!()
        };
        assert_eq!(models[0].id, "opus");
        assert_eq!(models[0].efforts, ["low", "high"]);

        chats.set_model("s1", Some("opus")).unwrap();
        chats.set_effort("s1", Some("high")).unwrap();
        while !std::fs::read_to_string(&received).is_ok_and(|text| text.contains("effortLevel")) {
            assert!(
                std::time::Instant::now() < deadline,
                "the switch never arrived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let written = std::fs::read_to_string(&received).unwrap();
        let lines: Vec<serde_json::Value> = written
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines[1]["request"]["subtype"], "set_model");
        assert_eq!(lines[1]["request"]["model"], "opus");
        assert_eq!(lines[2]["request"]["settings"]["effortLevel"], "high");
        chats.stop("s1");
        // Kept after the process ends; a session not running has nothing to switch.
        while chats.running("s1") {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(chats.models("s1").is_some());
        assert!(chats.set_model("s1", None).is_ok());
    }

    /// The approvals a chat waits on are kept for a window that was not
    /// listening: listed across sessions, gone once answered, and gone with
    /// the process.
    #[test]
    fn waiting_approvals_are_listed_until_answered_or_the_agent_ends() {
        let root = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        let ask = |request: &str, command: &str| {
            serde_json::json!({
                "type": "control_request",
                "request_id": request,
                "request": {
                    "subtype": "can_use_tool",
                    "tool_name": "Bash",
                    "input": { "command": command },
                    "permission_suggestions": [{ "type": "addRules", "destination": "session" }]
                }
            })
            .to_string()
        };
        let chats = Arc::new(Chats::new());
        let start = |id: &str, lines: Vec<String>| {
            let printed: String = lines
                .iter()
                .map(|line| format!("printf '%s\\n' '{line}'; "))
                .collect();
            chats
                .start(
                    Arc::new(Window::new(app.handle())),
                    ChatLaunch {
                        id,
                        program: "/bin/sh",
                        args: &[
                            "-c".to_string(),
                            format!("{printed}exec cat > '{}'", root.path().join(id).display()),
                        ],
                        cwd: root.path(),
                        env: &[("PATH".to_string(), "/usr/bin:/bin".to_string())],
                        driver: Driver::Claude(convoy_core::chat::claude::ClaudeStream::new("s")),
                    },
                )
                .unwrap();
        };
        let before = now_ms();
        start("s1", vec![ask("r1", "npm test"), ask("r2", "git push")]);
        start("s2", vec![ask("r3", "rm -rf build")]);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while chats.approvals().len() < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "{:?}",
                chats.approvals()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let listed = chats.approvals();
        let mut seen: Vec<(String, String)> = listed
            .iter()
            .map(|item| {
                assert!(item.at >= before);
                let ChatEvent::Approval {
                    request, always, ..
                } = &item.event
                else {
                    panic!("not an approval: {:?}", item.event)
                };
                assert!(always);
                (item.id.clone(), request.clone())
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            [
                ("s1".into(), "r1".into()),
                ("s1".into(), "r2".into()),
                ("s2".into(), "r3".into())
            ]
        );
        assert!(listed.windows(2).all(|pair| pair[0].at <= pair[1].at));
        let value = serde_json::to_value(&listed[0]).unwrap();
        assert_eq!(value["event"]["type"], "approval");
        assert!(value["at"].is_u64());

        // Answered: gone from the list, the rest still waiting.
        chats.answer("s1", "r1", Decision::Allow).unwrap();
        let requests = |chats: &Chats| -> Vec<String> {
            chats
                .approvals()
                .into_iter()
                .filter_map(|item| match item.event {
                    ChatEvent::Approval { request, .. } => Some(request),
                    _ => None,
                })
                .collect()
        };
        let mut left = requests(&chats);
        left.sort();
        assert_eq!(left, ["r2", "r3"]);

        // The process ended: nothing it asked can be answered.
        chats.stop("s2");
        while chats.running("s2") {
            assert!(std::time::Instant::now() < deadline, "s2 never stopped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(requests(&chats), ["r2"]);
        chats.stop("s1");
        while chats.running("s1") {
            assert!(std::time::Instant::now() < deadline, "s1 never stopped");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(chats.approvals().is_empty());
    }
}
