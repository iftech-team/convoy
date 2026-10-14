//! Task documents: the last stage of a task, and the list of what was
//! written.
//!
//! Once QA passes, a documentation writer — the task's agent, with the
//! project's instructions and skills for the role — writes the document the
//! people who test and integrate the change read. It goes into the project's
//! documents folder, outside the repository (`docsDir`, or Convoy's own data
//! folder), and the agent reports the files it wrote ([`crate::stage`]).
//! Convoy keeps only files that exist and lie inside that folder.
//!
//! A failure here does not undo the task: the code is done. The task needs
//! attention, marked as having no document.

use crate::model::{Project, Role, Task, TaskStatus};
use crate::pipeline::{self, Action, Context};
use crate::stage::{self, Stage};
use crate::workspace::{new_id, NewSession, Workspace};
use crate::{bail, ensure, json::head, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Markdown,
    Pdf,
    Other,
}

impl DocKind {
    pub fn of(path: &Path) -> DocKind {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("md" | "markdown") => DocKind::Markdown,
            Some("pdf") => DocKind::Pdf,
            _ => DocKind::Other,
        }
    }
}

/// A document written for a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDoc {
    pub path: PathBuf,
    pub title: String,
    pub kind: DocKind,
    pub created: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsStatus {
    Writing,
    Written,
    Failed,
}

/// A task's latest documentation run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocsRun {
    pub id: String,
    pub status: DocsStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub started: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
}

/// Where a project's task documents go: its own folder, or one in Convoy's
/// data, never the repository.
pub fn folder(workspace: &Workspace, project: &Project) -> PathBuf {
    match project
        .docs_dir
        .as_ref()
        .filter(|dir| !dir.as_os_str().is_empty())
    {
        Some(dir) => dir.clone(),
        None => stage::storage(workspace)
            .root()
            .join("docs")
            .join(&project.id),
    }
}

/// `path` if it is a file inside `folder`, by its real location: `../` and
/// links out of the folder are refused.
pub fn inside(folder: &Path, path: &Path) -> Result<PathBuf> {
    let (Ok(root), Ok(real)) = (folder.canonicalize(), path.canonicalize()) else {
        bail!("Document not found.")
    };
    ensure!(
        real.starts_with(&root) && real.is_file(),
        "That file is not in the project's documents folder."
    );
    Ok(real)
}

fn find(workspace: &Workspace, task_id: &str) -> Result<Task> {
    match workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == task_id)
    {
        Some(task) => Ok(task.clone()),
        None => bail!("Task not found."),
    }
}

/// Everything the document is written from, so the writer need not read
/// the history of each session.
fn brief(workspace: &Workspace, task: &Task, folder: &Path, file: &Path) -> Result<String> {
    let project = workspace.project(&task.project_id)?;
    let instructions = crate::roles::brief(workspace.settings(), project, Role::Docs);
    let state = workspace.state();
    let mut facts = Vec::new();
    if let Some(source) = &task.source {
        facts.push(format!("Issue: {} {}", source.tracker.label(), source.key));
    }
    let builder = task
        .session_id
        .as_deref()
        .and_then(|id| state.sessions.iter().find(|session| session.id == id));
    if let Some(branch) = builder.and_then(|session| session.branch.as_deref()) {
        facts.push(format!("Branch: {branch}"));
    }
    if let Some(pr) = &task.pr_url {
        facts.push(format!("Pull request: {pr}"));
    }
    let facts = if facts.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", facts.join("\n"))
    };
    let mut verdicts = String::new();
    if let Some(review) = &task.review {
        verdicts.push_str(&format!("\n## Review\nVerdict: {:?}\n", review.status));
        if !review.findings.trim().is_empty() {
            verdicts.push_str(&format!("{}\n", review.findings.trim()));
        }
    }
    if let Some(qa) = &task.qa {
        verdicts.push_str(&format!("\n## QA\nVerdict: {:?}\n", qa.status));
        for check in &qa.checks {
            verdicts.push_str(&format!(
                "- `{}`: {}\n",
                check.command,
                if check.ok { "passed" } else { "failed" }
            ));
        }
        if !qa.findings.trim().is_empty() {
            verdicts.push_str(&format!("{}\n", qa.findings.trim()));
        }
    }
    let prompt = format!(
        "{instructions}\n\nDo not change the code, and do not commit. Put the documents in {} and nowhere else.\n\n## Task\n{}\n\n{}\n{facts}{}{verdicts}\n{}",
        folder.display(),
        task.title,
        task.details,
        pipeline::task_context(task),
        stage::closing(Stage::Docs, file)
    );
    Ok(head(&prompt, 32_000).to_string())
}

/// Starts the documentation writer for a task. Returns what the window has
/// to start.
pub fn begin(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    ensure!(
        !task
            .docs_run
            .as_ref()
            .is_some_and(|run| run.status == DocsStatus::Writing),
        "The document is already being written."
    );
    let project = workspace.project(&task.project_id)?.clone();
    let folder = folder(workspace, &project);
    std::fs::create_dir_all(&folder)?;
    std::fs::create_dir_all(stage::dir(workspace))?;
    let run = new_id();
    let file = stage::file(workspace, Stage::Docs, &run);
    let mut input = NewSession::new(
        task.project_id.clone(),
        task.agent,
        head(&format!("Docs: {}", task.title), 200).to_string(),
    );
    input.prompt = brief(workspace, &task, &folder, &file)?;
    input.model = task.model.clone().unwrap_or_default();
    input.effort = task.effort.clone().unwrap_or_default();
    input.profile_id = task.profile_id.clone();
    input.directory_of = task
        .session_id
        .clone()
        .filter(|id| workspace.session(id).is_ok());
    input.skills = context.skills(&task.project_id, task.agent, Role::Docs);
    let session = workspace
        .add_session(input)?
        .sessions
        .last()
        .map(|session| session.id.clone())
        .unwrap_or_default();
    let record = DocsRun {
        id: run,
        status: DocsStatus::Writing,
        session_id: Some(session.clone()),
        started: crate::time::now_iso8601(),
        finished: None,
    };
    pipeline::change(workspace, task_id, move |task| task.docs_run = Some(record))?;
    Ok(vec![Action::Start {
        task: task_id.to_string(),
        session,
    }])
}

/// The writer reported its files: those inside the documents folder are
/// kept on the task.
pub fn written(workspace: &mut Workspace, task_id: &str, files: &[PathBuf]) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let project = workspace.project(&task.project_id)?.clone();
    let folder = folder(workspace, &project);
    let now = crate::time::now_iso8601();
    let accepted: Vec<TaskDoc> = files
        .iter()
        .filter_map(|path| inside(&folder, path).ok())
        .map(|path| TaskDoc {
            title: path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Document")
                .to_string(),
            kind: DocKind::of(&path),
            path,
            created: now.clone(),
        })
        .collect();
    if accepted.is_empty() {
        return failed(
            workspace,
            task_id,
            &format!(
                "The documentation writer reported no document inside {}.",
                folder.display()
            ),
        );
    }
    let count = accepted.len();
    pipeline::change(workspace, task_id, move |task| {
        for doc in accepted {
            task.docs.retain(|kept| kept.path != doc.path);
            task.docs.push(doc);
        }
        if let Some(run) = task.docs_run.as_mut() {
            run.status = DocsStatus::Written;
            run.finished = Some(crate::time::now_iso8601());
        }
    })?;
    Ok(vec![Action::Notice {
        task: task_id.to_string(),
        event: "docs_written",
        detail: count.to_string(),
    }])
}

/// The stage failed: the task keeps its code, and needs attention.
pub fn failed(workspace: &mut Workspace, task_id: &str, reason: &str) -> Result<Vec<Action>> {
    pipeline::change(workspace, task_id, |task| {
        if let Some(run) = task.docs_run.as_mut() {
            run.status = DocsStatus::Failed;
            run.finished = Some(crate::time::now_iso8601());
        }
    })?;
    pipeline::attention(workspace, task_id, &format!("No document. {reason}"))
}

/// A document on the Documents page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    pub path: PathBuf,
    pub title: String,
    pub kind: DocKind,
    /// When it was written, or last changed for a file found in the folder.
    pub created: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    /// The tracker key, as `LOYAL-523`.
    pub task_key: Option<String>,
    pub task_status: Option<TaskStatus>,
}

/// A project's task documents: those its tasks recorded, then any other
/// Markdown or PDF in its documents folder, put there by hand. Newest first.
pub fn list(workspace: &Workspace, project_id: &str) -> Result<Vec<Listed>> {
    let project = workspace.project(project_id)?;
    let folder = folder(workspace, project);
    let mut listed: Vec<Listed> = Vec::new();
    for task in workspace
        .state()
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
    {
        for doc in task.docs.iter().filter(|doc| doc.path.is_file()) {
            listed.push(Listed {
                path: doc.path.clone(),
                title: doc.title.clone(),
                kind: doc.kind,
                created: doc.created.clone(),
                task_id: Some(task.id.clone()),
                task_title: Some(task.title.clone()),
                task_key: task.source.as_ref().map(|source| source.key.clone()),
                task_status: Some(task.status),
            });
        }
    }
    let known: Vec<PathBuf> = listed
        .iter()
        .filter_map(|doc| doc.path.canonicalize().ok())
        .collect();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = DocKind::of(&path);
            if kind == DocKind::Other || !path.is_file() {
                continue;
            }
            if path.canonicalize().is_ok_and(|real| known.contains(&real)) {
                continue;
            }
            let created = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| crate::time::iso8601(since.as_secs() as i64, since.subsec_millis()))
                .unwrap_or_default();
            listed.push(Listed {
                title: path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_string(),
                kind,
                path,
                created,
                task_id: None,
                task_title: None,
                task_key: None,
                task_status: None,
            });
        }
    }
    listed.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(listed)
}
