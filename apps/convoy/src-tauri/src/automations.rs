//! Scheduled automations, as the running app runs them.
//!
//! The rules are `convoy_core::automations`; this runs them. A thread looks
//! every half minute whether or not the window is open — on macOS closing the
//! window leaves the app running — and starts what is due as a new session
//! of its project, the way a manager starts a helper: created, given its own
//! worktree when asked, and started with the automation's prompt as its first
//! message. The run's end is followed through the session's chat events, or
//! for a terminal through its agent's hooks and its exit.

use convoy_core::automations::{
    plan_tick, session_title, Automation, AutomationInput, Instant, LocalZone, Run, RunStatus,
    Schedule, Store, Zone,
};
use convoy_core::chat::{ChatEvent, Usage};
use convoy_core::helpers::{Exit, HelperStatus, Settled, Tracker};
use convoy_core::model::ActivityKind;
use convoy_core::workspace::NewSession;
use convoy_core::Storage;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::chat::{is_running, Chats};
use crate::commands::Workspace;
use crate::pty::{Ended, Terminals};

/// The size a terminal run starts at before a window shows it.
const COLUMNS: u16 = 120;
const ROWS: u16 = 32;

/// One writer of `automations.json` at a time: the scheduler, the window's
/// commands and the end of a run all load, change and write it whole.
static FILE: Mutex<()> = Mutex::new(());

/// What is known of the runs this app started.
#[derive(Default)]
pub struct Automations {
    /// Each run's session, followed as a manager follows a helper; the
    /// "parent" is the automation.
    tracker: Mutex<Tracker>,
    /// Terminal runs, by session: when they started, in milliseconds, for
    /// reading their agent's hook reports.
    terminal: Mutex<HashMap<String, f64>>,
    /// Automations being started right now, so one never starts twice.
    starting: Mutex<HashSet<String>>,
}

impl Automations {
    fn status(&self, session: &str, running: bool) -> HelperStatus {
        self.tracker
            .lock()
            .map(|tracker| tracker.status(session, running))
            .unwrap_or(HelperStatus::Running)
    }

    fn followed(&self, session: &str) -> bool {
        self.tracker
            .lock()
            .is_ok_and(|tracker| tracker.get(session).is_some())
    }

    /// Whether the automation's last run is still going.
    fn busy(&self, automation: &Automation, terminals: &Terminals, chats: &Chats) -> bool {
        let starting = self
            .starting
            .lock()
            .is_ok_and(|starting| starting.contains(&automation.id));
        let going = automation
            .active_run()
            .and_then(|run| run.session_id.as_deref())
            .is_some_and(|session| {
                is_running(terminals, chats, session)
                    && self.status(session, true) == HelperStatus::Running
            });
        starting || going
    }
}

pub fn now() -> Instant {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as Instant)
        .unwrap_or(0)
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Loads the file, applies a change and lets the store write it.
pub fn with_store<T>(
    storage: &Storage,
    change: impl FnOnce(&mut Store) -> convoy_core::Result<T>,
) -> Result<T, String> {
    let _guard = FILE.lock().map_err(|_| "Automations unavailable.")?;
    let mut store = Store::load(storage.automations()).map_err(|error| error.to_string())?;
    change(&mut store).map_err(|error| error.to_string())
}

/// A project was removed: its automations go with it.
pub fn forget_project(storage: &Storage, project_id: &str) {
    let _ = with_store(storage, |store| store.remove_project(project_id));
}

/// Every project was removed.
pub fn forget_all(storage: &Storage) {
    let _ = with_store(storage, |store| store.remove_all());
}

// ---------------------------------------------------------------------------
// Following runs.
// ---------------------------------------------------------------------------

/// Every chat event passes here; a run's finished turn finishes the run.
pub fn chat_event<R: Runtime>(app: &AppHandle<R>, id: &str, event: &ChatEvent) {
    let Some(automations) = app.try_state::<Automations>() else {
        return;
    };
    let settled = automations
        .tracker
        .lock()
        .ok()
        .and_then(|mut tracker| tracker.observe(id, event));
    if let Some(settled) = settled {
        let error = match event {
            ChatEvent::TurnEnded { error, .. } => error.clone(),
            _ => None,
        };
        if settle(app, &settled, error) {
            // Its work is done: the agent is not kept waiting for a message
            // nobody may send. Sending one resumes it.
            if let Some(chats) = app.try_state::<Arc<Chats>>() {
                let chats = Arc::clone(&chats);
                let id = id.to_string();
                std::thread::spawn(move || chats.stop(&id));
            }
        }
    }
}

/// Every agent process ending passes here, terminal or chat.
pub fn process_ended<R: Runtime>(app: &AppHandle<R>, id: &str, how: Ended) {
    let Some(automations) = app.try_state::<Automations>() else {
        return;
    };
    if let Ok(mut terminal) = automations.terminal.lock() {
        terminal.remove(id);
    }
    let exit = match how {
        Ended::Exited(code) => Exit::Code(code),
        Ended::Stopped | Ended::Hibernated => Exit::Stopped,
    };
    let (settled, error) = match automations.tracker.lock() {
        Ok(mut tracker) => {
            let settled = tracker.ended(id, exit);
            let error = tracker.get(id).and_then(|tracked| tracked.error.clone());
            (settled, error)
        }
        Err(_) => return,
    };
    if let Some(settled) = settled {
        settle(app, &settled, error);
    }
}

/// Records how a run ended. Returns whether it was an open run.
fn settle<R: Runtime>(app: &AppHandle<R>, settled: &Settled, error: Option<String>) -> bool {
    let (status, note) = match settled.status {
        HelperStatus::Done => (RunStatus::Done, String::new()),
        HelperStatus::Stopped => (RunStatus::Failed, "Stopped before it finished.".to_string()),
        _ => (
            RunStatus::Failed,
            error.unwrap_or_else(|| "The agent failed.".to_string()),
        ),
    };
    finish_run(app, &settled.child, status, &note)
}

fn finish_run<R: Runtime>(
    app: &AppHandle<R>,
    session: &str,
    status: RunStatus,
    note: &str,
) -> bool {
    let Some(workspace) = app.try_state::<Workspace>() else {
        return false;
    };
    let finished = with_store(&workspace.storage, |store| {
        store.finish(session, status, note)
    });
    let Ok(Some(automation)) = finished else {
        return false;
    };
    let (kind, word) = if status == RunStatus::Done {
        (ActivityKind::Done, "Automation finished")
    } else {
        (ActivityKind::Exited, "Automation failed")
    };
    let _ = workspace.act(|core| {
        core.record(kind, session, &format!("{word}: {}", automation.title))
            .map(|_| ())
    });
    let _ = app.emit(
        "automation:finished",
        json!({
            "id": automation.id,
            "title": automation.title,
            "project_id": automation.project_id,
            "session_id": session,
            "status": status,
            "note": note,
        }),
    );
    let _ = app.emit("automation:changed", automation.id.clone());
    let _ = app.emit("session:changed", session.to_string());
    true
}

/// A terminal run's turn ends when its agent's hook says so. Codex reports
/// nothing; its terminal run ends with its process.
fn poll_terminals<R: Runtime>(
    app: &AppHandle<R>,
    automations: &Automations,
    root: &std::path::Path,
) {
    let runs: Vec<(String, f64)> = automations
        .terminal
        .lock()
        .map(|runs| runs.iter().map(|(id, at)| (id.clone(), *at)).collect())
        .unwrap_or_default();
    for (id, since) in runs {
        let Some(status) = convoy_core::monitor::read_status(root, &id, since, now_ms()) else {
            continue;
        };
        if status.state != convoy_core::monitor::AgentState::Done {
            continue;
        }
        if let Ok(mut terminal) = automations.terminal.lock() {
            terminal.remove(&id);
        }
        let ended = ChatEvent::TurnEnded {
            usage: Usage {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
            },
            error: None,
        };
        let settled = automations
            .tracker
            .lock()
            .ok()
            .and_then(|mut tracker| tracker.observe(&id, &ended));
        if let Some(settled) = settled {
            settle(app, &settled, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Starting runs.
// ---------------------------------------------------------------------------

/// Starts one run: a new session of the automation's project, in a fresh
/// worktree when asked, started with the prompt as its first message.
/// `slot` is the scheduled time it is for; none for Run now. Returns the
/// session's id.
pub fn start_run<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    slot: Option<Instant>,
    now: Instant,
    zone: &dyn Zone,
) -> Result<String, String> {
    let (Some(workspace), Some(automations), Some(terminals), Some(chats)) = (
        app.try_state::<Workspace>(),
        app.try_state::<Automations>(),
        app.try_state::<Arc<Terminals>>(),
        app.try_state::<Arc<Chats>>(),
    ) else {
        return Err("Convoy is starting. Try again in a moment.".into());
    };
    let automation = with_store(&workspace.storage, |store| store.get(id).cloned())?;
    if automations.busy(&automation, &terminals, &chats) {
        return Err("Its last run is still going.".into());
    }
    {
        let mut starting = automations
            .starting
            .lock()
            .map_err(|_| "Automations unavailable.")?;
        if !starting.insert(id.to_string()) {
            return Err("Its last run is still going.".into());
        }
    }
    let result = launch(
        app,
        &workspace,
        &automations,
        &terminals,
        &chats,
        &automation,
        slot,
        now,
        zone,
    );
    if let Ok(mut starting) = automations.starting.lock() {
        starting.remove(id);
    }
    let _ = app.emit("automation:changed", id.to_string());
    result
}

#[allow(clippy::too_many_arguments)]
fn launch<R: Runtime>(
    app: &AppHandle<R>,
    workspace: &Workspace,
    automations: &Automations,
    terminals: &Arc<Terminals>,
    chats: &Arc<Chats>,
    automation: &Automation,
    slot: Option<Instant>,
    now: Instant,
    zone: &dyn Zone,
) -> Result<String, String> {
    let storage = workspace.storage.clone();
    let fail = |note: String, session: Option<&str>| -> String {
        let _ = with_store(&storage, |store| {
            store.record_outcome(&automation.id, RunStatus::Failed, &note, session, slot, now)
        });
        note
    };

    let title = session_title(&automation.title, zone, now);
    let created = workspace.act(|core| {
        let before: HashSet<String> = core
            .state()
            .sessions
            .iter()
            .map(|session| session.id.clone())
            .collect();
        let mut input = NewSession::new(automation.project_id.clone(), automation.agent, &title);
        input.prompt = automation.prompt.clone();
        input.model = automation.model.clone();
        input.effort = automation.effort.clone();
        input.profile_id = automation.profile_id.clone();
        input.skills = automation.skills.clone().unwrap_or_default();
        core.add_session(input)?;
        let id = core
            .state()
            .sessions
            .iter()
            .rev()
            .find(|session| !before.contains(&session.id))
            .map(|session| session.id.clone())
            .ok_or_else(|| convoy_core::ConvoyError::message("Session not found."))?;
        core.set_session_view(&id, &automation.view)?;
        Ok(id)
    });
    let session = match created {
        Ok(session) => session,
        Err(error) => return Err(fail(format!("Could not create its session: {error}"), None)),
    };

    if automation.worktree {
        let branch = convoy_core::automations::branch_for(&automation.title, zone, now);
        // Two runs in one minute: the second branch gets a suffix.
        let made = crate::helpers::make_worktree(app, workspace, &session, &branch).or_else(|_| {
            let suffix = &uuid_suffix();
            crate::helpers::make_worktree(app, workspace, &session, &format!("{branch}-{suffix}"))
        });
        if let Err(error) = made {
            let _ = workspace.act(|core| {
                core.remove_sessions(std::slice::from_ref(&session))
                    .map(|_| ())
            });
            return Err(fail(
                format!("Could not create its worktree: {error}"),
                None,
            ));
        }
    }

    // Followed and recorded before it starts, so its first events — and a
    // quick finish — find the run.
    if let Ok(mut tracker) = automations.tracker.lock() {
        tracker.track(&session, &automation.id);
    }
    if automation.view == "terminal" {
        if let Ok(mut terminal) = automations.terminal.lock() {
            terminal.insert(session.clone(), now_ms());
        }
    }
    with_store(&storage, |store| {
        store.record_start(&automation.id, &session, slot, now)
    })?;
    let _ = workspace.act(|core| {
        core.record(
            ActivityKind::Started,
            &session,
            &format!("Automation run: {}", automation.title),
        )
        .map(|_| ())
    });

    let started = if automation.view == "terminal" {
        let helpers = app.try_state::<crate::helpers::Helpers>();
        crate::commands::session::start_terminal(
            app,
            &session,
            COLUMNS,
            ROWS,
            workspace,
            terminals,
            chats,
            helpers.as_deref(),
        )
    } else {
        crate::chat::start_chat(app, &session, workspace, terminals, chats)
    };
    let _ = app.emit("session:spawned", json!({ "id": session, "parent": null }));
    if let Err(error) = started {
        if let Ok(mut tracker) = automations.tracker.lock() {
            tracker.ended(&session, Exit::Code(1));
        }
        if let Ok(mut terminal) = automations.terminal.lock() {
            terminal.remove(&session);
        }
        let note = format!("Could not start: {error}");
        finish_run(app, &session, RunStatus::Failed, &note);
        return Err(note);
    }
    Ok(session)
}

fn uuid_suffix() -> String {
    convoy_core::workspace::new_id()[..4].to_string()
}

// ---------------------------------------------------------------------------
// The scheduler.
// ---------------------------------------------------------------------------

/// Starts the thread that runs what is due. It runs until the app exits.
pub fn start_scheduler<R: Runtime>(app: AppHandle<R>) {
    std::thread::Builder::new()
        .name("convoy-automations".into())
        .spawn(move || {
            // A moment for the app to settle before a missed run catches up.
            std::thread::sleep(Duration::from_secs(3));
            loop {
                tick(&app, now(), &LocalZone);
                std::thread::sleep(Duration::from_secs(convoy_core::automations::TICK_SECONDS));
            }
        })
        .ok();
}

/// One pass: settle what ended, then start, skip or let go what is due.
pub fn tick<R: Runtime>(app: &AppHandle<R>, now: Instant, zone: &dyn Zone) {
    let (Some(workspace), Some(automations), Some(terminals), Some(chats)) = (
        app.try_state::<Workspace>(),
        app.try_state::<Automations>(),
        app.try_state::<Arc<Terminals>>(),
        app.try_state::<Arc<Chats>>(),
    ) else {
        return;
    };
    poll_terminals(app, &automations, &workspace.storage.telemetry());

    let storage = workspace.storage.clone();
    let alive = |session: &str| automations.followed(session);
    let listed = workspace.with(|core| {
        with_store(&storage, |store| {
            let mut changed = store.prune(core)? > 0;
            changed |= store.settle_interrupted(&alive)? > 0;
            Ok((store.all().to_vec(), changed))
        })
    });
    let Ok((list, mut changed)) = listed else {
        return;
    };
    let busy = |automation: &Automation| automations.busy(automation, &terminals, &chats);
    let plan = plan_tick(&list, now, zone, &busy);
    for (id, slot) in plan.passed() {
        changed |= with_store(&storage, |store| store.pass(id, *slot)).is_ok();
    }
    for (id, slot) in &plan.skip {
        let session = list
            .iter()
            .find(|automation| &automation.id == id)
            .and_then(|automation| automation.active_run())
            .and_then(|run| run.session_id.clone());
        changed |= with_store(&storage, |store| {
            store.record_outcome(
                id,
                RunStatus::Skipped,
                "Still running",
                session.as_deref(),
                Some(*slot),
                now,
            )
        })
        .is_ok();
    }
    for (id, slot, _missed) in &plan.start {
        let _ = start_run(app, id, Some(*slot), now, zone);
    }
    if changed {
        let _ = app.emit("automation:changed", "");
    }
}

// ---------------------------------------------------------------------------
// The window's commands.
// ---------------------------------------------------------------------------

/// An automation as the window lists it.
#[derive(Serialize)]
pub struct AutomationView {
    #[serde(flatten)]
    pub automation: Automation,
    /// The next time it runs, when it is on.
    pub next_run: Option<Instant>,
    pub last_run: Option<Run>,
    /// Its last run is still going.
    pub running: bool,
}

/// A project's automations, or every project's.
#[tauri::command]
pub fn automations_list(
    project_id: Option<String>,
    workspace: State<'_, Workspace>,
    automations: State<'_, Automations>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<AutomationView>, String> {
    let storage = workspace.storage.clone();
    let list = workspace.with(|core| {
        with_store(&storage, |store| {
            store.prune(core)?;
            Ok(store.all().to_vec())
        })
    })?;
    let now = now();
    Ok(list
        .into_iter()
        .filter(|automation| {
            project_id
                .as_deref()
                .is_none_or(|project| automation.project_id == project)
        })
        .map(|automation| AutomationView {
            next_run: automation.next_run(now, &LocalZone),
            last_run: automation.last_run().cloned(),
            running: automations.busy(&automation, &terminals, &chats),
            automation,
        })
        .collect())
}

/// Creates or edits one. Returns its id.
#[tauri::command]
pub fn automation_save(
    mut input: AutomationInput,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let storage = workspace.storage.clone();
    // Checked against the library; a new one asked about none gets the
    // automatic ones.
    if input.skills.is_some() || input.id.is_none() {
        input.skills = Some(crate::skills::resolve(
            &storage,
            &input.project_id,
            convoy_core::model::Agent::parse(&input.agent),
            input.skills.as_deref(),
        )?);
    }
    workspace.with(|core| with_store(&storage, |store| store.save(input, core, now())))
}

#[tauri::command]
pub fn automation_delete(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    with_store(&workspace.storage, |store| store.remove(&id))
}

#[tauri::command]
pub fn automation_enable(
    id: String,
    enabled: bool,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    with_store(&workspace.storage, |store| {
        store.set_enabled(&id, enabled, now())
    })
}

/// The next three times a schedule runs, for the editor; why it cannot,
/// otherwise.
#[tauri::command]
pub fn automation_preview(schedule: Schedule) -> Result<Vec<Instant>, String> {
    if let Some(problem) = schedule.problem() {
        return Err(problem.to_string());
    }
    Ok(convoy_core::automations::next_runs(
        &schedule,
        now(),
        3,
        &LocalZone,
    ))
}

/// Runs one now, whatever its schedule. Off the main thread: a worktree
/// takes a moment.
#[tauri::command]
pub async fn automation_run_now(app: AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || start_run(&app, &id, None, now(), &LocalZone))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::chat::FakeAgent;
    use convoy_core::automations::{AutomationInput, Day, RuleZone};
    use convoy_core::model::Agent;
    use convoy_core::workspace::SettingsPatch;
    use convoy_core::Workspace as CoreWorkspace;
    use std::path::Path;
    use std::time::Instant as Clock;

    /// A Claude chat that answers its first message from a recording and
    /// ends, as the helpers' tests use.
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

    /// An agent that never answers.
    fn silent_agent() -> (String, Vec<String>) {
        (
            "/bin/sh".into(),
            vec!["-c".into(), "exec cat > /dev/null".into()],
        )
    }

    struct World {
        root: tempfile::TempDir,
        app: tauri::App<tauri::test::MockRuntime>,
        storage: Storage,
        project: String,
    }

    fn world() -> World {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let project = {
            let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
            core.save_settings(SettingsPatch {
                auto_trust: Some(false),
                status_hooks: Some(false),
                ..Default::default()
            })
            .unwrap();
            core.add_project(&project).unwrap();
            core.state().projects[0].id.clone()
        };
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        app.manage(Arc::new(Terminals::new()));
        app.manage(Arc::new(Chats::new()));
        app.manage(Automations::default());
        app.manage(FakeAgent::default());
        *app.state::<FakeAgent>().0.lock().unwrap() = answering_agent(root.path());
        World {
            root,
            app,
            storage,
            project,
        }
    }

    const HOUR: i64 = 3600;
    /// A fixed zone, so "09:00" is the same instant on every machine.
    fn zone() -> RuleZone {
        RuleZone {
            initial: 2 * HOUR,
            changes: vec![],
        }
    }
    /// 2026-06-10 09:00 at +02:00.
    const NINE: Instant = 1_781_074_800;

    fn create(world: &World, title: &str, created: Instant) -> String {
        let workspace = world.app.state::<Workspace>();
        workspace
            .with(|core| {
                with_store(&world.storage, |store| {
                    store.save(
                        AutomationInput {
                            skills: None,
                            id: None,
                            project_id: world.project.clone(),
                            title: title.into(),
                            prompt: "Reply with: ok".into(),
                            agent: "claude".into(),
                            model: String::new(),
                            effort: String::new(),
                            profile_id: None,
                            view: "chat".into(),
                            worktree: false,
                            schedule: Schedule::Weekdays {
                                at: "09:00".into(),
                                days: Day::ALL.to_vec(),
                            },
                            enabled: true,
                            catch_up: true,
                        },
                        core,
                        created,
                    )
                })
            })
            .unwrap()
    }

    fn get(world: &World, id: &str) -> Automation {
        with_store(&world.storage, |store| store.get(id).cloned()).unwrap()
    }

    fn until(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Clock::now() + Duration::from_secs(15);
        while !ready() {
            assert!(Clock::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_time_is_right() {
        assert_eq!(
            convoy_core::automations::local_stamp(&zone(), NINE),
            "2026-06-10 09:00"
        );
    }

    /// A due automation becomes a chat session of its project, started with
    /// its prompt; the run finishes as done when the turn ends, the activity
    /// says so, and the agent is let go. Nothing runs twice.
    #[test]
    fn a_due_automation_starts_a_session_and_finishes() {
        let world = world();
        let id = create(&world, "Morning check", NINE - HOUR);
        let handle = world.app.handle();
        tick(handle, NINE - 60, &zone());
        assert!(get(&world, &id).history.is_empty(), "not yet");

        tick(handle, NINE + 5, &zone());
        let item = get(&world, &id);
        let run = item.last_run().unwrap().clone();
        assert_eq!(run.status, RunStatus::Started);
        assert_eq!(run.slot, Some(NINE));
        assert!(!run.manual);
        let session = run.session_id.unwrap();
        let workspace = world.app.state::<Workspace>();
        let (title, view, prompt, project, started) = workspace
            .with(|core| {
                let s = core.session(&session).unwrap();
                Ok((
                    s.title.clone(),
                    s.view.clone(),
                    s.prompt.clone(),
                    s.project_id.clone(),
                    s.started,
                ))
            })
            .unwrap();
        assert_eq!(title, "Morning check · 2026-06-10 09:00");
        assert_eq!(view.as_deref(), Some("chat"));
        assert_eq!(prompt, "Reply with: ok");
        assert_eq!(project, world.project);
        assert!(started);

        until("the run to finish", || {
            get(&world, &id).last_run().unwrap().status == RunStatus::Done
        });
        let chats = world.app.state::<Arc<Chats>>();
        until("the agent to go", || !chats.running(&session));
        let details: Vec<String> = workspace
            .with(|core| {
                Ok(core
                    .state()
                    .activity
                    .iter()
                    .map(|e| e.detail.clone())
                    .collect())
            })
            .unwrap();
        assert!(
            details.contains(&"Automation run: Morning check".to_string()),
            "{details:?}"
        );
        assert!(
            details.contains(&"Automation finished: Morning check".to_string()),
            "{details:?}"
        );

        // The same nine o'clock again: nothing.
        tick(handle, NINE + 40, &zone());
        assert_eq!(get(&world, &id).history.len(), 1);
    }

    /// A run still going when its next time comes is skipped, not doubled;
    /// Run now refuses too. Once it ends, Run now works.
    #[test]
    fn a_run_still_going_is_skipped_and_run_now_waits_for_it() {
        let world = world();
        *world.app.state::<FakeAgent>().0.lock().unwrap() = silent_agent();
        let id = create(&world, "Long job", NINE - HOUR);
        let handle = world.app.handle();
        tick(handle, NINE + 5, &zone());
        let first = get(&world, &id)
            .last_run()
            .unwrap()
            .session_id
            .clone()
            .unwrap();
        let chats = world.app.state::<Arc<Chats>>();
        assert!(chats.running(&first));

        tick(handle, NINE + 24 * HOUR + 5, &zone());
        let item = get(&world, &id);
        assert_eq!(item.history.len(), 2);
        assert_eq!(item.history[0].status, RunStatus::Skipped);
        assert_eq!(item.history[0].note, "Still running");
        assert_eq!(item.history[0].session_id.as_deref(), Some(first.as_str()));
        assert_eq!(
            item.active_run().unwrap().session_id.as_deref(),
            Some(first.as_str())
        );

        let refused = start_run(handle, &id, None, NINE + 25 * HOUR, &zone()).unwrap_err();
        assert!(refused.contains("still going"), "{refused}");

        // Stopped: the run failed, and Run now starts a new one.
        chats.stop(&first);
        until("the stop", || !chats.running(&first));
        until("the run to fail", || {
            get(&world, &id).history.iter().any(|run| {
                run.session_id.as_deref() == Some(first.as_str()) && run.status == RunStatus::Failed
            })
        });
        *world.app.state::<FakeAgent>().0.lock().unwrap() = answering_agent(world.root.path());
        let second = start_run(handle, &id, None, NINE + 26 * HOUR, &zone()).unwrap();
        assert_ne!(second, first);
        let item = get(&world, &id);
        assert!(item.last_run().unwrap().manual);
        assert_eq!(
            item.last_slot,
            Some(NINE + 24 * HOUR),
            "Run now is not a slot"
        );
        until("the manual run to finish", || {
            get(&world, &id).last_run().unwrap().status == RunStatus::Done
        });
        until("every stop", || chats.ids().is_empty());
    }

    /// Several due at once: three start in a pass, the rest in the next.
    /// A missed time while the app was closed runs once.
    #[test]
    fn three_start_per_pass_and_a_missed_one_runs_once() {
        let world = world();
        *world.app.state::<FakeAgent>().0.lock().unwrap() = silent_agent();
        let ids: Vec<String> = (0..4)
            .map(|n| create(&world, &format!("Job {n}"), NINE - 3 * 24 * HOUR))
            .collect();
        let handle = world.app.handle();
        // Opened at noon, three days on: one run each, not three.
        let noon = NINE + 3 * HOUR;
        tick(handle, noon, &zone());
        let started = ids
            .iter()
            .filter(|id| !get(&world, id).history.is_empty())
            .count();
        assert_eq!(started, 3);
        tick(handle, noon + 30, &zone());
        for id in &ids {
            let item = get(&world, id);
            assert_eq!(item.history.len(), 1, "once: {:?}", item.history);
            assert_eq!(item.last_slot, Some(NINE));
        }
        let chats = world.app.state::<Arc<Chats>>();
        for id in chats.ids() {
            chats.stop(&id);
        }
        until("every stop", || chats.ids().is_empty());
    }

    /// Removing a project removes its automations.
    #[test]
    fn forgetting_a_project_forgets_its_automations() {
        let world = world();
        create(&world, "One", NINE);
        create(&world, "Two", NINE);
        forget_project(&world.storage, &world.project);
        assert!(with_store(&world.storage, |store| Ok(store.all().is_empty())).unwrap());
        create(&world, "Three", NINE);
        forget_all(&world.storage);
        assert!(with_store(&world.storage, |store| Ok(store.all().is_empty())).unwrap());
    }

    /// A run left open by a Convoy that quit is failed on the next pass,
    /// not left "started" for ever.
    #[test]
    fn a_run_left_open_by_a_quit_is_failed() {
        let world = world();
        let id = create(&world, "Nightly", NINE - HOUR);
        with_store(&world.storage, |store| {
            store.record_start(&id, "gone", Some(NINE - 30 * 60), NINE - 30 * 60)
        })
        .unwrap();
        tick(world.app.handle(), NINE - 60, &zone());
        let item = get(&world, &id);
        assert_eq!(item.history[0].status, RunStatus::Failed);
        assert!(item.history[0].note.contains("closed"));
    }

    /// The real thing, once: an automation run now with the real Claude
    /// Code, as a chat, finishing and recording `done`. Spends a little of
    /// the subscription, so it only runs when asked:
    ///
    ///     cargo test real_automation -- --ignored --nocapture
    ///
    /// Run it with a clean environment (`env -i HOME=… PATH=… SHELL=…`) when
    /// started from inside Claude Code.
    #[test]
    #[ignore = "talks to the real Claude Code"]
    fn real_automation_runs_now_and_finishes() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        std::fs::create_dir_all(&folder).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let project = {
            let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
            core.save_settings(SettingsPatch {
                auto_trust: Some(false),
                status_hooks: Some(false),
                claude_usage: Some(false),
                ..Default::default()
            })
            .unwrap();
            core.add_project(&folder).unwrap();
            core.state().projects[0].id.clone()
        };
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));
        app.manage(Arc::new(Terminals::new()));
        app.manage(Arc::new(Chats::new()));
        app.manage(Automations::default());
        let id = app
            .state::<Workspace>()
            .with(|core| {
                with_store(&storage, |store| {
                    store.save(
                        AutomationInput {
                            skills: None,
                            id: None,
                            project_id: project.clone(),
                            title: "Say ok".into(),
                            prompt: "Reply with: ok".into(),
                            agent: Agent::Claude.as_str().into(),
                            model: "haiku".into(),
                            effort: String::new(),
                            profile_id: None,
                            view: "chat".into(),
                            worktree: false,
                            schedule: Schedule::Daily { at: "03:00".into() },
                            enabled: true,
                            catch_up: false,
                        },
                        core,
                        now(),
                    )
                })
            })
            .unwrap();
        let started = Clock::now();
        let session = start_run(app.handle(), &id, None, now(), &LocalZone).unwrap();
        println!("started session {session}");
        let mut status = RunStatus::Started;
        while started.elapsed() < Duration::from_secs(180) {
            let item = with_store(&storage, |store| store.get(&id).cloned()).unwrap();
            let run = item.last_run().unwrap().clone();
            status = run.status;
            if status != RunStatus::Started {
                println!("run: {run:?}");
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        println!("finished as {status:?} after {:?}", started.elapsed());
        let answer = convoy_core::chat::history::source(
            &CoreWorkspace::load(storage.workspace_file()).unwrap(),
            &storage,
            &session,
        )
        .ok()
        .flatten()
        .and_then(|source| {
            convoy_core::helpers::last_answer(&convoy_core::chat::history::read(&source))
        });
        println!("answer: {answer:?}");
        let chats = app.state::<Arc<Chats>>();
        let deadline = Clock::now() + Duration::from_secs(10);
        while !chats.ids().is_empty() && Clock::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(status, RunStatus::Done);
    }
}
