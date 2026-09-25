//! AI Limits for the whole window, and the Setup checks.
//!
//! The per-session reading in `usage_read` answers "what is this session's
//! account at"; the status bar asks the same of the default login, as the
//! macOS app does: Codex through its own CLI, Claude from the newest reading
//! its status line left behind. Neither copies a token or sends a model
//! request.

use super::Workspace;
use convoy_core::model::Agent;
use serde::Serialize;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;
use tauri::State;

#[derive(Serialize, Clone)]
pub struct LimitWindow {
    pub name: String,
    pub percent: f64,
    /// Seconds since the epoch, when the provider says.
    pub resets_at: Option<f64>,
    pub minutes: Option<f64>,
}

#[derive(Serialize)]
pub struct ProviderLimits {
    pub windows: Vec<LimitWindow>,
    /// Why nothing is shown, when nothing is.
    pub note: Option<String>,
    /// When the reading was taken, seconds since the epoch.
    pub updated_at: Option<f64>,
}

#[derive(Serialize)]
pub struct LimitsView {
    pub claude: ProviderLimits,
    pub codex: Option<ProviderLimits>,
}

fn window(value: &serde_json::Value) -> Option<LimitWindow> {
    Some(LimitWindow {
        name: value.get("name")?.as_str().unwrap_or("window").to_string(),
        percent: value.get("percent")?.as_f64()?,
        resets_at: value.get("resetsAt").and_then(|v| v.as_f64()),
        minutes: value.get("minutes").and_then(|v| v.as_f64()),
    })
}

/// The newest Claude usage any session reported, skipping expired windows.
fn claude_limits(workspace: &Workspace, now: f64) -> Result<ProviderLimits, String> {
    let telemetry = workspace.storage.telemetry();
    let ids: Vec<String> = workspace.with(|core| {
        Ok(core
            .state()
            .sessions
            .iter()
            .filter(|session| session.agent == Agent::Claude)
            .map(|session| session.id.clone())
            .collect())
    })?;
    let newest = ids
        .iter()
        .filter_map(|id| {
            let file = convoy_core::telemetry::with_suffix(
                &convoy_core::telemetry::output_path(&telemetry, id),
                ".usage",
            );
            let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
            Some((modified, id))
        })
        .max_by_key(|(modified, _)| *modified);
    let Some((modified, id)) = newest else {
        return Ok(ProviderLimits {
            windows: Vec::new(),
            note: Some(
                "No reading yet. Turn on the Claude usage status line in Settings → AI Limits, then send a message in a Claude session."
                    .into(),
            ),
            updated_at: None,
        });
    };
    let windows: Vec<LimitWindow> = convoy_core::telemetry::read(&telemetry, id, ".usage")
        .and_then(|value| value.get("windows").cloned())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(window)
        .filter(|window| window.resets_at.is_none_or(|resets| resets > now))
        .collect();
    let note = windows.is_empty().then(|| {
        "The last reading has expired. It refreshes after the next Claude response.".to_string()
    });
    Ok(ProviderLimits {
        windows,
        note,
        updated_at: modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|elapsed| elapsed.as_secs_f64()),
    })
}

/// Reads both providers. Codex is asked only when `codex` is set: it starts
/// the CLI, which takes a moment, so the status bar asks on demand.
#[tauri::command]
pub async fn limits_read(
    codex: bool,
    workspace: State<'_, Workspace>,
) -> Result<LimitsView, String> {
    let now = convoy_core::provider::now_seconds();
    let claude = claude_limits(&workspace, now)?;
    let codex = if codex {
        let reading = tauri::async_runtime::spawn_blocking(move || {
            let env = convoy_core::provider::launch::current_environment();
            let home = convoy_core::storage::home(convoy_core::Platform::current());
            convoy_core::provider::codex::codex_limits(&env, &home, now)
        })
        .await
        .map_err(|error| error.to_string())?;
        Some(match reading {
            Ok(windows) => ProviderLimits {
                note: windows
                    .is_empty()
                    .then(|| "Codex reported no active quota window.".to_string()),
                windows: windows
                    .into_iter()
                    .map(|window| LimitWindow {
                        name: window.name,
                        percent: window.percent,
                        resets_at: window.resets_at,
                        minutes: window.minutes,
                    })
                    .collect(),
                updated_at: Some(now),
            },
            Err(error) => ProviderLimits {
                windows: Vec::new(),
                note: Some(error.to_string()),
                updated_at: None,
            },
        })
    } else {
        None
    };
    Ok(LimitsView { claude, codex })
}

// ------------------------------------------------------------------- Setup --

#[derive(Serialize)]
pub struct Check {
    pub name: String,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub purpose: String,
    pub install: String,
}

const TOOLS: [(&str, &str, &str); 5] = [
    (
        "claude",
        "Runs Claude Code sessions.",
        "npm install -g @anthropic-ai/claude-code",
    ),
    (
        "codex",
        "Runs Codex sessions and reads its limits.",
        "npm install -g @openai/codex",
    ),
    (
        "git",
        "Branches, worktrees and Files & Changes.",
        "Install Git from git-scm.com or your package manager.",
    ),
    (
        "gh",
        "Opens pull requests from tasks and Files & Changes.",
        "Install the GitHub CLI from cli.github.com, then run gh auth login.",
    ),
    (
        "node",
        "Runs the npm-installed agent CLIs.",
        "Install Node.js 20 or newer from nodejs.org.",
    ),
];

/// Finds a tool the way a terminal would: through the login shell on Unix,
/// so the PATH matches what an agent session sees.
fn locate(tool: &str) -> (Option<String>, Option<String>) {
    let output = if cfg!(windows) {
        std::process::Command::new("where").arg(tool).output()
    } else {
        std::process::Command::new("/bin/bash")
            .args([
                "-lc",
                &format!("command -v {tool} && {tool} --version 2>/dev/null | head -1"),
            ])
            .output()
    };
    let Ok(output) = output else {
        return (None, None);
    };
    if !output.status.success() {
        return (None, None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let path = lines.next().map(str::to_string);
    let version = if cfg!(windows) {
        path.as_ref().and_then(|found| {
            let out = std::process::Command::new(found)
                .arg("--version")
                .output()
                .ok()?;
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .map(|line| line.trim().to_string())
        })
    } else {
        lines.next().map(str::to_string)
    };
    (path, version)
}

#[tauri::command]
pub async fn diagnostics_run() -> Result<Vec<Check>, String> {
    let handles: Vec<_> = TOOLS
        .iter()
        .map(|(name, purpose, install)| {
            let (name, purpose, install) =
                (name.to_string(), purpose.to_string(), install.to_string());
            tauri::async_runtime::spawn_blocking(move || {
                let (path, version) = locate(&name);
                Check {
                    found: path.is_some(),
                    name,
                    path,
                    version,
                    purpose,
                    install,
                }
            })
        })
        .collect();
    let mut checks = Vec::new();
    for handle in handles {
        checks.push(handle.await.map_err(|error| error.to_string())?);
    }
    Ok(checks)
}

/// Where Convoy keeps the worktrees it creates.
#[tauri::command]
pub fn worktrees_path(workspace: State<'_, Workspace>) -> String {
    let path: PathBuf = workspace.storage.worktrees();
    path.to_string_lossy().into_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::locate;

    #[test]
    fn finds_a_tool_through_the_login_shell_and_reports_a_missing_one() {
        let (path, version) = locate("git");
        assert!(path.is_some_and(|path| path.ends_with("/git")));
        assert!(version.is_some_and(|version| version.starts_with("git version")));
        assert_eq!(locate("convoy-no-such-tool"), (None, None));
    }
}

#[derive(Serialize)]
pub struct ProjectGit {
    pub branch: String,
    pub changed_files: usize,
}

/// Branch and changed-file count for a project folder, for the sidebar's
/// detailed rows. A folder that is not a repository has nothing to show.
#[tauri::command]
pub async fn project_git(
    id: String,
    workspace: State<'_, Workspace>,
) -> Result<Option<ProjectGit>, String> {
    let path = workspace.with(|core| {
        core.state()
            .projects
            .iter()
            .find(|project| project.id == id)
            .map(|project| project.path.clone())
            .ok_or_else(|| "Project not found.".to_string())
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        convoy_core::Git::default()
            .status(&path)
            .ok()
            .map(|status| ProjectGit {
                branch: status.branch,
                changed_files: status.changed_files,
            })
    })
    .await
    .map_err(|error| error.to_string())
}
