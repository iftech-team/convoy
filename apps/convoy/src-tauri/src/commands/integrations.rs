//! Account profiles, the provider's own conversations, quota readings and the
//! activity log.

use super::{directory_or_project, Workspace};
use convoy_core::model::Agent;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct ProfileView {
    pub id: String,
    pub label: String,
    pub agent: String,
    /// Sessions bound to it. One that is in use cannot be removed.
    pub sessions: usize,
}

#[tauri::command]
pub fn profiles_read(workspace: State<'_, Workspace>) -> Result<Vec<ProfileView>, String> {
    workspace.with(|workspace| {
        let state = workspace.state();
        Ok(state
            .profiles
            .iter()
            .map(|profile| ProfileView {
                id: profile.id.clone(),
                label: profile.label.clone(),
                agent: profile.agent.as_str().to_string(),
                sessions: state
                    .sessions
                    .iter()
                    .filter(|session| session.profile_id.as_deref() == Some(profile.id.as_str()))
                    .count(),
            })
            .collect())
    })
}

#[tauri::command]
pub fn profile_add(
    label: String,
    agent: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent.")?;
    workspace.act(|workspace| workspace.add_profile(&label, agent).map(|_| ()))
}

/// Removes Convoy's record of a profile. The credential files stay where they
/// are, and a profile still bound to saved sessions cannot be removed at all.
#[tauri::command]
pub fn profile_remove(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| {
        let bound = workspace
            .state()
            .sessions
            .iter()
            .any(|session| session.profile_id.as_deref() == Some(id.as_str()));
        if bound {
            return Err(convoy_core::ConvoyError::message(
                "This account is bound to saved sessions. Remove their projects before removing the profile.",
            ));
        }
        workspace
            .update(move |state| {
                state.profiles.retain(|profile| profile.id != id);
                Ok(())
            })
            .map(|_| ())
    })
}

#[derive(Serialize)]
pub struct TranscriptView {
    pub provider_id: String,
    pub title: String,
    pub at: f64,
}

/// The conversations the provider already has for a folder. Nothing is copied
/// out of the provider's own storage; importing records the identity so the
/// conversation can be resumed exactly.
#[tauri::command]
pub fn transcripts_scan(
    project_id: String,
    agent: String,
    profile_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<TranscriptView>, String> {
    let agent = Agent::parse(&agent).ok_or("Invalid provider.")?;
    let storage = workspace.storage.clone();
    let (home, path) = workspace.act(|core| {
        let project = core.project(&project_id)?.clone();
        let mut probe = convoy_core::model::Session::new(project_id.clone(), agent, "scan");
        probe.profile_id = profile_id.clone().filter(|id| !id.is_empty());
        let account = convoy_core::accounts::account_environment(
            &probe,
            &core.state().profiles,
            &storage.accounts(),
            &convoy_core::provider::launch::current_environment(),
        )?;
        Ok((account.home, project.path))
    })?;

    Ok(
        convoy_core::provider::transcripts::scan(agent, &home, &path)
            .into_iter()
            .map(|transcript| TranscriptView {
                provider_id: transcript.provider_id,
                title: transcript.title,
                at: transcript.at,
            })
            .collect(),
    )
}

/// Records an existing conversation as a session. It is marked started, so
/// opening it resumes that exact conversation rather than beginning a new one.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn transcripts_import(
    project_id: String,
    agent: String,
    provider_id: String,
    title: String,
    profile_id: Option<String>,
    directory: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let agent = Agent::parse(&agent).ok_or("Invalid provider.")?;
    let storage = workspace.storage.clone();
    workspace.act(|core| {
        let mut probe = convoy_core::model::Session::new(project_id.clone(), agent, "import");
        probe.profile_id = profile_id.clone().filter(|id| !id.is_empty());
        let account = convoy_core::accounts::account_environment(
            &probe,
            &core.state().profiles,
            &storage.accounts(),
            &convoy_core::provider::launch::current_environment(),
        )?;
        let home = account.home.clone();
        let project_path = Some(core.project(&project_id)?.path.clone());

        if let Some(existing) = core.state().sessions.iter().find(|session| {
            session.provider_id == provider_id
                && session.agent == agent
                && session.agent_home.as_deref() == Some(home.as_path())
        }) {
            return Ok(existing.id.clone());
        }

        let mut input = convoy_core::workspace::NewSession::new(
            project_id.clone(),
            agent,
            if title.trim().is_empty() {
                "Imported session".to_string()
            } else {
                title.clone()
            },
        );
        input.profile_id = probe.profile_id.clone();
        let state = core.add_session(input)?;
        let id = state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default();

        let bound = id.clone();
        let identity = provider_id.clone();
        core.update(move |state| {
            if let Some(session) = state.sessions.iter_mut().find(|s| s.id == bound) {
                session.provider_id = identity;
                session.started = true;
                session.agent_home = Some(home);
                // A conversation held in a worktree resumes only from there.
                session.working_directory = directory
                    .map(std::path::PathBuf::from)
                    .filter(|folder| Some(folder.as_path()) != project_path.as_deref());
            }
            Ok(())
        })?;
        Ok(id)
    })
}

#[derive(Serialize)]
pub struct ActivityView {
    pub id: String,
    pub at: String,
    pub kind: String,
    pub session_id: String,
    pub title: String,
    pub detail: String,
    /// The session's project, for the feed that spans all projects.
    pub project_id: Option<String>,
    pub project: Option<String>,
}

#[tauri::command]
pub fn activity_read(workspace: State<'_, Workspace>) -> Result<Vec<ActivityView>, String> {
    workspace.with(|workspace| {
        let state = workspace.state();
        let owner = |session_id: &str| {
            let project_id = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .map(|session| session.project_id.clone())?;
            let title = state
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.title.clone());
            Some((project_id, title))
        };
        Ok(state
            .activity
            .iter()
            .map(|event| ActivityView {
                project_id: owner(&event.session_id).map(|(id, _)| id),
                project: owner(&event.session_id).and_then(|(_, title)| title),
                id: event.id.clone(),
                at: event.at.clone(),
                kind: format!("{:?}", event.kind).to_lowercase(),
                session_id: event.session_id.clone(),
                title: event.title.clone(),
                detail: event.detail.clone(),
            })
            .collect())
    })
}

/// Empties the activity feed.
#[tauri::command]
pub fn activity_clear(workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|core| core.clear_activity().map(|_| ()))
}

#[derive(Serialize)]
pub struct UsageView {
    pub windows: Vec<UsageWindow>,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct UsageWindow {
    pub name: String,
    pub percent: f64,
}

/// Quota windows. Claude reports them through its hooks; Codex is asked
/// directly, with no model request involved. A missing quota is reported as
/// unavailable and never guessed as zero.
#[tauri::command]
pub fn usage_read(id: String, workspace: State<'_, Workspace>) -> Result<UsageView, String> {
    let telemetry = workspace.storage.telemetry();
    let storage = workspace.storage.clone();
    let agent = workspace.act(|core| Ok(core.session(&id)?.agent))?;
    let now = convoy_core::provider::now_seconds();

    if agent == Agent::Claude {
        let stored = convoy_core::telemetry::read(&telemetry, &id, ".usage");
        let windows: Vec<UsageWindow> = stored
            .as_ref()
            .and_then(|value| value.get("windows"))
            .and_then(|value| value.as_array())
            .map(|windows| {
                windows
                    .iter()
                    .filter(|window| {
                        window
                            .get("resetsAt")
                            .and_then(|value| value.as_f64())
                            .is_none_or(|resets| resets > now)
                    })
                    .map(|window| UsageWindow {
                        name: window
                            .get("name")
                            .and_then(|value| value.as_str())
                            .unwrap_or("window")
                            .to_string(),
                        percent: window
                            .get("percent")
                            .and_then(|value| value.as_f64())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let note = windows.is_empty().then(|| {
            "Claude reports usage through its status line. Turn it on in Settings and relaunch the session.".to_string()
        });
        return Ok(UsageView { windows, note });
    }

    let directory = directory_or_project(&workspace, &id)?;
    let account = workspace.act(|core| {
        let session = core.session(&id)?.clone();
        convoy_core::session::prepare_account(core, &storage, &session)
    })?;
    let windows = convoy_core::provider::codex::codex_limits(&account.env, &directory, now)
        .map_err(|error| error.to_string())?;
    let note = windows
        .is_empty()
        .then(|| "Codex reported no active quota window.".to_string());
    Ok(UsageView {
        windows: windows
            .into_iter()
            .map(|window| UsageWindow {
                name: window.name,
                percent: window.percent,
            })
            .collect(),
        note,
    })
}

#[derive(Serialize)]
pub struct HistoryEntry {
    pub agent: String,
    pub provider_id: String,
    pub title: String,
    pub at: f64,
    /// The folder the conversation was held in: the project, or a worktree.
    pub directory: String,
    /// The account it belongs to; none is the system login.
    pub profile_id: Option<String>,
    pub profile: Option<String>,
    /// The session already bound to it, when there is one.
    pub session_id: Option<String>,
}

/// Every conversation the agents saved for a project: in its folder and in its
/// sessions' worktrees, for both agents, under the system login and every
/// account. Newest first, each once.
#[tauri::command]
pub fn history_scan(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<HistoryEntry>, String> {
    let storage = workspace.storage.clone();
    let (folders, homes, sessions) = workspace.act(|core| {
        let project = core.project(&project_id)?.clone();
        let state = core.state();
        let mut folders = vec![project.path.clone()];
        for session in state.sessions.iter().filter(|s| s.project_id == project_id) {
            if let Some(folder) = &session.working_directory {
                if !folders.contains(folder) {
                    folders.push(folder.clone());
                }
            }
        }
        let environment = convoy_core::provider::launch::current_environment();
        let mut homes = Vec::new();
        for agent in [Agent::Claude, Agent::Codex] {
            let mut probe = convoy_core::model::Session::new(project_id.clone(), agent, "history");
            let system = convoy_core::accounts::account_environment(
                &probe,
                &state.profiles,
                &storage.accounts(),
                &environment,
            )?;
            homes.push((agent, None, None, system.home));
            for profile in state.profiles.iter().filter(|p| p.agent == agent) {
                probe.profile_id = Some(profile.id.clone());
                let account = convoy_core::accounts::account_environment(
                    &probe,
                    &state.profiles,
                    &storage.accounts(),
                    &environment,
                )?;
                homes.push((
                    agent,
                    Some(profile.id.clone()),
                    Some(profile.label.clone()),
                    account.home,
                ));
            }
        }
        let sessions: Vec<(String, Agent, String)> = state
            .sessions
            .iter()
            .map(|s| (s.provider_id.clone(), s.agent, s.id.clone()))
            .collect();
        Ok((folders, homes, sessions))
    })?;

    let mut entries: Vec<HistoryEntry> = Vec::new();
    for (agent, profile_id, profile, home) in &homes {
        for folder in &folders {
            for transcript in convoy_core::provider::transcripts::scan(*agent, home, folder) {
                if entries
                    .iter()
                    .any(|entry| entry.provider_id == transcript.provider_id)
                {
                    continue;
                }
                let session_id = sessions
                    .iter()
                    .find(|(id, kind, _)| *id == transcript.provider_id && kind == agent)
                    .map(|(_, _, session)| session.clone());
                entries.push(HistoryEntry {
                    agent: agent.as_str().to_string(),
                    provider_id: transcript.provider_id,
                    title: transcript.title,
                    at: transcript.at,
                    directory: folder.to_string_lossy().into_owned(),
                    profile_id: profile_id.clone(),
                    profile: profile.clone(),
                    session_id,
                });
            }
        }
    }
    entries.sort_by(|a, b| b.at.total_cmp(&a.at));
    Ok(entries)
}
