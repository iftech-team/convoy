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
    terminals.start(Arc::new(Window { app: app.clone() }), launch)
}

struct Window<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Sink for Window<R> {
    fn data(&self, id: &str, data: &str) {
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
        if let Some(workspace) = self.app.try_state::<crate::commands::Workspace>() {
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
}
