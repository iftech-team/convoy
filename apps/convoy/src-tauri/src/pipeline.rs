//! The task pipeline, as the window drives it: the rules are in
//! `convoy_core::pipeline`; this supplies what they cannot know — the skills
//! library and which sessions have a process — and hands the window the
//! actions only it can take, such as starting an agent.

use convoy_core::pipeline::{self, Action, Context};
use convoy_core::task_docs;
use convoy_core::Workspace as CoreWorkspace;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::chat::Chats;
use crate::commands::Workspace;
use crate::pty::Terminals;

/// Runs `action` with the pipeline's context, holding the workspace and
/// then the skills file, in the order every other command takes them.
pub fn run_with<T>(
    workspace: &Workspace,
    running: &dyn Fn(&str) -> bool,
    action: impl FnOnce(&mut CoreWorkspace, &Context<'_>) -> convoy_core::Result<T>,
) -> Result<T, String> {
    let storage = workspace.storage.clone();
    workspace.with(|core| {
        crate::skills::with_skills(&storage, |store| {
            let context = Context {
                skills: Some(store),
                running,
            };
            action(core, &context)
        })
    })
}

/// [`run_with`] for a command, which knows the terminals and chats.
pub fn run<T>(
    workspace: &Workspace,
    terminals: &Terminals,
    chats: &Chats,
    action: impl FnOnce(&mut CoreWorkspace, &Context<'_>) -> convoy_core::Result<T>,
) -> Result<T, String> {
    let running = |id: &str| crate::chat::is_running(terminals, chats, id);
    run_with(workspace, &running, action)
}

/// The two-second look for stages that reported by file.
#[tauri::command]
pub fn pipeline_tick(
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, pipeline::poll)
}

/// A session ended: a reviewer, QA agent, planner or documentation writer
/// without its file needs attention. Anything already reported is acted on.
#[tauri::command]
pub fn pipeline_after_exit(
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        pipeline::on_exit(core, context, &id)
    })
}

/// Starts publishing a task by hand: after a failure, or where it is not
/// automatic.
#[tauri::command]
pub fn task_publish_start(
    task_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        convoy_core::publish::begin(core, context, &task_id)
    })
}

/// Pushes the task's branch and opens its request, off the main thread,
/// then records how it went.
#[tauri::command]
pub async fn task_publish(
    task_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    let plan = workspace.act(|core| convoy_core::publish::plan(core, &task_id))?;
    let running = plan.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        convoy_core::publish::run(&running, Arc::new(convoy_core::StdRunner))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string());
    run(&workspace, &terminals, &chats, |core, context| {
        convoy_core::publish::finish(core, context, &task_id, &plan.run_id, outcome)
    })
}

/// Reviews a finished task now.
#[tauri::command]
pub fn review_now(
    task_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        pipeline::review_now(core, context, &task_id)
    })
}

#[tauri::command]
pub fn review_skip(
    task_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        pipeline::skip_review(core, context, &task_id)
    })
}

/// Whether a task is planned first, and whether its plans approve
/// themselves (`None` follows the project).
#[tauri::command]
pub fn task_plan(
    id: String,
    plan_first: bool,
    auto_approve: Option<bool>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|core| {
        core.set_task_plan(&id, plan_first, auto_approve)
            .map(|_| ())
    })
}

/// Approves the plan; the window runs the task next.
#[tauri::command]
pub fn plan_approve(task_id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|core| convoy_core::plan::approve(core, &task_id))
}

#[tauri::command]
pub fn plan_request_changes(
    task_id: String,
    comment: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        convoy_core::plan::request_changes(core, context, &task_id, &comment)
    })
}

/// Writes the task's document now.
#[tauri::command]
pub fn task_docs_start(
    task_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<Action>, String> {
    run(&workspace, &terminals, &chats, |core, context| {
        task_docs::begin(core, context, &task_id)
    })
}

/// A project's task documents, for the Documents page.
#[tauri::command]
pub fn task_docs_list(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<task_docs::Listed>, String> {
    workspace.act(|core| task_docs::list(core, &project_id))
}

/// A path the page names, checked to be inside the project's documents
/// folder.
fn document(workspace: &Workspace, project_id: &str, path: &str) -> Result<PathBuf, String> {
    workspace.act(|core| {
        let folder = task_docs::folder(core, core.project(project_id)?);
        task_docs::inside(&folder, std::path::Path::new(path))
    })
}

/// What a task document holds: Markdown as text, a PDF as base64 for the
/// page's viewer.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DocContent {
    Markdown { text: String },
    Pdf { base64: String },
}

#[tauri::command]
pub fn task_doc_read(
    project_id: String,
    path: String,
    workspace: State<'_, Workspace>,
) -> Result<DocContent, String> {
    let path = document(&workspace, &project_id, &path)?;
    let size = std::fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len();
    if size > 32 * 1024 * 1024 {
        return Err("The document is too large to show here. Open it in the system viewer.".into());
    }
    match task_docs::DocKind::of(&path) {
        task_docs::DocKind::Markdown => Ok(DocContent::Markdown {
            text: std::fs::read_to_string(&path).map_err(|error| error.to_string())?,
        }),
        task_docs::DocKind::Pdf => {
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            Ok(DocContent::Pdf {
                base64: convoy_core::integrations::base64(&bytes),
            })
        }
        task_docs::DocKind::Other => Err("Only Markdown and PDF documents are shown here.".into()),
    }
}

/// Opens a task document in the system's viewer.
#[tauri::command]
pub fn task_doc_open(
    project_id: String,
    path: String,
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = document(&workspace, &project_id, &path)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

/// Shows a task document in Finder or the file manager.
#[tauri::command]
pub fn task_doc_reveal(
    project_id: String,
    path: String,
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = document(&workspace, &project_id, &path)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}
