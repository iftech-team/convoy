//! Tasks and the Linear/Jira import that feeds them.
//!
//! As with the rest of the commands, the rules live in `convoy-core`: which
//! request to send, how to read the answer, how an issue becomes a task. The
//! only thing done here is the HTTP call itself, off the main thread.

use convoy_core::integrations::{
    self, Connection, Integrations, Issue, IssueSource, TrackerAuth, TrackerKind,
};
use convoy_core::planning::ImportOptions;
use convoy_core::workspace::model::{Agent, PublishMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::State;

use crate::commands::Workspace;

/// Saved connections, loaded on first use from beside `workspace.json`.
#[derive(Default)]
pub struct Trackers {
    inner: Mutex<Option<Integrations>>,
}

impl Trackers {
    fn with<T>(
        &self,
        workspace: &Workspace,
        action: impl FnOnce(&mut Integrations) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.inner.lock().map_err(|_| "Integrations unavailable.")?;
        if guard.is_none() {
            let loaded = Integrations::load(workspace.storage.integrations())
                .map_err(|error| error.to_string())?;
            *guard = Some(loaded);
        }
        action(guard.as_mut().expect("just loaded"))
    }

    /// Drops the cached copy, so the next call reads what is on disk now.
    pub fn forget(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

/// A connection as the page sees it: never the secret, only whether one is saved.
#[derive(Serialize)]
pub struct ConnectionView {
    id: String,
    kind: TrackerKind,
    auth: TrackerAuth,
    name: String,
    site: Option<String>,
    username: Option<String>,
    has_secret: bool,
}

impl From<&Connection> for ConnectionView {
    fn from(connection: &Connection) -> Self {
        ConnectionView {
            id: connection.id.clone(),
            kind: connection.kind,
            auth: connection.auth,
            name: connection.name.clone(),
            site: connection.site.clone(),
            username: connection.username.clone(),
            has_secret: !connection.secret.is_empty(),
        }
    }
}

#[derive(Deserialize)]
pub struct ConnectionInput {
    #[serde(default)]
    id: String,
    kind: TrackerKind,
    auth: TrackerAuth,
    name: String,
    #[serde(default)]
    site: Option<String>,
    #[serde(default)]
    username: Option<String>,
    /// Empty keeps the saved secret.
    #[serde(default)]
    secret: String,
}

#[tauri::command]
pub fn integrations_list(
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<ConnectionView>, String> {
    trackers.with(&workspace, |integrations| {
        Ok(integrations
            .connections()
            .iter()
            .map(ConnectionView::from)
            .collect())
    })
}

#[tauri::command]
pub fn integration_save(
    input: ConnectionInput,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<ConnectionView, String> {
    let blank = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let connection = Connection {
        id: input.id,
        kind: input.kind,
        auth: input.auth,
        name: input.name.trim().to_string(),
        site: blank(input.site),
        username: blank(input.username),
        secret: input.secret,
    };
    trackers.with(&workspace, |integrations| {
        integrations
            .save(connection)
            .map(ConnectionView::from)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn integration_remove(
    id: String,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    trackers.with(&workspace, |integrations| {
        integrations.remove(&id).map_err(|error| error.to_string())
    })
}

fn fetch(
    connection: &Connection,
    query: &str,
    mine_only: bool,
    raw_jql: bool,
) -> Result<Vec<Issue>, String> {
    let request = integrations::search_request(connection, query, mine_only, raw_jql)
        .map_err(|error| error.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let mut call = agent.request(request.method, &request.url);
    for (name, value) in &request.headers {
        call = call.set(name, value);
    }
    let answer = match &request.body {
        Some(body) => call.send_string(body),
        None => call.call(),
    };
    match answer {
        Ok(response) => {
            let body = response.into_string().map_err(|error| error.to_string())?;
            integrations::parse_issues(connection.kind, &body, connection.site_url().as_deref())
                .map(|issues| integrations::with_origins(issues, connection))
                .map_err(|error| error.to_string())
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            Err(integrations::http_error(connection, status, &body).to_string())
        }
        Err(error) => Err(format!(
            "Could not reach {}: {error}",
            connection.kind.label()
        )),
    }
}

/// Searches a connection: free text, an issue key or link, or Jira JQL;
/// `raw_jql` sends the query to Jira as written.
#[tauri::command]
pub async fn issues_search(
    connection_id: String,
    query: String,
    mine_only: bool,
    raw_jql: bool,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Issue>, String> {
    let connection = trackers.with(&workspace, |integrations| {
        integrations
            .get(&connection_id)
            .cloned()
            .map_err(|error| error.to_string())
    })?;
    tauri::async_runtime::spawn_blocking(move || fetch(&connection, &query, mine_only, raw_jql))
        .await
        .map_err(|error| error.to_string())?
}

/// Keys and links typed for an MCP connection, parsed by the core rules.
#[tauri::command]
pub fn issues_parse_keys(
    connection_id: String,
    text: String,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Issue>, String> {
    trackers.with(&workspace, |integrations| {
        let connection = integrations
            .get(&connection_id)
            .map_err(|error| error.to_string())?;
        Ok(integrations::with_origins(
            integrations::manual_issues(&text),
            connection,
        ))
    })
}

#[derive(Deserialize)]
pub struct Pick {
    agent: String,
    #[serde(default)]
    model: String,
}

#[derive(Deserialize)]
pub struct ImportInput {
    agent: String,
    #[serde(default)]
    model: String,
    mode: String,
    #[serde(default)]
    auto_review: bool,
    #[serde(default)]
    overrides: BTreeMap<String, Pick>,
}

#[derive(Serialize)]
pub struct Imported {
    created: Vec<String>,
    skipped: usize,
}

fn mode(value: &str) -> Result<PublishMode, String> {
    match value {
        "pr" => Ok(PublishMode::Pr),
        "push" => Ok(PublishMode::Push),
        "none" => Ok(PublishMode::None),
        other => Err(format!("Unknown publish mode: {other}")),
    }
}

#[tauri::command]
pub fn issues_import(
    project_id: String,
    connection_id: String,
    issues: Vec<Issue>,
    options: ImportInput,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Imported, String> {
    let connection = trackers.with(&workspace, |integrations| {
        integrations
            .get(&connection_id)
            .cloned()
            .map_err(|error| error.to_string())
    })?;
    let mut overrides = BTreeMap::new();
    for (key, pick) in options.overrides {
        overrides.insert(
            key,
            (
                Agent::parse(&pick.agent).ok_or("Unknown agent.")?,
                pick.model,
            ),
        );
    }
    let options = ImportOptions {
        agent: Agent::parse(&options.agent).ok_or("Unknown agent.")?,
        model: options.model,
        mode: mode(&options.mode)?,
        auto_review: options.auto_review,
        overrides,
    };
    workspace.with(|workspace| {
        let before: std::collections::HashSet<String> = workspace
            .state()
            .tasks
            .iter()
            .map(|task| task.id.clone())
            .collect();
        let (_, skipped) = workspace
            .import_issues(&project_id, &connection, &issues, &options)
            .map_err(|error| error.to_string())?;
        // The new tasks, in issue order, so "run immediately" starts exactly these.
        let created = workspace
            .state()
            .tasks
            .iter()
            .filter(|task| !before.contains(&task.id))
            .map(|task| task.id.clone())
            .collect();
        Ok(Imported { created, skipped })
    })
}

#[derive(Serialize)]
pub struct TaskView {
    id: String,
    title: String,
    details: String,
    agent: String,
    model: Option<String>,
    status: String,
    mode: PublishMode,
    session_id: Option<String>,
    last_error: Option<String>,
    source: Option<IssueSource>,
}

#[tauri::command]
pub fn tasks_for(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<TaskView>, String> {
    workspace.with(|workspace| {
        Ok(workspace
            .state()
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id)
            .map(|task| TaskView {
                id: task.id.clone(),
                title: task.title.clone(),
                details: task.details.clone(),
                agent: task.agent.as_str().to_string(),
                model: task.model.clone(),
                status: task.status.as_str().to_string(),
                mode: task.mode,
                session_id: task.session_id.clone(),
                last_error: task.last_error.clone(),
                source: task.source.clone(),
            })
            .collect())
    })
}

#[tauri::command]
pub fn task_agent(
    id: String,
    agent: String,
    model: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent.")?;
    workspace.with(|workspace| {
        workspace
            .set_task_agent(&id, agent, &model)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

/// Open the saved tracker link using the system browser.
#[tauri::command]
pub fn issue_open(
    id: String,
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = workspace.with(|workspace| {
        workspace
            .state()
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.source.as_ref())
            .and_then(|source| source.url.clone())
            .ok_or_else(|| "This task has no issue link.".to_string())
    })?;
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Issue links must use HTTP or HTTPS.".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
