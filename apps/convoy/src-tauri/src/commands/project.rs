//! Projects: adding a folder, editing how it is shown, and letting it go.

use super::Workspace;
use convoy_core::workspace::ProjectPatch;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::State;

/// Adds every project inside a folder, or the folder itself when it contains
/// none. Projects found inside a folder are grouped under its name, so opening
/// a directory of repositories gives a tree rather than a flat list.
#[tauri::command]
pub fn project_open(path: String, workspace: State<'_, Workspace>) -> Result<usize, String> {
    let root = PathBuf::from(&path);
    let found = convoy_core::files::discover(&root);
    let grouped = found.len() > 1;

    let mut added = 0usize;
    let mut failure: Option<String> = None;
    for directory in &found {
        match workspace.act(|workspace| workspace.add_project(directory).map(|_| ())) {
            Ok(()) => added += 1,
            Err(error) => failure = Some(error),
        }
    }
    if added == 0 {
        return Err(failure.unwrap_or_else(|| "Nothing was added.".into()));
    }

    if grouped {
        if let Some(name) = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        {
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            workspace.act(|workspace| {
                workspace
                    .update(move |state| {
                        for project in state.projects.iter_mut() {
                            if project.path != root && project.path.starts_with(&root) {
                                project.group = Some(name.clone());
                            }
                        }
                        Ok(())
                    })
                    .map(|_| ())
            })?;
        }
    }
    Ok(added)
}

#[derive(Deserialize, Default)]
pub struct ProjectInput {
    pub title: Option<String>,
    pub group: Option<String>,
    pub icon: Option<String>,
    pub setup_command: Option<String>,
    pub shared_paths: Option<String>,
    pub review_template: Option<String>,
}

#[tauri::command]
pub fn project_edit(
    id: String,
    input: ProjectInput,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| {
        workspace
            .edit_project(
                &id,
                ProjectPatch {
                    title: input.title,
                    group: input.group,
                    icon: input.icon,
                    setup_command: input.setup_command,
                    shared_paths: input.shared_paths,
                    review_template: input.review_template,
                    ..Default::default()
                },
            )
            .map(|_| ())
    })
}

/// Points a project at a folder that moved. Sessions keep their identities.
#[tauri::command]
pub fn project_reconnect(
    id: String,
    path: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let resolved = std::fs::canonicalize(&path).map_err(|error| error.to_string())?;
    workspace.act(|workspace| {
        workspace
            .edit_project(
                &id,
                ProjectPatch {
                    path: Some(resolved.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .map(|_| ())
    })
}

/// Forgets a project and its saved sessions. Files, worktrees and the
/// provider's own conversations stay where they are.
#[tauri::command]
pub fn project_remove(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| workspace.remove_project(&id).map(|_| ()))
}

/// Everything the project settings form needs.
#[derive(serde::Serialize)]
pub struct ProjectDetail {
    pub id: String,
    pub title: String,
    pub path: String,
    pub group: String,
    pub icon: String,
    pub setup_command: String,
    pub shared_paths: String,
    pub review_template: String,
}

#[tauri::command]
pub fn project_detail(
    id: String,
    workspace: State<'_, Workspace>,
) -> Result<ProjectDetail, String> {
    workspace.act(|workspace| {
        let project = workspace.project(&id)?;
        Ok(ProjectDetail {
            id: project.id.clone(),
            title: project.title.clone(),
            path: project.path.to_string_lossy().into_owned(),
            group: project.group.clone().unwrap_or_default(),
            icon: project.icon.clone().unwrap_or_default(),
            setup_command: project.setup_command.clone().unwrap_or_default(),
            shared_paths: project.shared_paths.clone().unwrap_or_default(),
            review_template: project.review_template.clone().unwrap_or_default(),
        })
    })
}

/// Opens a path in the desktop's own file manager.
#[tauri::command]
pub fn path_reveal(path: String) -> Result<(), String> {
    let path = Path::new(&path);
    if !path.exists() {
        return Err("That folder is no longer there.".into());
    }
    opener_open(path)
}

#[cfg(target_os = "windows")]
fn opener_open(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn opener_open(path: &Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn opener_open(path: &Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
