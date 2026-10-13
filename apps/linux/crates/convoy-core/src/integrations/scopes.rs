//! Where a project's tasks come from: the levels inside a tracker that a
//! Convoy project is linked to, and the requests that list them.
//!
//! Every tracker has up to two levels below its connection. Peshbar has
//! workspaces and their projects, Linear teams and their projects, Jira
//! projects only. A project's [`ProjectLink`] names a connection and,
//! optionally, one of each; the import then searches only there.

use super::{authorization, Connection, HttpRequest, Issue, TrackerAuth, TrackerKind};
use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A workspace, team or project inside a tracker, as its API names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub id: String,
    pub name: String,
    /// The prefix of its task keys (`ENG` in `ENG-12`), where it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// The connection a Convoy project imports from, and how far it is narrowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLink {
    pub connection_id: String,
    /// Peshbar workspace, Linear team or Jira project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Scope>,
    /// Peshbar or Linear project, inside [`ProjectLink::workspace`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<Scope>,
}

impl TrackerKind {
    /// What the two levels are called; `None` where the tracker has no
    /// second level.
    pub fn levels(self) -> (&'static str, Option<&'static str>) {
        match self {
            TrackerKind::Linear => ("Team", Some("Project")),
            TrackerKind::Jira => ("Project", None),
            TrackerKind::Peshbar => ("Workspace", Some("Project")),
        }
    }
}

pub const LINEAR_TEAMS: &str = "query Teams { teams(first: 100) { nodes { id key name } } }";

pub const LINEAR_PROJECTS: &str = "query Projects($id: String!) {
  team(id: $id) { projects(first: 100) { nodes { id name } } }
}";

/// Lists a connection's workspaces (no `parent`) or the projects inside one.
pub fn scopes_request(connection: &Connection, parent: Option<&Scope>) -> Result<HttpRequest> {
    ensure!(
        connection.auth != TrackerAuth::Mcp,
        "An MCP connection is read by the agent; it lists nothing here."
    );
    let authorization = authorization(connection)?;
    match connection.kind {
        TrackerKind::Linear => {
            let body = match parent {
                None => json!({ "query": LINEAR_TEAMS }),
                Some(team) => json!({ "query": LINEAR_PROJECTS, "variables": { "id": team.id } }),
            };
            Ok(HttpRequest {
                method: "POST",
                url: "https://api.linear.app/graphql".into(),
                headers: vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("Authorization".into(), authorization),
                ],
                body: Some(body.to_string()),
            })
        }
        TrackerKind::Jira => {
            ensure!(parent.is_none(), "Jira projects have no level below them.");
            let Some(site) = connection.site_url() else {
                bail!("Set the Jira site URL first.")
            };
            let url = if connection.is_jira_cloud() {
                format!("{site}/rest/api/3/project/search?maxResults=100&orderBy=name")
            } else {
                format!("{site}/rest/api/2/project")
            };
            Ok(HttpRequest {
                method: "GET",
                url,
                headers: vec![
                    ("Accept".into(), "application/json".into()),
                    ("Authorization".into(), authorization),
                ],
                body: None,
            })
        }
        TrackerKind::Peshbar => {
            let api = connection.peshbar_api();
            let mut headers = vec![
                ("Accept".into(), "application/json".into()),
                ("Authorization".into(), authorization),
            ];
            let url = match parent {
                // The user's workspaces come with the profile.
                None => format!("{api}/api/me"),
                Some(workspace) => {
                    headers.push((WORKSPACE_HEADER.into(), workspace.id.clone()));
                    // Paginated by 15 unless asked for more.
                    format!("{api}/api/projects?per_page=100")
                }
            };
            Ok(HttpRequest {
                method: "GET",
                url,
                headers,
                body: None,
            })
        }
    }
}

/// The header that runs a Peshbar request in one of the user's workspaces
/// instead of the one their web app has open.
pub const WORKSPACE_HEADER: &str = "X-Workspace";

/// Reads what [`scopes_request`] got back, sorted by name.
pub fn parse_scopes(kind: TrackerKind, second_level: bool, body: &str) -> Result<Vec<Scope>> {
    let json: Value = serde_json::from_str(body).map_err(|_| {
        ConvoyError::message(format!("{} sent an unexpected response.", kind.label()))
    })?;
    let text = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    };
    let scope = |item: &Value, key: Option<&Value>| {
        Some(Scope {
            id: text(&item["id"])?,
            name: text(&item["name"])?,
            key: key.and_then(text).filter(|key| !key.is_empty()),
        })
    };
    let mut scopes: Vec<Scope> = match kind {
        TrackerKind::Linear => {
            if let Some(errors) = json["errors"].as_array().filter(|e| !e.is_empty()) {
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|e| e["message"].as_str())
                    .collect();
                bail!("{}", messages.join("; "));
            }
            let nodes = if second_level {
                &json["data"]["team"]["projects"]["nodes"]
            } else {
                &json["data"]["teams"]["nodes"]
            };
            list(nodes)
                .iter()
                .filter_map(|node| scope(node, Some(&node["key"])))
                .collect()
        }
        TrackerKind::Jira => {
            if let Some(messages) = json["errorMessages"].as_array().filter(|m| !m.is_empty()) {
                let messages: Vec<&str> = messages.iter().filter_map(Value::as_str).collect();
                bail!("{}", messages.join("; "));
            }
            // Cloud pages them under `values`; Server answers a bare array.
            let projects = if json.is_array() {
                &json
            } else {
                &json["values"]
            };
            list(projects)
                .iter()
                .filter_map(|project| scope(project, Some(&project["key"])))
                .collect()
        }
        TrackerKind::Peshbar => {
            let data = json.get("data").unwrap_or(&json);
            if let Some(message) = json["message"]
                .as_str()
                .filter(|_| json.get("data").is_none())
            {
                bail!("{message}");
            }
            if second_level {
                list(data)
                    .iter()
                    .filter_map(|project| scope(project, Some(&project["key"])))
                    .collect()
            } else {
                list(&data["companies"])
                    .iter()
                    .filter_map(|company| scope(company, None))
                    .collect()
            }
        }
    };
    scopes.sort_by_key(|scope| scope.name.to_lowercase());
    Ok(scopes)
}

fn list(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

/// The Linear filter, kept inside the linked team and project.
pub(super) fn linear_scoped(filter: Value, link: Option<&ProjectLink>) -> Value {
    let mut scope = serde_json::Map::new();
    if let Some(team) = link.and_then(|link| link.workspace.as_ref()) {
        scope.insert("team".into(), json!({ "id": { "eq": team.id } }));
    }
    if let Some(project) = link.and_then(|link| link.project.as_ref()) {
        scope.insert("project".into(), json!({ "id": { "eq": project.id } }));
    }
    if scope.is_empty() {
        filter
    } else {
        json!({ "and": [filter, Value::Object(scope)] })
    }
}

/// The JQL, kept inside the linked Jira project; any `ORDER BY` stays last.
pub(super) fn jira_scoped(jql: String, link: Option<&ProjectLink>) -> String {
    let Some(project) = link.and_then(|link| link.workspace.as_ref()) else {
        return jql;
    };
    let key = project.key.as_deref().unwrap_or(&project.id);
    let clause = format!("project = \"{}\"", key.replace('"', "\\\""));
    let at = jql.to_uppercase().find("ORDER BY");
    let (head, tail) = match at {
        Some(index) => (jql[..index].trim(), Some(jql[index..].trim())),
        None => (jql.trim(), None),
    };
    let scoped = if head.is_empty() {
        clause
    } else {
        format!("{clause} AND ({head})")
    };
    match tail {
        Some(tail) => format!("{scoped} {tail}"),
        None => scoped,
    }
}

/// A linked Peshbar project's own task list, searched by `query`, for
/// anything but a task key or the user's own tasks: those keep the global
/// search and `me/tasks`, narrowed afterwards.
pub(super) fn peshbar_project_tasks(
    api: &str,
    link: Option<&ProjectLink>,
    query: &str,
    mine_only: bool,
) -> Option<String> {
    let project = link?.project.as_ref()?;
    if mine_only || super::issue_key(query).is_some() {
        return None;
    }
    // The board view is every task at once; completed ones are dropped
    // when the answer is read.
    let mut url = format!(
        "{api}/api/projects/{}/tasks?view=board",
        super::percent_encode(&project.id)
    );
    if !query.is_empty() {
        url.push_str(&format!(
            "&filter%5Bsearch%5D={}",
            super::percent_encode(query)
        ));
    }
    Some(url)
}

/// Peshbar runs the whole request in the linked workspace.
pub(super) fn peshbar_headers(link: Option<&ProjectLink>) -> Vec<(String, String)> {
    link.and_then(|link| link.workspace.as_ref())
        .map(|workspace| vec![(WORKSPACE_HEADER.to_string(), workspace.id.clone())])
        .unwrap_or_default()
}

/// What the search found, kept to the linked project where the tracker could
/// not filter by it: a Peshbar search spans the workspace, so its tasks are
/// matched on their key prefix.
pub fn narrow(kind: TrackerKind, issues: Vec<Issue>, link: Option<&ProjectLink>) -> Vec<Issue> {
    let prefix = match (kind, link.and_then(|link| link.project.as_ref())) {
        (TrackerKind::Peshbar, Some(project)) => project
            .key
            .as_deref()
            .map(|key| format!("{}-", key.to_uppercase())),
        _ => None,
    };
    match prefix {
        Some(prefix) => issues
            .into_iter()
            .filter(|issue| issue.key.to_uppercase().starts_with(&prefix))
            .collect(),
        None => issues,
    }
}

// ------------------------------------------------------------- storage --

/// Each project's link, by project id. Kept beside `integrations.json` rather
/// than in it, so the file older builds read keeps its shape; it holds no
/// secrets.
pub struct ProjectLinks {
    file: PathBuf,
    links: BTreeMap<String, ProjectLink>,
}

impl ProjectLinks {
    pub fn load(file: impl AsRef<Path>) -> Result<Self> {
        let file = file.as_ref().to_path_buf();
        let links = match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(ProjectLinks { file, links })
    }

    pub fn get(&self, project_id: &str) -> Option<&ProjectLink> {
        self.links.get(project_id)
    }

    /// Links a project, or with `None` unlinks it. A project only narrows
    /// to a level of the one above it, so a project without a workspace is
    /// refused.
    pub fn set(&mut self, project_id: &str, link: Option<ProjectLink>) -> Result<()> {
        match link {
            Some(link) => {
                ensure!(!link.connection_id.is_empty(), "Pick a connection.");
                ensure!(
                    link.project.is_none() || link.workspace.is_some(),
                    "Pick the workspace before a project in it."
                );
                self.links.insert(project_id.to_string(), link);
            }
            None => {
                self.links.remove(project_id);
            }
        }
        self.write()
    }

    /// Drops every link to a connection that was removed.
    pub fn forget_connection(&mut self, connection_id: &str) -> Result<()> {
        let before = self.links.len();
        self.links
            .retain(|_, link| link.connection_id != connection_id);
        if self.links.len() == before {
            return Ok(());
        }
        self.write()
    }

    fn write(&self) -> Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = self.file.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_string_pretty(&self.links)?)?;
        std::fs::rename(&temporary, &self.file)?;
        Ok(())
    }
}
