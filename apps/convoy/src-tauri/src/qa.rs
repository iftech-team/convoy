//! QA: running a task's checks off the main thread, then handing their
//! result to the pipeline, which starts the review or the QA agent, or
//! sends the task back to its builder. The rules are in `convoy_core::qa`
//! and `convoy_core::pipeline`; this runs them and tells the window as they
//! progress.

use convoy_core::pipeline::{self, Action};
use convoy_core::process::StdRunner;
use convoy_core::qa;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::chat::Chats;
use crate::commands::Workspace;
use crate::pty::Terminals;

/// Tests a task: it moves to testing, its checks run one by one (the window
/// hears of each), then the pipeline decides what follows. Returns what the
/// window has to do.
#[tauri::command]
pub async fn qa_start<R: Runtime>(
    task_id: String,
    app: AppHandle<R>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    let plan = workspace.act(|core| qa::begin(core, &task_id))?;
    let _ = app.emit("task:qa", &task_id);
    let checks = {
        let plan = plan.clone();
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            qa::run_checks(&plan, &StdRunner, |check| {
                let _ = app.emit("task:qa-check", (&plan.task_id, check));
            })
        })
        .await
        .map_err(|error| error.to_string())?
    };
    let actions = crate::pipeline::run(&workspace, &terminals, &chats, |core, context| {
        let next = qa::record_checks(core, &task_id, &plan.run_id, checks)?;
        pipeline::after_checks(core, context, next, &task_id)
    })?;
    let _ = app.emit("task:qa", &task_id);
    Ok(actions)
}

/// The test commands a project probably uses, read from its manifests.
#[tauri::command]
pub fn qa_suggest_commands(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    workspace.act(|core| Ok(qa::suggest_commands(&core.project(&project_id)?.path)))
}
