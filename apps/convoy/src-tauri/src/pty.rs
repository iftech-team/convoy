//! The window's end of the terminal.
//!
//! `convoy-pty` runs the agent and says what happened; this decides what that
//! means. Output becomes an event the web view listens on, and an ending is
//! first recorded in the workspace and then announced — in that order, because
//! whether an agent's work is kept must not depend on a window being able to
//! answer.

use convoy_core::session::ExitCause;
use convoy_pty::{Ended, Sink};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub use convoy_pty::{Launch, Terminals};

#[derive(Clone, Serialize)]
pub struct Output {
    pub id: String,
    pub data: String,
}

#[derive(Clone, Serialize)]
pub struct Exit {
    pub id: String,
    pub code: i32,
    /// How it ended: `exited`, `stopped` or `hibernated`. Hibernation is a
    /// stop that is not a failure, and the three are not interchangeable — a
    /// stop pauses the queue and hibernation does not.
    pub cause: &'static str,
    /// Whether the queue may pull the next task.
    pub clean: bool,
}

/// Starts an agent and points its output at this window.
pub fn start<R: Runtime>(
    terminals: &Arc<Terminals>,
    app: &AppHandle<R>,
    launch: Launch<'_>,
) -> Result<u32, String> {
    terminals.start(
        Arc::new(Window {
            app: app.clone(),
            watch: std::sync::Mutex::new(Watch::default()),
        }),
        launch,
    )
}

struct Window<R: Runtime> {
    app: AppHandle<R>,
    watch: std::sync::Mutex<Watch>,
}

/// The end of the output, read for the two things an agent prints that
/// Convoy acts on: Codex's resume id and a pull request link.
#[derive(Default)]
struct Watch {
    tail: String,
    codex_id: bool,
    pull_request: Option<String>,
}

/// How much output is kept to look through. Enough for a line printed across
/// several chunks; the macOS app keeps the same.
const TAIL: usize = 6000;

impl<R: Runtime> Window<R> {
    fn watch(&self, id: &str, data: &str) {
        if id.starts_with("login:") {
            return;
        }
        let Ok(mut watch) = self.watch.lock() else {
            return;
        };
        watch.tail.push_str(&convoy_core::history::plain(data));
        if watch.tail.len() > TAIL * 2 {
            let mut cut = watch.tail.len() - TAIL;
            while !watch.tail.is_char_boundary(cut) {
                cut += 1;
            }
            watch.tail.drain(..cut);
        }
        let Some(workspace) = self.app.try_state::<crate::commands::Workspace>() else {
            return;
        };
        let mut changed = false;
        if !watch.codex_id && watch.tail.contains("codex resume ") {
            if let Some(found) = convoy_core::session::codex_resume_id(&watch.tail) {
                watch.codex_id = true;
                changed |= workspace
                    .act(|core| convoy_core::session::record_provider_id(core, id, &found))
                    .unwrap_or(false);
            }
        }
        if watch.tail.contains("/pull/") || watch.tail.contains("/merge_requests/") {
            if let Some(url) = convoy_core::session::pull_request_url(&watch.tail) {
                if watch.pull_request.as_deref() != Some(url.as_str()) {
                    watch.pull_request = Some(url.clone());
                    changed |= workspace
                        .act(|core| convoy_core::session::record_pull_request(core, id, &url))
                        .unwrap_or(false);
                }
            }
        }
        if changed {
            let _ = self.app.emit("session:changed", id.to_string());
        }
    }
}

impl<R: Runtime> Sink for Window<R> {
    fn data(&self, id: &str, data: &str) {
        self.watch(id, data);
        let _ = self.app.emit(
            "terminal:data",
            Output {
                id: id.to_string(),
                data: data.to_string(),
            },
        );
    }

    fn ended(&self, id: &str, how: Ended) {
        let cause = match how {
            Ended::Exited(code) => ExitCause::Exited(code),
            Ended::Stopped => ExitCause::Stopped,
            Ended::Hibernated => ExitCause::Hibernated,
        };

        // A clean exit sends the task to review and a failure pauses the
        // queue. Recorded before the event goes out, so a window reacting to
        // the event reads a workspace that already agrees with it.
        // A login terminal is not a session; there is nothing to record.
        let session = !id.starts_with("login:");
        if let Some(workspace) = self
            .app
            .try_state::<crate::commands::Workspace>()
            .filter(|_| session)
        {
            if let Err(error) =
                workspace.act(|core| convoy_core::session::finish_session(core, id, cause))
            {
                let _ = self.app.emit("terminal:trouble", error);
            }
        }

        let _ = self.app.emit(
            "terminal:exit",
            Exit {
                id: id.to_string(),
                code: match how {
                    Ended::Exited(code) => code,
                    _ => 0,
                },
                cause: match how {
                    Ended::Exited(_) => "exited",
                    Ended::Stopped => "stopped",
                    Ended::Hibernated => "hibernated",
                },
                clean: convoy_core::session::completed_cleanly(cause),
            },
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use convoy_core::model::{Agent, PublishMode, TaskStatus};
    use convoy_core::planning::TaskInput;
    use convoy_core::{Storage, Workspace as CoreWorkspace};
    use std::time::Duration;
    use tauri::Listener;

    /// The rule the whole queue rests on: an agent that exits cleanly hands
    /// its task to review, and never straight to done.
    ///
    /// This is here rather than in convoy-core because the core has always had
    /// the rule; what had never been true is that anything called it. The
    /// assertion is on the workspace file after the child is reaped, which is
    /// the only way to catch the wiring being absent.
    ///
    /// Unix only: this binary links Tauri, and on Windows such a test binary
    /// cannot be loaded at all. The terminal itself is tested on both
    /// platforms in `convoy-pty`, which depends on no window.
    #[test]
    fn a_clean_exit_sends_the_task_to_review() {
        let root = std::env::temp_dir().join(format!("convoy-exit-{}", std::process::id()));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.join("state"));

        let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        core.add_project(&project).unwrap();
        let project_id = core.state().projects[0].id.clone();
        core.save_task(TaskInput {
            id: None,
            project_id,
            spec_id: None,
            title: "Something to finish".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::None,
            auto_review: false,
        })
        .unwrap();
        let task_id = core.state().tasks[0].id.clone();
        core.prepare_task(&task_id, None).unwrap();
        let session_id = core.state().tasks[0].session_id.clone().unwrap();
        drop(core);

        let app = tauri::test::mock_app();
        app.manage(crate::commands::Workspace::at(storage.clone()));
        let terminals = Arc::new(Terminals::new());
        let (exits, exited) = std::sync::mpsc::channel::<()>();
        app.handle().listen("terminal:exit", move |_| {
            let _ = exits.send(());
        });

        let env = vec![(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )];
        start(
            &terminals,
            app.handle(),
            Launch {
                id: &session_id,
                program: "/bin/sh",
                args: &["-c".to_string(), "exit 0".to_string()],
                cwd: &project,
                env: &env,
                cols: 80,
                rows: 24,
            },
        )
        .expect("spawn");

        exited
            .recv_timeout(Duration::from_secs(10))
            .expect("no exit event");

        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(
            core.state().tasks[0].status,
            TaskStatus::Review,
            "a clean exit must hand the task to review"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A pull request link in the output is recorded on the task, and a task
    /// with one awaits its pull request when the agent exits.
    #[test]
    fn a_printed_pull_request_is_kept_on_the_task() {
        let root = std::env::temp_dir().join(format!("convoy-pr-{}", std::process::id()));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.join("state"));

        let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        core.add_project(&project).unwrap();
        let project_id = core.state().projects[0].id.clone();
        core.save_task(TaskInput {
            id: None,
            project_id,
            spec_id: None,
            title: "Open a pull request".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::Pr,
            auto_review: false,
        })
        .unwrap();
        let task_id = core.state().tasks[0].id.clone();
        core.prepare_task(&task_id, None).unwrap();
        let session_id = core.state().tasks[0].session_id.clone().unwrap();
        drop(core);

        let app = tauri::test::mock_app();
        app.manage(crate::commands::Workspace::at(storage.clone()));
        let terminals = Arc::new(Terminals::new());
        let (exits, exited) = std::sync::mpsc::channel::<()>();
        app.handle().listen("terminal:exit", move |_| {
            let _ = exits.send(());
        });
        let env = vec![(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )];
        start(
            &terminals,
            app.handle(),
            Launch {
                id: &session_id,
                program: "/bin/sh",
                args: &[
                    "-c".to_string(),
                    "printf 'Created https://github.com/acme/app/pull/42\\n'; exit 0".to_string(),
                ],
                cwd: &project,
                env: &env,
                cols: 80,
                rows: 24,
            },
        )
        .expect("spawn");
        exited
            .recv_timeout(Duration::from_secs(10))
            .expect("no exit event");

        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        let task = &core.state().tasks[0];
        assert_eq!(
            task.pr_url.as_deref(),
            Some("https://github.com/acme/app/pull/42")
        );
        assert_eq!(task.status, TaskStatus::Pr);
        std::fs::remove_dir_all(&root).ok();
    }
}
