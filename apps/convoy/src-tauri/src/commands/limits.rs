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

#[derive(Serialize)]
pub struct SetupCheck {
    pub id: &'static str,
    pub title: String,
    pub detail: String,
    /// `ok`, `warning` or `failed`.
    pub status: &'static str,
    pub fix: Option<String>,
}

fn check(
    id: &'static str,
    title: impl Into<String>,
    detail: impl Into<String>,
    status: &'static str,
    fix: Option<&str>,
) -> SetupCheck {
    SetupCheck {
        id,
        title: title.into(),
        detail: detail.into(),
        status,
        fix: fix.map(str::to_string),
    }
}

/// Runs a command through the login shell, as a terminal would, and returns
/// whether it succeeded with what it printed.
fn shell(command: &str) -> (bool, String) {
    let output = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", command])
            .output()
    } else {
        std::process::Command::new("/bin/bash")
            .args(["-lc", command])
            .output()
    };
    match output {
        Ok(output) => (
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ),
        Err(_) => (false, String::new()),
    }
}

/// Everything beyond the tools that decides whether sessions will work: the
/// GitHub login, each agent's login for the account new sessions use, the
/// hooks, folder trust, Convoy's own folder, and the projects on disk — the
/// macOS app's Setup check. `claude_profile` and `codex_profile` are the
/// active accounts; none means the system login.
#[tauri::command]
pub async fn setup_checks(
    claude_profile: Option<String>,
    codex_profile: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<SetupCheck>, String> {
    let storage = workspace.storage.clone();
    let (settings, projects, profiles) = workspace.with(|core| {
        Ok((
            core.settings().clone(),
            core.state().projects.clone(),
            core.state().profiles.clone(),
        ))
    })?;

    let gh = tauri::async_runtime::spawn_blocking(|| {
        shell("command -v gh >/dev/null 2>&1 && gh auth status 2>&1 | head -4")
    });
    let keychain = tauri::async_runtime::spawn_blocking(|| {
        cfg!(target_os = "macos")
            && shell("security find-generic-password -s 'Claude Code-credentials' >/dev/null 2>&1")
                .0
    });
    let mut out = Vec::new();

    let (installed, text) = gh.await.map_err(|error| error.to_string())?;
    out.push(if !installed && !text.contains("Logged in") {
        check(
            "gh",
            "GitHub CLI login",
            "Not installed or not logged in — tasks that open pull requests stop at the last step.",
            "warning",
            Some("Install gh, then run `gh auth login` in a terminal."),
        )
    } else if let Some(line) = text.lines().find(|line| line.contains("Logged in")) {
        check(
            "gh",
            "GitHub CLI login",
            line.trim().trim_start_matches(['✓', ' ']),
            "ok",
            None,
        )
    } else {
        check(
            "gh",
            "GitHub CLI login",
            "Installed but not logged in.",
            "warning",
            Some("Run `gh auth login` in a terminal."),
        )
    });

    let environment = convoy_core::provider::launch::current_environment();
    let keychain = keychain.await.map_err(|error| error.to_string())?;
    for (agent, wanted) in [
        (convoy_core::model::Agent::Claude, claude_profile),
        (convoy_core::model::Agent::Codex, codex_profile),
    ] {
        let (name, file) = match agent {
            convoy_core::model::Agent::Claude => ("Claude", ".credentials.json"),
            convoy_core::model::Agent::Codex => ("Codex", "auth.json"),
        };
        let id = if name == "Claude" {
            "login.claude"
        } else {
            "login.codex"
        };
        let profile = wanted.and_then(|wanted| {
            profiles
                .iter()
                .find(|p| p.id == wanted && p.agent == agent)
                .cloned()
        });
        let mut probe = convoy_core::model::Session::new(String::new(), agent, "setup");
        probe.profile_id = profile.as_ref().map(|p| p.id.clone());
        let account = convoy_core::accounts::account_environment(
            &probe,
            &profiles,
            &storage.accounts(),
            &environment,
        )
        .map_err(|error| error.to_string())?;
        let present =
            account.home.join(file).exists() || (profile.is_none() && name == "Claude" && keychain);
        out.push(match profile {
            Some(profile) => check(
                id,
                format!("{name} login (account {})", profile.label),
                if present {
                    "Credentials present"
                } else {
                    "No credentials in this account yet"
                },
                if present { "ok" } else { "warning" },
                Some("Settings → Accounts → Log in… for this account."),
            ),
            None => check(
                id,
                format!("{name} login"),
                if present {
                    "Logged in"
                } else {
                    "No saved login found"
                },
                if present { "ok" } else { "warning" },
                Some(if name == "Claude" {
                    "Run `claude` once in a terminal and complete the login."
                } else {
                    "Run `codex login` in a terminal."
                }),
            ),
        });
    }

    out.push(if settings.status_hooks {
        check(
            "hooks",
            "Agent status hooks",
            "On: Claude sessions report working, waiting and done",
            "ok",
            None,
        )
    } else {
        check(
            "hooks",
            "Agent status hooks",
            "Off: Convoy cannot tell when an agent needs you",
            "warning",
            Some("Settings → Agents → Agent status hooks."),
        )
    });
    out.push(if settings.auto_trust {
        check(
            "trust",
            "Folder trust",
            "Session folders are trusted automatically",
            "ok",
            None,
        )
    } else {
        check(
            "trust",
            "Folder trust",
            "Off: agents ask to trust each new folder",
            "warning",
            Some("Settings → Agents → Trust project folders automatically."),
        )
    });

    let root = storage.root().to_path_buf();
    let probe = root.join(".convoy-write-check");
    let writable = std::fs::create_dir_all(&root).is_ok() && std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    out.push(check(
        "support",
        "Convoy's data folder",
        if writable {
            root.to_string_lossy().into_owned()
        } else {
            format!("Not writable: {}", root.display())
        },
        if writable { "ok" } else { "failed" },
        Some("Fix the folder's permissions."),
    ));

    let missing: Vec<&str> = projects
        .iter()
        .filter(|p| !p.path.exists())
        .map(|p| p.title.as_str())
        .collect();
    out.push(if projects.is_empty() {
        check(
            "projects",
            "Projects",
            "No projects yet.",
            "warning",
            Some("Open a folder to add a repository or a folder of projects."),
        )
    } else if missing.is_empty() {
        check(
            "projects",
            "Projects",
            format!(
                "{} folder{} present",
                projects.len(),
                if projects.len() == 1 { "" } else { "s" }
            ),
            "ok",
            None,
        )
    } else {
        check(
            "projects",
            "Projects",
            format!("Missing on disk: {}", missing.join(", ")),
            "failed",
            Some("Reconnect or remove them from the sidebar."),
        )
    });
    let plain: Vec<&str> = projects
        .iter()
        .filter(|p| p.path.exists() && !p.path.join(".git").exists())
        .map(|p| p.title.as_str())
        .collect();
    if !plain.is_empty() {
        out.push(check("projects.git", "Projects without Git", plain.join(", "), "warning", Some("Worktrees, tasks that push and Files & Changes need a repository. Run `git init` if you want them.")));
    }
    Ok(out)
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

    /// The checks run against the real machine; only what does not depend on
    /// it is asserted: every check is there, Convoy's folder is writable, and
    /// an empty workspace is told it has no projects.
    #[test]
    fn setup_checks_cover_logins_hooks_and_the_workspace() {
        use tauri::Manager;
        let root = std::env::temp_dir().join(format!("convoy-setup-{}", std::process::id()));
        let app = tauri::test::mock_app();
        app.manage(crate::commands::Workspace::at(convoy_core::Storage::new(
            &root,
        )));
        let checks =
            tauri::async_runtime::block_on(super::setup_checks(None, None, app.state())).unwrap();
        let ids: Vec<&str> = checks.iter().map(|check| check.id).collect();
        for id in [
            "gh",
            "login.claude",
            "login.codex",
            "hooks",
            "trust",
            "support",
            "projects",
        ] {
            assert!(ids.contains(&id), "{id} is checked");
        }
        let status = |id: &str| checks.iter().find(|check| check.id == id).unwrap().status;
        assert_eq!(status("support"), "ok");
        assert_eq!(status("projects"), "warning");
        std::fs::remove_dir_all(&root).ok();
    }

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

/// The built-in review brief, for "Insert default".
#[tauri::command]
pub fn review_template_default() -> &'static str {
    convoy_core::review::DEFAULT_TEMPLATE
}
