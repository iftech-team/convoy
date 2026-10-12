//! "Import all history": every Claude Code and Codex conversation on this
//! computer, matched to a project, reviewed, then recorded as sessions. The
//! rules are `convoy_core::provider::sweep`; this only carries them across.

use super::Workspace;
use convoy_core::model::{Agent, Session};
use convoy_core::provider::sweep::{self, Found, Reason, Roots};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use tauri::State;

/// The last scan, kept so an import acts on exactly what the user reviewed
/// rather than on paths the window sends back.
#[derive(Default)]
pub struct Sweep {
    found: Mutex<Vec<Found>>,
}

#[derive(Serialize)]
pub struct SweepRow {
    pub agent: String,
    pub provider_id: String,
    pub title: String,
    pub at: f64,
    /// Where it was started, with the home folder as `~`; empty when unknown.
    pub place: String,
    pub project_id: Option<String>,
    /// `folder`, `paths` or `none`.
    pub reason: &'static str,
    /// For `paths`, how many tool calls referred to the project.
    pub votes: u32,
    /// For an unassigned row, the projects it mentioned most.
    pub suggestions: Vec<String>,
}

#[derive(Deserialize)]
pub struct SweepChoice {
    pub provider_id: String,
    pub project_id: String,
}

#[derive(Serialize)]
pub struct SweepReport {
    pub imported: usize,
    /// Rows skipped because Claude's transcript could not be copied.
    pub failed: usize,
}

/// The system login's homes for both agents, honouring `CLAUDE_CONFIG_DIR`
/// and `CODEX_HOME` as a launch would.
pub(crate) fn roots(
    core: &convoy_core::Workspace,
    storage: &convoy_core::Storage,
) -> convoy_core::Result<Roots> {
    let environment = convoy_core::provider::launch::current_environment();
    let home = |agent| {
        convoy_core::accounts::account_environment(
            &Session::new(String::new(), agent, "sweep"),
            &core.state().profiles,
            &storage.accounts(),
            &environment,
        )
        .map(|account| account.home)
    };
    Ok(Roots {
        claude: home(Agent::Claude)?,
        codex: home(Agent::Codex)?,
        home: convoy_core::storage::home(convoy_core::Platform::current()),
    })
}

fn shortened(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Reads every conversation on disk. It can take a while on a busy machine, so
/// it runs on a blocking thread and the window stays responsive.
#[tauri::command]
pub async fn history_sweep_scan(
    workspace: State<'_, Workspace>,
    sweep: State<'_, Sweep>,
) -> Result<Vec<SweepRow>, String> {
    let storage = workspace.storage.clone();
    let (roots, targets, known) = workspace.act(|core| {
        let roots = roots(core, &storage)?;
        let targets = sweep::targets(core.state(), &roots.home);
        Ok((roots, targets, sweep::known(core.state())))
    })?;
    let home = roots.home.clone();
    let found = tauri::async_runtime::spawn_blocking(move || sweep::scan(&roots, &targets, &known))
        .await
        .map_err(|error| error.to_string())?;
    let rows = found
        .iter()
        .map(|item| SweepRow {
            agent: item.agent.as_str().to_string(),
            provider_id: item.provider_id.clone(),
            title: item.title.clone(),
            at: item.at,
            place: item
                .cwd
                .as_deref()
                .map(|cwd| shortened(cwd, &home))
                .unwrap_or_default(),
            project_id: item.project_id.clone(),
            reason: match item.reason {
                Reason::Folder => "folder",
                Reason::Paths(_) => "paths",
                Reason::Unassigned => "none",
            },
            votes: match item.reason {
                Reason::Paths(count) => count,
                _ => 0,
            },
            suggestions: item.suggestions.clone(),
        })
        .collect();
    *sweep.found.lock().map_err(|_| "Scan unavailable.")? = found;
    Ok(rows)
}

/// Records the chosen conversations in the chosen projects, copying Claude's
/// transcript first where it has to resume from another folder. A copy that
/// fails skips that row only. Nothing is launched; the workspace is saved once.
#[tauri::command]
pub async fn history_sweep_import(
    choices: Vec<SweepChoice>,
    workspace: State<'_, Workspace>,
    sweep: State<'_, Sweep>,
) -> Result<SweepReport, String> {
    let picked: Vec<(Found, String)> = {
        let found = sweep.found.lock().map_err(|_| "Scan unavailable.")?;
        choices
            .iter()
            .filter_map(|choice| {
                found
                    .iter()
                    .find(|item| item.provider_id == choice.provider_id)
                    .map(|item| (item.clone(), choice.project_id.clone()))
            })
            .collect()
    };
    let storage = workspace.storage.clone();
    let (roots, targets) = workspace.act(|core| {
        let roots = roots(core, &storage)?;
        let targets = sweep::targets(core.state(), &roots.home);
        Ok((roots, targets))
    })?;

    let (imports, failed) = tauri::async_runtime::spawn_blocking(move || {
        let mut imports = Vec::new();
        let mut failed = 0;
        for (found, project_id) in picked {
            let Some(placement) = sweep::place(&found, &project_id, &targets, &roots) else {
                continue;
            };
            if let Some(into) = &placement.copy_into {
                if sweep::copy_transcript(&found.file, into).is_err() {
                    failed += 1;
                    continue;
                }
            }
            imports.push(sweep::Import {
                agent: found.agent,
                provider_id: found.provider_id,
                title: found.title,
                project_id,
                working_directory: placement.working_directory,
                agent_home: roots.agent_home(found.agent).to_path_buf(),
                at: found.at,
            });
        }
        (imports, failed)
    })
    .await
    .map_err(|error| error.to_string())?;

    let mut imported = 0;
    if !imports.is_empty() {
        workspace.act(|core| {
            core.update(|state| {
                imported = sweep::record(state, imports);
                Ok(())
            })
            .map(|_| ())
        })?;
    }
    if let Ok(mut found) = sweep.found.lock() {
        found.clear();
    }
    Ok(SweepReport { imported, failed })
}
