//! The Docs tab: the project doc, specs and the repository's own Markdown.
//! The rules — which files, which paths are allowed — live in
//! `convoy_core::docs`; these only find the project folder.

use super::Workspace;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

fn root(workspace: &Workspace, project_id: &str) -> Result<PathBuf, String> {
    workspace.act(|core| Ok(core.project(project_id)?.path.clone()))
}

#[tauri::command]
pub fn docs_list(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    Ok(convoy_core::docs::files(&root(&workspace, &project_id)?))
}

#[tauri::command]
pub fn doc_read(
    project_id: String,
    path: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    convoy_core::docs::read(&root(&workspace, &project_id)?, &path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn doc_write(
    project_id: String,
    path: String,
    text: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    convoy_core::docs::write(&root(&workspace, &project_id)?, &path, &text)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn spec_create(
    project_id: String,
    title: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    convoy_core::docs::create_spec(&root(&workspace, &project_id)?, &title)
        .map_err(|error| error.to_string())
}

#[derive(Serialize)]
pub struct DocPrompts {
    pub project_doc: String,
    pub spec: String,
}

/// The prompts the AI actions start a session with, from the core so every
/// build asks the agent the same thing.
#[tauri::command]
pub fn doc_prompts(path: String, title: String) -> DocPrompts {
    DocPrompts {
        project_doc: convoy_core::docs::PROJECT_DOC_PROMPT.to_string(),
        spec: convoy_core::docs::spec_prompt(&path, &title),
    }
}
