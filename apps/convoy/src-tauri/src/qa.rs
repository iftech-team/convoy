//! QA: running a task's checks off the main thread, starting its QA agent,
//! and reading the verdict when that agent ends. The rules are in
//! `convoy_core::qa`; this runs them and tells the window as they progress.

use convoy_core::process::StdRunner;
use convoy_core::qa::{self, Next, Outcome};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::chat::Chats;
use crate::commands::Workspace;
use crate::pty::Terminals;

/// What the window does with a builder's findings: send them now, because
/// the builder is running. A stopped builder already has them waiting for
/// its next resume.
#[derive(Serialize)]
pub struct Delivery {
    builder: String,
    feedback: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum QaStart {
    /// The QA agent's session, created and waiting to be started.
    Agent {
        session: String,
    },
    Passed,
    Failed {
        delivery: Option<Delivery>,
    },
}

#[derive(Serialize)]
pub struct QaExit {
    task: String,
    /// `None` when the agent left no verdict.
    passed: Option<bool>,
    findings: String,
    delivery: Option<Delivery>,
}

/// Findings for the builder: handed to the window when it is running,
/// otherwise kept for the session's next resume.
fn deliver(
    outcome: &Outcome,
    workspace: &Workspace,
    terminals: &Terminals,
    chats: &Chats,
) -> Result<Option<Delivery>, String> {
    let (Some(builder), Some(feedback)) = (outcome.builder.clone(), outcome.feedback()) else {
        return Ok(None);
    };
    if terminals.running(&builder) || chats.running(&builder) {
        return Ok(Some(Delivery { builder, feedback }));
    }
    workspace.act(|core| qa::queue_feedback(core, &builder, &feedback))?;
    Ok(None)
}

/// Tests a task: it moves to testing, its checks run one by one (the window
/// hears of each), then the QA agent is set up, or the run ends here.
#[tauri::command]
pub async fn qa_start<R: Runtime>(
    task_id: String,
    app: AppHandle<R>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<QaStart, String> {
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
    let next = workspace.act(|core| qa::record_checks(core, &task_id, &plan.run_id, checks))?;
    let _ = app.emit("task:qa", &task_id);
    match next {
        Next::Passed => Ok(QaStart::Passed),
        Next::Failed(outcome) => Ok(QaStart::Failed {
            delivery: deliver(&outcome, &workspace, &terminals, &chats)?,
        }),
        Next::Agent(agent) => {
            let folder = workspace.storage.qa();
            std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
            let verdict = qa::verdict_file(&folder, &plan.run_id);
            let session = workspace.act(|core| {
                let brief = qa::brief(core, &task_id, &verdict)?;
                let handoff = qa::handoff(core, &task_id, agent, brief)?;
                let session = core
                    .add_session(handoff)?
                    .sessions
                    .last()
                    .map(|session| session.id.clone())
                    .unwrap_or_default();
                qa::record_session(core, &task_id, &plan.run_id, &session)?;
                Ok(session)
            })?;
            Ok(QaStart::Agent { session })
        }
    }
}

/// When a QA session ends, cleanly or not: its verdict, applied. `None`
/// for any other session.
#[tauri::command]
pub fn qa_after_exit(
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Option<QaExit>, String> {
    let folder = workspace.storage.qa();
    let Some(outcome) = workspace.act(|core| qa::on_exit(core, &folder, &id))? else {
        return Ok(None);
    };
    let delivery = deliver(&outcome, &workspace, &terminals, &chats)?;
    Ok(Some(QaExit {
        task: outcome.task_id,
        passed: outcome.passed,
        findings: outcome.findings,
        delivery,
    }))
}

/// The test commands a project probably uses, read from its manifests.
#[tauri::command]
pub fn qa_suggest_commands(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    workspace.act(|core| Ok(qa::suggest_commands(&core.project(&project_id)?.path)))
}
