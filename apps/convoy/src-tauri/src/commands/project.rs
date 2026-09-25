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
    pub color: Option<String>,
    pub default_agent: Option<String>,
    pub base_ref: Option<String>,
    pub branch_prefix: Option<String>,
    pub task_mode: Option<String>,
    pub auto_run_tasks: Option<bool>,
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
                    color: input.color,
                    default_agent: input.default_agent,
                    base_ref: input.base_ref,
                    branch_prefix: input.branch_prefix,
                    task_mode: input.task_mode,
                    auto_run_tasks: input.auto_run_tasks,
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
    pub color: String,
    pub default_agent: String,
    pub base_ref: String,
    pub branch_prefix: String,
    pub task_mode: String,
    pub auto_run_tasks: bool,
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
            color: project.color.clone().unwrap_or_default(),
            default_agent: project
                .default_agent
                .map(|agent| agent.as_str().to_string())
                .unwrap_or_default(),
            base_ref: project.base_ref.clone().unwrap_or_default(),
            branch_prefix: project.branch_prefix.clone().unwrap_or_default(),
            task_mode: project.task_mode.clone().unwrap_or_default(),
            auto_run_tasks: project.auto_run_tasks == Some(true),
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

/// Moves a project to `index` in the sidebar's order.
#[tauri::command]
pub fn project_move(
    id: String,
    index: usize,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.move_project(&id, index).map(|_| ()))
}

/// "Refresh projects" / "Import projects from this folder": adds the
/// repositories inside a project's folder that are not projects yet, grouped
/// under the project's name as opening a folder of repositories would.
/// Returns how many were added.
#[tauri::command]
pub fn project_refresh(id: String, workspace: State<'_, Workspace>) -> Result<usize, String> {
    let (root, name, known) = workspace.act(|core| {
        let project = core.project(&id)?.clone();
        let known: Vec<PathBuf> = core
            .state()
            .projects
            .iter()
            .map(|item| item.path.clone())
            .collect();
        // A project in a group was found in the group's folder: that folder
        // is the one to look through again.
        let parent = project.path.parent().map(Path::to_path_buf);
        let folder = match (&project.group, &parent) {
            (Some(group), Some(parent))
                if parent
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == *group) =>
            {
                parent.clone()
            }
            _ => project.path.clone(),
        };
        Ok((folder, project.group.unwrap_or(project.title), known))
    })?;
    let found: Vec<PathBuf> = convoy_core::files::discover_within(&root)
        .into_iter()
        .filter(|directory| {
            let resolved = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.clone());
            resolved != root && !known.contains(&resolved)
        })
        .collect();
    let mut added = 0;
    for directory in &found {
        workspace.act(|core| core.add_project(directory).map(|_| ()))?;
        added += 1;
    }
    if added > 0 {
        let fresh: Vec<PathBuf> = found
            .iter()
            .map(|directory| std::fs::canonicalize(directory).unwrap_or_else(|_| directory.clone()))
            .collect();
        workspace.act(|core| {
            core.update(move |state| {
                for project in state.projects.iter_mut() {
                    if fresh.contains(&project.path) {
                        project.group = Some(name.clone());
                    }
                }
                Ok(())
            })
            .map(|_| ())
        })?;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convoy_core::{Storage, Workspace as CoreWorkspace};
    use tauri::Manager;

    /// Refreshing a project in a group adds the repositories that appeared in
    /// the group's folder, under the same group, and nothing twice.
    #[test]
    fn refreshing_a_group_adds_only_what_is_new() {
        let root = std::env::temp_dir().join(format!("convoy-refresh-{}", std::process::id()));
        let group = root.join("clients");
        for name in ["alpha", "beta"] {
            std::fs::create_dir_all(group.join(name).join(".git")).unwrap();
        }
        let storage = Storage::new(root.join("state"));
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));

        assert_eq!(
            project_open(group.to_string_lossy().into_owned(), app.state()).unwrap(),
            2
        );
        let alpha = app
            .state::<Workspace>()
            .act(|core| Ok(core.state().projects[0].id.clone()))
            .unwrap();
        assert_eq!(project_refresh(alpha.clone(), app.state()).unwrap(), 0);

        std::fs::create_dir_all(group.join("gamma").join(".git")).unwrap();
        assert_eq!(project_refresh(alpha, app.state()).unwrap(), 1);
        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        let gamma = core
            .state()
            .projects
            .iter()
            .find(|project| project.title == "gamma")
            .expect("gamma was added");
        assert_eq!(gamma.group.as_deref(), Some("clients"));
        std::fs::remove_dir_all(&root).ok();
    }
}
