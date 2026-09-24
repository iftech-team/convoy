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
pub fn transcripts_import(
    project_id: String,
    agent: String,
    provider_id: String,
    title: String,
    profile_id: Option<String>,
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
}

#[tauri::command]
pub fn activity_read(workspace: State<'_, Workspace>) -> Result<Vec<ActivityView>, String> {
    workspace.with(|workspace| {
        Ok(workspace
            .state()
            .activity
            .iter()
            .map(|event| ActivityView {
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
