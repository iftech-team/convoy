//! Projects: adding a folder, editing how it is shown, and letting it go.

use super::Workspace;
use crate::pty::Terminals;
use convoy_core::model::Role;
use convoy_core::workspace::ProjectPatch;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

/// Adds a folder as the macOS app does: every project inside it, nested under
/// the folders between, which become groups. Opening `~/Projects` gives a
/// `Projects` group holding `peshbar` and its repositories; a folder with no
/// project inside is added as one project.
#[tauri::command]
pub fn project_open(path: String, workspace: State<'_, Workspace>) -> Result<usize, String> {
    let found = convoy_core::files::discover_tree(Path::new(&path), false);
    if found.is_empty() {
        return Err("Nothing was added.".into());
    }
    workspace.act(|workspace| workspace.import_tree(&found))?;
    Ok(found.iter().filter(|item| !item.group).count().max(1))
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
    pub test_commands: Option<String>,
    pub qa_agent: Option<String>,
    pub qa_auto: Option<bool>,
    pub qa_timeout: Option<u32>,
    #[serde(default)]
    pub role_instructions: Option<std::collections::BTreeMap<convoy_core::model::Role, String>>,
    #[serde(default)]
    pub max_fix_rounds: Option<u32>,
    #[serde(default)]
    pub review_auto: Option<bool>,
    #[serde(default)]
    pub plan_first_default: Option<bool>,
    #[serde(default)]
    pub plan_auto_approve: Option<bool>,
    #[serde(default)]
    pub docs_dir: Option<String>,
    #[serde(default)]
    pub docs_auto: Option<bool>,
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
                    test_commands: input.test_commands,
                    qa_agent: input.qa_agent,
                    qa_auto: input.qa_auto,
                    qa_timeout: input.qa_timeout,
                    role_instructions: input.role_instructions,
                    max_fix_rounds: input.max_fix_rounds,
                    review_auto: input.review_auto,
                    plan_first_default: input.plan_first_default,
                    plan_auto_approve: input.plan_auto_approve,
                    docs_dir: input.docs_dir,
                    docs_auto: input.docs_auto,
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
    workspace.act(|workspace| workspace.remove_project(&id).map(|_| ()))?;
    // Its automations go with it; they live in a file of their own.
    crate::automations::forget_project(&workspace.storage, &id);
    // So do its own skills.
    crate::skills::forget_project(&workspace, &id);
    Ok(())
}

/// Forgets every project and its saved sessions at once. Refused while an
/// agent runs: its terminal would be left with no project to belong to.
#[tauri::command]
pub fn projects_remove_all(
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<crate::chat::Chats>>,
) -> Result<(), String> {
    let running = workspace.act(|workspace| {
        Ok(workspace
            .state()
            .sessions
            .iter()
            .filter(|session| crate::chat::is_running(&terminals, &chats, &session.id))
            .count())
    })?;
    if running > 0 {
        return Err(format!(
            "Stop the running sessions first ({running} running)."
        ));
    }
    workspace.act(|workspace| workspace.remove_all_projects().map(|_| ()))?;
    crate::automations::forget_all(&workspace.storage);
    crate::skills::forget_all(&workspace);
    Ok(())
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
    pub test_commands: String,
    pub qa_agent: String,
    pub qa_auto: bool,
    pub qa_timeout: u32,
    /// The project's own instructions for each role it sets; the older
    /// review template shows as the reviewer's.
    pub role_instructions: std::collections::BTreeMap<convoy_core::model::Role, String>,
    /// What each role follows when the project sets nothing: the global
    /// instructions, or the built-in ones.
    pub role_fallbacks: std::collections::BTreeMap<convoy_core::model::Role, String>,
    pub max_fix_rounds: u32,
    pub review_auto: bool,
    pub plan_first_default: bool,
    pub plan_auto_approve: bool,
    pub docs_dir: String,
    /// Where documents go when no folder is set.
    pub docs_default_dir: String,
    pub docs_auto: bool,
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
            test_commands: project.test_commands.clone().unwrap_or_default(),
            qa_agent: project
                .qa_agent
                .map(|agent| agent.as_str().to_string())
                .unwrap_or_default(),
            qa_auto: project.qa_auto == Some(true),
            qa_timeout: project
                .qa_timeout
                .unwrap_or(convoy_core::qa::DEFAULT_TIMEOUT_MINUTES),
            role_instructions: Role::ALL
                .into_iter()
                .filter_map(|role| {
                    convoy_core::roles::project_instructions(project, role)
                        .map(|text| (role, text.to_string()))
                })
                .collect(),
            role_fallbacks: Role::ALL
                .into_iter()
                .map(|role| {
                    let text = convoy_core::roles::global_instructions(workspace.settings(), role)
                        .unwrap_or_else(|| convoy_core::roles::builtin(role));
                    (role, text.to_string())
                })
                .collect(),
            max_fix_rounds: convoy_core::pipeline::max_rounds(project),
            review_auto: project.review_auto == Some(true),
            plan_first_default: project.plan_first_default == Some(true),
            plan_auto_approve: project.plan_auto_approve == Some(true),
            docs_dir: project
                .docs_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned())
                .unwrap_or_default(),
            docs_default_dir: convoy_core::task_docs::folder(
                workspace,
                &convoy_core::model::Project {
                    docs_dir: None,
                    ..project.clone()
                },
            )
            .to_string_lossy()
            .into_owned(),
            docs_auto: project.docs_auto == Some(true),
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

/// "Refresh projects" / "Import projects from this folder": looks through the
/// project's folder again, even when it is a project itself, and nests what
/// is new under it, as the macOS app does. Returns how many were added.
#[tauri::command]
pub fn project_refresh(id: String, workspace: State<'_, Workspace>) -> Result<usize, String> {
    let root = workspace.act(|core| Ok(core.project(&id)?.path.clone()))?;
    let found = convoy_core::files::discover_tree(&root, true);
    workspace.act(|core| core.import_tree(&found))
}

#[cfg(test)]
mod tests {
    use super::*;
    use convoy_core::{Storage, Workspace as CoreWorkspace};
    use tauri::Manager;

    /// Opening a folder of repositories nests them under it, as on macOS;
    /// refreshing that group adds only the repositories that appeared since,
    /// under the same group.
    #[test]
    fn opening_and_refreshing_a_folder_builds_the_tree() {
        let root = std::env::temp_dir().join(format!("convoy-refresh-{}", std::process::id()));
        let group = root.join("clients");
        for name in ["alpha", "beta"] {
            std::fs::create_dir_all(group.join(name).join(".git")).unwrap();
        }
        std::fs::create_dir_all(group.join("nested").join("gamma").join(".git")).unwrap();
        std::fs::create_dir_all(group.join("empty")).unwrap();
        let storage = Storage::new(root.join("state"));
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage.clone()));

        assert_eq!(
            project_open(group.to_string_lossy().into_owned(), app.state()).unwrap(),
            3
        );
        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        let find = |core: &CoreWorkspace, title: &str| {
            core.state()
                .projects
                .iter()
                .find(|project| project.title == title)
                .cloned()
                .unwrap_or_else(|| panic!("{title} is a project"))
        };
        let clients = find(&core, "clients");
        let nested = find(&core, "nested");
        assert_eq!(clients.is_group, Some(true));
        assert_eq!(clients.parent_id, None);
        assert_eq!(find(&core, "alpha").parent_id.as_ref(), Some(&clients.id));
        assert_eq!(nested.parent_id.as_ref(), Some(&clients.id));
        assert_eq!(find(&core, "gamma").parent_id.as_ref(), Some(&nested.id));
        assert_eq!(
            core.state().projects.len(),
            5,
            "a folder without projects is left out"
        );

        assert_eq!(project_refresh(clients.id.clone(), app.state()).unwrap(), 0);
        std::fs::create_dir_all(group.join("delta").join(".git")).unwrap();
        assert_eq!(project_refresh(clients.id.clone(), app.state()).unwrap(), 1);
        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(find(&core, "delta").parent_id.as_ref(), Some(&clients.id));

        // Removing the group keeps its children, moved up a level.
        project_remove(clients.id.clone(), app.state()).unwrap();
        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(core.state().projects.len(), 5);
        assert_eq!(find(&core, "alpha").parent_id, None);
        assert_eq!(find(&core, "gamma").parent_id.as_ref(), Some(&nested.id));
        std::fs::remove_dir_all(&root).ok();
    }
}
