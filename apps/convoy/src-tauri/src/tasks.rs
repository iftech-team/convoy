//! Tasks and the import from Linear, Jira and Peshbar that feeds them.
//!
//! As with the rest of the commands, the rules live in `convoy-core`: which
//! request to send, how to read the answer, how an issue becomes a task. The
//! only thing done here is the HTTP call itself, off the main thread.

use convoy_core::integrations::{
    self, Connection, HttpRequest, Integrations, Issue, IssueSource, ProjectLink, ProjectLinks,
    Scope, TrackerAuth, TrackerKind,
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
    links: Mutex<Option<ProjectLinks>>,
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

    pub(crate) fn with_links<T>(
        &self,
        workspace: &Workspace,
        action: impl FnOnce(&mut ProjectLinks) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.links.lock().map_err(|_| "Integrations unavailable.")?;
        if guard.is_none() {
            let loaded = ProjectLinks::load(workspace.storage.integration_links())
                .map_err(|error| error.to_string())?;
            *guard = Some(loaded);
        }
        action(guard.as_mut().expect("just loaded"))
    }

    /// Drops the cached copies, so the next call reads what is on disk now.
    pub fn forget(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.links.lock() {
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
    /// What its workspace and project levels are called (`TrackerKind::levels`).
    levels: (&'static str, Option<&'static str>),
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
            levels: connection.kind.levels(),
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
    })?;
    trackers.with_links(&workspace, |links| {
        links
            .forget_connection(&id)
            .map_err(|error| error.to_string())
    })
}

/// Sends a request and returns the body of a successful answer; a failed
/// one is read as the tracker's own error message.
pub(crate) fn send(connection: &Connection, request: &HttpRequest) -> Result<String, String> {
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
        Ok(response) => response.into_string().map_err(|error| error.to_string()),
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

fn fetch(
    connection: &Connection,
    link: Option<&ProjectLink>,
    query: &str,
    mine_only: bool,
    raw_jql: bool,
) -> Result<Vec<Issue>, String> {
    let request = integrations::search_request_in(connection, link, query, mine_only, raw_jql)
        .map_err(|error| error.to_string())?;
    let body = send(connection, &request)?;
    integrations::parse_issues(connection.kind, &body, connection.site_url().as_deref())
        .map(|issues| integrations::narrow(connection.kind, issues, link))
        .map(|issues| integrations::with_origins(issues, connection))
        .map_err(|error| error.to_string())
}

/// Searches a connection: free text, an issue key or link, or Jira JQL;
/// `raw_jql` sends the query to Jira as written.
/// Searching from a project that is linked to this connection stays inside
/// the linked workspace and project.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn issues_search(
    connection_id: String,
    query: String,
    mine_only: bool,
    raw_jql: bool,
    project_id: Option<String>,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Issue>, String> {
    let connection = connection(&trackers, &workspace, &connection_id)?;
    let link = match project_id {
        Some(project_id) => trackers.with_links(&workspace, |links| {
            Ok(links
                .get(&project_id)
                .filter(|link| link.connection_id == connection_id)
                .cloned())
        })?,
        None => None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        fetch(&connection, link.as_ref(), &query, mine_only, raw_jql)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn connection(
    trackers: &Trackers,
    workspace: &Workspace,
    id: &str,
) -> Result<Connection, String> {
    trackers.with(workspace, |integrations| {
        integrations
            .get(id)
            .cloned()
            .map_err(|error| error.to_string())
    })
}

/// A connection's workspaces, or with `parent` the projects inside one.
#[tauri::command]
pub async fn integration_scopes(
    connection_id: String,
    parent: Option<Scope>,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Scope>, String> {
    let connection = connection(&trackers, &workspace, &connection_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let request = integrations::scopes_request(&connection, parent.as_ref())
            .map_err(|error| error.to_string())?;
        let body = send(&connection, &request)?;
        integrations::parse_scopes(connection.kind, parent.is_some(), &body)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Where a project imports from, if it is linked.
#[tauri::command]
pub fn project_link_get(
    project_id: String,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Option<ProjectLink>, String> {
    trackers.with_links(&workspace, |links| Ok(links.get(&project_id).cloned()))
}

/// Links a project to a connection and, optionally, a workspace and project
/// in it; `None` unlinks it.
#[tauri::command]
pub fn project_link_save(
    project_id: String,
    link: Option<ProjectLink>,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    if let Some(link) = &link {
        connection(&trackers, &workspace, &link.connection_id)?;
    }
    trackers.with_links(&workspace, |links| {
        links
            .set(&project_id, link)
            .map_err(|error| error.to_string())
    })
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

/// A Notion page's text lives in its blocks, which the search does not
/// return: each page is read here, before it becomes a task. A page that
/// cannot be read keeps the properties the search showed.
fn with_page_text(connection: &Connection, issues: Vec<Issue>) -> Vec<Issue> {
    issues
        .into_iter()
        .map(|mut issue| {
            let text = issue.url.as_deref().and_then(|url| {
                let request = integrations::notion::content_request(connection, url).ok()?;
                let body = send(connection, &request).ok()?;
                integrations::notion::parse_content(&body).ok()
            });
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                issue.details = if issue.details.is_empty() {
                    text
                } else {
                    format!("{text}\n\n{}", issue.details)
                };
            }
            issue
        })
        .collect()
}

#[tauri::command]
pub async fn issues_import(
    project_id: String,
    connection_id: String,
    issues: Vec<Issue>,
    options: ImportInput,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<Imported, String> {
    let connection = connection(&trackers, &workspace, &connection_id)?;
    let issues = if connection.kind == TrackerKind::Notion && connection.auth != TrackerAuth::Mcp {
        let reader = connection.clone();
        tauri::async_runtime::spawn_blocking(move || with_page_text(&reader, issues))
            .await
            .map_err(|error| error.to_string())?
    } else {
        issues
    };
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
    let skills = crate::skills::resolve(&workspace.storage, &project_id, None, None)?;
    let options = ImportOptions {
        skills,
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
    effort: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent.")?;
    workspace.with(|workspace| {
        workspace
            .set_task_agent(&id, agent, &model, effort.as_deref().unwrap_or_default())
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

/// Opens the pull request a task's session opened. Only the link recorded on
/// the task is opened, never one the page supplies.
#[tauri::command]
pub fn task_pr_open(
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
            .and_then(|task| task.pr_url.clone())
            .ok_or_else(|| "This task has no pull request.".to_string())
    })?;
    // Self-hosted servers on a private network may still serve plain HTTP.
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Pull request links must use HTTP or HTTPS.".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
