//! Manager sessions' helpers, as the running app serves them.
//!
//! The rules are `convoy_core::helpers`; this runs them. A thread picks up
//! the request files a manager's `convoy --session …` writes, every half
//! second whether or not the window is open, and answers each with a result
//! file. Helpers start as chats, so their events — and their approvals —
//! reach the window like any other chat's.

use convoy_core::helpers::{
    self, authorize, children, create_helper, own_helpers, plan_spawn, Command, Exit, HelperView,
    ManagerLaunch, Request, Response, Settled, Tokens, Tracker,
};
use convoy_core::model::ActivityKind;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::chat::{is_running, Chats};
use crate::commands::{Repositories, Workspace};
use crate::pty::{Ended, Terminals};

/// How often requests are looked for.
const POLL: Duration = Duration::from_millis(500);

/// The secrets of running managers and what is known of their helpers.
pub struct Helpers {
    tokens: Mutex<Tokens>,
    tracker: Mutex<Tracker>,
    /// The binary a manager runs: this one, re-entered in `--session` mode.
    bin: PathBuf,
}

impl Default for Helpers {
    fn default() -> Self {
        Helpers::with_bin(std::env::current_exe().unwrap_or_default())
    }
}

impl Helpers {
    pub fn with_bin(bin: PathBuf) -> Self {
        Helpers {
            tokens: Mutex::new(Tokens::default()),
            tracker: Mutex::new(Tracker::default()),
            bin,
        }
    }

    /// A fresh secret and what goes with it, for a session that is a
    /// manager; nothing for any other.
    pub fn launch_for(&self, workspace: &Workspace, id: &str) -> Option<ManagerLaunch> {
        let manager = workspace
            .with(|core| {
                Ok(core
                    .session(id)
                    .is_ok_and(|session| session.is_manager() && session.parent_id.is_none()))
            })
            .unwrap_or(false);
        if !manager {
            return None;
        }
        let token = self.tokens.lock().ok()?.issue(id);
        Some(ManagerLaunch {
            token,
            bin: self.bin.clone(),
            requests: helpers::requests_dir(&workspace.storage),
        })
    }

    /// The manager stopped: its secret goes with it.
    pub fn revoke(&self, id: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.revoke(id);
        }
    }

    fn status(&self, id: &str, running: bool) -> helpers::HelperStatus {
        self.tracker
            .lock()
            .map(|tracker| tracker.status(id, running))
            .unwrap_or(helpers::HelperStatus::Running)
    }

    fn last_text(&self, id: &str) -> Option<String> {
        self.tracker
            .lock()
            .ok()?
            .get(id)
            .and_then(|tracked| tracked.last_text.clone())
    }
}

/// Every chat event passes here; a helper's finished turn is recorded on its
/// manager.
pub fn chat_event<R: Runtime>(app: &AppHandle<R>, id: &str, event: &convoy_core::chat::ChatEvent) {
    let Some(helpers) = app.try_state::<Helpers>() else {
        return;
    };
    let settled = helpers
        .tracker
        .lock()
        .ok()
        .and_then(|mut tracker| tracker.observe(id, event));
    if let Some(settled) = settled {
        record_settled(app, &settled);
    }
}

/// Every agent process ending passes here, terminal or chat.
pub fn process_ended<R: Runtime>(app: &AppHandle<R>, id: &str, how: Ended) {
    let Some(helpers) = app.try_state::<Helpers>() else {
        return;
    };
    helpers.revoke(id);
    let exit = match how {
        Ended::Exited(code) => Exit::Code(code),
        Ended::Stopped | Ended::Hibernated => Exit::Stopped,
    };
    let settled = helpers
        .tracker
        .lock()
        .ok()
        .and_then(|mut tracker| tracker.ended(id, exit));
    if let Some(settled) = settled {
        record_settled(app, &settled);
    }
}

fn record_settled<R: Runtime>(app: &AppHandle<R>, settled: &Settled) {
    let Some(workspace) = app.try_state::<Workspace>() else {
        return;
    };
    let (kind, word) = match settled.status {
        helpers::HelperStatus::Done => (ActivityKind::Done, "Helper finished"),
        helpers::HelperStatus::Stopped => (ActivityKind::Exited, "Helper stopped"),
        _ => (ActivityKind::Exited, "Helper failed"),
    };
    let _ = workspace.act(|core| {
        let title = core.session(&settled.child)?.title.clone();
        core.record(kind, &settled.parent, &format!("{word}: {title}"))
            .map(|_| ())
    });
    let _ = app.emit("session:changed", settled.parent.clone());
}

/// Starts the thread that answers requests. It runs until the app exits.
pub fn start_poller<R: Runtime>(app: AppHandle<R>) {
    std::thread::Builder::new()
        .name("convoy-helpers".into())
        .spawn(move || {
            let mut turns: u64 = 0;
            loop {
                poll_once(&app, turns % 120 == 0);
                turns = turns.wrapping_add(1);
                std::thread::sleep(POLL);
            }
        })
        .ok();
}

/// One pass: say the app is here, tidy now and then, answer what waits.
pub fn poll_once<R: Runtime>(app: &AppHandle<R>, tidy: bool) {
    let Some(workspace) = app.try_state::<Workspace>() else {
        return;
    };
    let dir = helpers::requests_dir(&workspace.storage);
    let _ = helpers::heartbeat(&dir);
    if tidy {
        helpers::cleanup(&dir, std::time::SystemTime::now());
    }
    for path in helpers::pending(&dir) {
        answer_file(app, &dir, &path);
    }
}

fn answer_file<R: Runtime>(app: &AppHandle<R>, dir: &Path, path: &Path) {
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".json"))
        .unwrap_or_default()
        .to_string();
    let response = match helpers::read_request(path) {
        Ok(request) if request.id != stem => Response::error("Invalid request."),
        Ok(request) if helpers::now_ms().saturating_sub(request.at) > helpers::STALE_MS => {
            Response::error("The request expired.")
        }
        Ok(request) => handle(app, &request),
        Err(error) => Response::error(error.to_string()),
    };
    // Removed first, so a slow write can never have it answered twice.
    let _ = std::fs::remove_file(path);
    let _ = helpers::write_result(dir, &stem, &response);
}

/// Answers one request.
pub fn handle<R: Runtime>(app: &AppHandle<R>, request: &Request) -> Response {
    let (Some(workspace), Some(helpers), Some(terminals), Some(chats)) = (
        app.try_state::<Workspace>(),
        app.try_state::<Helpers>(),
        app.try_state::<Arc<Terminals>>(),
        app.try_state::<Arc<Chats>>(),
    ) else {
        return Response::error("Convoy is starting. Try again in a moment.");
    };
    let caller = {
        let Ok(tokens) = helpers.tokens.lock() else {
            return Response::error("Convoy is busy.");
        };
        match workspace.act(|core| authorize(core, &tokens, request).cloned()) {
            Ok(caller) => caller,
            Err(error) => return Response::error(error),
        }
    };
    let running = |id: &str| is_running(&terminals, &chats, id);
    let view = |id: &str| -> Result<HelperView, String> {
        let (title, agent) = workspace.act(|core| {
            let session = core.session(id)?;
            Ok((session.title.clone(), session.agent.as_str().to_string()))
        })?;
        Ok(HelperView {
            id: id.to_string(),
            status: helpers.status(id, running(id)),
            title,
            agent,
        })
    };
    let result: Result<serde_json::Value, String> = match &request.command {
        Command::Spawn(spawn) => spawn_helper(
            app, &workspace, &helpers, &terminals, &chats, &caller, spawn,
        )
        .map(|id| json!({ "id": id })),
        Command::Status { ids } => workspace
            .act(|core| own_helpers(core, &caller.id, ids))
            .and_then(|_| ids.iter().map(|id| view(id)).collect::<Result<Vec<_>, _>>())
            .map(|list| json!(list)),
        Command::List => workspace
            .with(|core| {
                Ok(children(core, &caller.id)
                    .iter()
                    .map(|session| session.id.clone())
                    .collect::<Vec<_>>())
            })
            .and_then(|ids| ids.iter().map(|id| view(id)).collect::<Result<Vec<_>, _>>())
            .map(|list| json!(list)),
        Command::Output { id } => workspace
            .act(|core| own_helpers(core, &caller.id, std::slice::from_ref(id)))
            .and_then(|_| output_of(&workspace, &helpers, id))
            .map(|text| json!({ "text": text })),
    };
    match result {
        Ok(value) => Response::value(value),
        Err(error) => Response::error(error),
    }
}

/// The helper's last answer: what this app saw last, else its transcript.
fn output_of(workspace: &Workspace, helpers: &Helpers, id: &str) -> Result<String, String> {
    let storage = workspace.storage.clone();
    let from_transcript = || -> Result<Option<String>, String> {
        let source =
            workspace.act(|core| convoy_core::chat::history::source(core, &storage, id))?;
        Ok(source
            .and_then(|source| helpers::last_answer(&convoy_core::chat::history::read(&source))))
    };
    let text = match helpers.last_text(id) {
        Some(text) => Some(text),
        None => from_transcript()?,
    };
    text.ok_or_else(|| "This helper has not answered yet.".to_string())
}

/// Creates the helper, gives it its worktree when asked, and starts it as a
/// chat with the prompt as its first message.
#[allow(clippy::too_many_arguments)]
fn spawn_helper<R: Runtime>(
    app: &AppHandle<R>,
    workspace: &Workspace,
    helpers: &Helpers,
    terminals: &Terminals,
    chats: &Arc<Chats>,
    caller: &convoy_core::model::Session,
    spawn: &helpers::Spawn,
) -> Result<String, String> {
    let alive: Vec<String> = crate::chat::running_ids(terminals, chats);
    let plan = workspace.act(|core| {
        // Working ones count. A helper whose turn ended keeps its process
        // for a follow-up but is not using the agent.
        let running = children(core, &caller.id)
            .iter()
            .filter(|session| {
                alive.contains(&session.id)
                    && helpers.status(&session.id, true) == helpers::HelperStatus::Running
            })
            .count();
        plan_spawn(core, caller, spawn, running)
    })?;
    // The automatic skills, and any the manager named.
    let mut plan = plan;
    plan.session.skills = crate::skills::with_skills(&workspace.storage, |store| {
        let mut ids = store.defaults(&caller.project_id, Some(plan.session.agent));
        for id in store.named(&caller.project_id, plan.session.agent, &spawn.skills)? {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
    })?;
    let id = workspace.act(|core| create_helper(core, &plan))?;

    if let Some(branch) = &plan.branch {
        if let Err(error) = make_worktree(app, workspace, &id, branch) {
            let _ =
                workspace.act(|core| core.remove_sessions(std::slice::from_ref(&id)).map(|_| ()));
            return Err(format!("Could not create the helper's worktree: {error}"));
        }
    }

    // Followed before it starts, so its first events are not missed.
    if let Ok(mut tracker) = helpers.tracker.lock() {
        tracker.track(&id, &caller.id);
    }
    if let Err(error) = crate::chat::start_chat(app, &id, workspace, terminals, chats) {
        if let Ok(mut tracker) = helpers.tracker.lock() {
            tracker.ended(&id, Exit::Code(1));
        }
        let _ = app.emit("session:changed", id.clone());
        return Err(format!(
            "The helper was created but could not start: {error}"
        ));
    }
    let title = plan.session.title.clone();
    let _ = workspace.act(|core| {
        core.record(
            ActivityKind::Started,
            &caller.id,
            &format!("Started helper: {title}"),
        )
        .map(|_| ())
    });
    let _ = app.emit("session:spawned", json!({ "id": id, "parent": caller.id }));
    Ok(id)
}

/// The helper's own checkout, made as the New session sheet makes one; the
/// shared files are copied, the setup command is not run unattended.
pub(crate) fn make_worktree<R: Runtime>(
    app: &AppHandle<R>,
    workspace: &Workspace,
    id: &str,
    branch: &str,
) -> Result<(), String> {
    let root = workspace.storage.worktrees();
    let plan =
        workspace.act(|core| convoy_core::worktree::plan_create(core, id, branch, &|_| false))?;
    let create = || {
        convoy_core::Git::default()
            .create_worktree_from(
                &plan.project_path,
                &root,
                &plan.branch,
                plan.base_ref.as_deref(),
            )
            .map_err(|error| error.to_string())
    };
    let directory = match app.try_state::<Repositories>() {
        Some(repositories) => repositories.with(&plan.project_path, create)?,
        None => create()?,
    };
    workspace
        .act(|core| convoy_core::worktree::record_create(core, id, &directory, &plan.branch))?;
    if !plan.shared_paths.is_empty() {
        let _ = convoy_core::worktree::setup(
            &plan.project_path,
            &directory,
            &plan.shared_paths,
            None,
            &convoy_core::StdRunner,
        );
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::chat::FakeAgent;
    use convoy_core::helpers::cli::{self, Timing};
    use convoy_core::helpers::{HelperStatus, Spawn};
    use convoy_core::model::Agent;
    use convoy_core::workspace::{NewSession, SettingsPatch};
    use convoy_core::{Storage, Workspace as CoreWorkspace};
    use std::time::Instant;

    /// A Claude chat that answers its first message from a recording and ends.
    fn answering_agent(dir: &Path) -> (String, Vec<String>) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../linux/crates/convoy-core/tests/fixtures/chat/claude/approval-allow.ndjson",
        );
        let lines: Vec<String> = std::fs::read_to_string(fixture)
            .unwrap()
            .lines()
            .filter_map(|line| {
                let record: serde_json::Value = serde_json::from_str(line).ok()?;
                (record["dir"] == "in").then(|| record["msg"].to_string())
            })
            .collect();
        let file = dir.join("claude.ndjson");
        std::fs::write(&file, lines.join("\n") + "\n").unwrap();
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!(
                    "read -r first; read -r turn; cat '{}'; exit 0",
                    file.display()
                ),
            ],
        )
    }

    struct World {
        _root: tempfile::TempDir,
        app: tauri::App<tauri::test::MockRuntime>,
        storage: Storage,
        manager: String,
        token: String,
    }

    fn world() -> World {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let manager = {
            let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
            core.save_settings(SettingsPatch {
                // Nothing outside the fixture is touched.
                auto_trust: Some(false),
                status_hooks: Some(false),
                ..Default::default()
            })
            .unwrap();
            core.add_project(&project).unwrap();
            let project_id = core.state().projects[0].id.clone();
            core.add_session(NewSession::new(project_id, Agent::Claude, "Lead"))
                .unwrap();
            let id = core.state().sessions[0].id.clone();
            core.set_session_manager(&id, true).unwrap();
            id
        };
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        app.manage(Arc::new(Terminals::new()));
        app.manage(Arc::new(Chats::new()));
        app.manage(Helpers::with_bin("/opt/convoy".into()));
        app.manage(FakeAgent::default());
        *app.state::<FakeAgent>().0.lock().unwrap() = answering_agent(root.path());
        let token = app
            .state::<Helpers>()
            .launch_for(&app.state::<Workspace>(), &manager)
            .unwrap()
            .token;
        World {
            _root: root,
            app,
            storage,
            manager,
            token,
        }
    }

    /// Writes a request as the command line does and answers it as the
    /// poller does.
    fn ask(world: &World, token: &str, command: Command) -> Response {
        let dir = helpers::requests_dir(&world.storage);
        let request = Request::new(&world.manager, token, command);
        helpers::write_request(&dir, &request).unwrap();
        poll_once(world.app.handle(), false);
        assert!(helpers::pending(&dir).is_empty(), "the request was handled");
        helpers::take_result(&dir, &request.id).expect("a result")
    }

    fn until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The whole pipeline with a stand-in agent: a spawn creates the helper
    /// under its manager and starts it as a chat with the prompt; status
    /// follows its turn to done; output is its last answer; the activity
    /// says so. A wrong secret gets nothing.
    #[test]
    fn a_manager_spawns_waits_for_and_reads_a_helper() {
        let world = world();
        let refused = ask(&world, "wrong", Command::List);
        assert!(!refused.ok);
        assert!(refused.error.unwrap().contains("cannot start helpers"));

        let spawned = ask(
            &world,
            &world.token,
            Command::Spawn(Spawn {
                agent: "claude".into(),
                title: Some("Write the file".into()),
                prompt: "Create a file named claude.txt".into(),
                ..Default::default()
            }),
        );
        assert!(spawned.ok, "{:?}", spawned.error);
        let child = spawned.value["id"].as_str().unwrap().to_string();

        let workspace = world.app.state::<Workspace>();
        let (parent, view, started) = workspace
            .with(|core| {
                let session = core.session(&child).unwrap();
                Ok((
                    session.parent_id.clone(),
                    session.view.clone(),
                    session.started,
                ))
            })
            .unwrap();
        assert_eq!(parent.as_deref(), Some(world.manager.as_str()));
        assert_eq!(view.as_deref(), Some("chat"));
        assert!(started);

        until("the helper's turn to end", || {
            let status = ask(
                &world,
                &world.token,
                Command::Status {
                    ids: vec![child.clone()],
                },
            );
            status.value[0]["status"] == "done"
        });
        let list = ask(&world, &world.token, Command::List);
        let views: Vec<HelperView> = serde_json::from_value(list.value).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(
            (views[0].title.as_str(), views[0].agent.as_str()),
            ("Write the file", "claude")
        );
        assert_eq!(views[0].status, HelperStatus::Done);

        let output = ask(&world, &world.token, Command::Output { id: child.clone() });
        assert!(output.ok, "{:?}", output.error);
        assert!(!output.value["text"].as_str().unwrap().is_empty());

        // Only its own helpers.
        let other = ask(
            &world,
            &world.token,
            Command::Output {
                id: world.manager.clone(),
            },
        );
        assert!(!other.ok);

        let chats = world.app.state::<Arc<Chats>>();
        until("the helper to exit", || !chats.running(&child));
        let details: Vec<String> = workspace
            .with(|core| {
                Ok(core
                    .state()
                    .activity
                    .iter()
                    .map(|event| event.detail.clone())
                    .collect())
            })
            .unwrap();
        assert!(
            details
                .iter()
                .any(|detail| detail == "Started helper: Write the file"),
            "{details:?}"
        );
        assert!(
            details
                .iter()
                .any(|detail| detail == "Helper finished: Write the file"),
            "{details:?}"
        );
    }

    /// Four helpers may run at once; a fifth waits its turn. A stopped one
    /// reports as stopped.
    #[test]
    fn a_manager_runs_at_most_four_helpers() {
        let world = world();
        *world.app.state::<FakeAgent>().0.lock().unwrap() = (
            "/bin/sh".into(),
            vec!["-c".into(), "exec cat > /dev/null".into()],
        );
        let spawn = || {
            ask(
                &world,
                &world.token,
                Command::Spawn(Spawn {
                    agent: "codex".into(),
                    prompt: "Work for ever".into(),
                    ..Default::default()
                }),
            )
        };
        let ids: Vec<String> = (0..4)
            .map(|_| {
                let response = spawn();
                assert!(response.ok, "{:?}", response.error);
                response.value["id"].as_str().unwrap().to_string()
            })
            .collect();
        let fifth = spawn();
        assert!(fifth.error.unwrap().contains("already running"));

        let status = ask(&world, &world.token, Command::Status { ids: ids.clone() });
        assert!(status
            .value
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["status"] == "running"));

        let chats = world.app.state::<Arc<Chats>>();
        chats.stop(&ids[0]);
        until("the stop", || !chats.running(&ids[0]));
        let status = ask(
            &world,
            &world.token,
            Command::Status {
                ids: vec![ids[0].clone()],
            },
        );
        assert_eq!(status.value[0]["status"], "stopped");
        assert!(spawn().ok, "room for one more");
        for id in chats.ids() {
            chats.stop(&id);
        }
        until("every stop", || chats.ids().is_empty());
    }

    /// The command line itself, in a thread of its own, against the poller:
    /// files out, files back, the exit codes it promises.
    #[test]
    fn the_command_line_reaches_the_running_app() {
        let world = world();
        let dir = helpers::requests_dir(&world.storage);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // The heartbeat is there before the first command looks for it.
        poll_once(world.app.handle(), true);
        let poller = {
            let app = world.app.handle().clone();
            let done = done.clone();
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    poll_once(&app, false);
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        };
        let env = |key: &str| match key {
            helpers::ENV_SESSION => Some(world.manager.clone()),
            helpers::ENV_TOKEN => Some(world.token.clone()),
            helpers::ENV_REQUESTS => Some(dir.to_string_lossy().into_owned()),
            _ => None,
        };
        let timing = Timing {
            poll: Duration::from_millis(10),
            wait_step: Duration::from_millis(50),
            ..Default::default()
        };
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = cli::run_with(&args, &env, &mut out, &mut err, &timing);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let (code, out, err) = run(&["spawn", "--agent", "claude", "--prompt", "Say hi"]);
        assert_eq!(code, 0, "{err}");
        let child = out.trim().to_string();
        let (code, out, _) = run(&["wait", &child, "--timeout", "20"]);
        assert_eq!(code, 0, "{out}");
        let line: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(line["id"], child.as_str());
        assert_eq!(line["status"], "done");
        assert_eq!(line["title"], "Say hi");
        let (code, out, _) = run(&["output", &child]);
        assert_eq!(code, 0);
        assert!(!out.trim().is_empty());
        let (code, _, err) = run(&["spawn", "--agent", "gemini", "--prompt", "x"]);
        assert_eq!(
            (code, err.trim()),
            (1, "convoy: Unknown agent. Use claude or codex.")
        );
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        poller.join().unwrap();
    }

    /// The real thing, once: a Claude manager in a chat starts a Codex helper
    /// from its own shell through the built `convoy-tauri --session …`, waits
    /// for it, reads its answer and reports it. Spends a little of both
    /// subscriptions, so it only runs when asked:
    ///
    ///     cargo build && cargo test real_manager -- --ignored --nocapture
    ///
    /// Run it with a clean environment (`env -i HOME=… PATH=… SHELL=…`) when
    /// started from inside Claude Code.
    #[test]
    #[ignore = "talks to the real Claude Code and Codex"]
    fn real_manager_spawns_waits_for_and_reads_a_codex_helper() {
        use tauri::Listener;

        let bin = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("convoy-tauri");
        assert!(bin.exists(), "run `cargo build` first: {}", bin.display());

        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let manager = {
            let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
            core.save_settings(SettingsPatch {
                auto_trust: Some(false),
                // The hooks would run this test binary.
                status_hooks: Some(false),
                claude_usage: Some(false),
                yolo_claude: Some(true),
                yolo_codex: Some(true),
                ..Default::default()
            })
            .unwrap();
            core.add_project(&project).unwrap();
            let project_id = core.state().projects[0].id.clone();
            let mut input = NewSession::new(project_id, Agent::Claude, "Lead");
            input.prompt = "Use your helper commands. Spawn exactly one helper with --agent codex and this prompt: \
                 'Create a file named helper.txt in the current folder containing the single word ok. Then reply with exactly: wrote helper.txt'. \
                 Wait for it, read its output, and finish by replying with one line: HELPER SAID: <its output>."
                .into();
            input.model = "sonnet".into();
            core.add_session(input).unwrap();
            let id = core.state().sessions[0].id.clone();
            core.set_session_manager(&id, true).unwrap();
            core.set_session_view(&id, "chat").unwrap();
            id
        };

        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        app.manage(Arc::new(Terminals::new()));
        app.manage(Arc::new(Chats::new()));
        app.manage(Repositories::default());
        app.manage(Helpers::with_bin(bin));
        let (events_in, events) = std::sync::mpsc::channel::<serde_json::Value>();
        app.handle().listen("chat:event", move |event| {
            let _ = events_in.send(serde_json::from_str(event.payload()).unwrap());
        });
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let poller = {
            let app = app.handle().clone();
            let done = done.clone();
            poll_once(&app, true);
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    poll_once(&app, false);
                    std::thread::sleep(Duration::from_millis(300));
                }
            })
        };

        let workspace = app.state::<Workspace>();
        let terminals = app.state::<Arc<Terminals>>();
        let chats = app.state::<Arc<Chats>>();
        crate::chat::start_chat(app.handle(), &manager, &workspace, &terminals, &chats).unwrap();

        let started = Instant::now();
        let mut finished = None;
        let mut last_answer = String::new();
        while started.elapsed() < Duration::from_secs(900) {
            let Ok(payload) = events.recv_timeout(Duration::from_secs(5)) else {
                continue;
            };
            let id = payload["id"].as_str().unwrap_or_default().to_string();
            let who = if id == manager { "manager" } else { "helper " };
            let event = &payload["event"];
            let at = started.elapsed().as_secs();
            match event["type"].as_str().unwrap_or_default() {
                "tool" => println!("[{at:>3}s {who}] tool: {}", event["title"]),
                "tool_result" => println!(
                    "[{at:>3}s {who}] result ok={}: {}",
                    event["ok"],
                    convoy_core::json::head(event["output"].as_str().unwrap_or_default(), 300)
                ),
                "text_done" => {
                    println!("[{at:>3}s {who}] says: {}", event["text"]);
                    if id == manager {
                        last_answer = event["text"].as_str().unwrap_or_default().to_string();
                    }
                }
                "approval" => println!("[{at:>3}s {who}] APPROVAL: {}", event["title"]),
                "error" => println!("[{at:>3}s {who}] error: {}", event["message"]),
                "turn_ended" => {
                    println!("[{at:>3}s {who}] turn ended, error: {}", event["error"]);
                    if id == manager {
                        finished = Some(at);
                        break;
                    }
                }
                _ => {}
            }
        }
        println!("manager finished after {finished:?}s");

        let (helpers_made, activity) = workspace
            .with(|core| {
                Ok((
                    children(core, &manager)
                        .iter()
                        .map(|session| (session.id.clone(), session.agent, session.title.clone()))
                        .collect::<Vec<_>>(),
                    core.state()
                        .activity
                        .iter()
                        .map(|event| format!("{:?} {}: {}", event.kind, event.title, event.detail))
                        .collect::<Vec<_>>(),
                ))
            })
            .unwrap();
        println!("helpers: {helpers_made:?}");
        println!("activity (newest first):\n  {}", activity.join("\n  "));
        let written = std::fs::read_to_string(project.join("helper.txt")).unwrap_or_default();
        println!("helper.txt: {written:?}");

        for id in chats.ids() {
            chats.stop(&id);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !chats.ids().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        poller.join().unwrap();

        assert!(finished.is_some(), "the manager never finished its turn");
        assert_eq!(helpers_made.len(), 1, "{helpers_made:?}");
        assert_eq!(helpers_made[0].1, Agent::Codex);
        assert_eq!(written.trim(), "ok");
        assert!(last_answer.contains("HELPER SAID"), "{last_answer}");
    }
}
