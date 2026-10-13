//! Status sync: a task's status, and its pull request, pushed back to the
//! tracker it was imported from.
//!
//! A project's [`ProjectLink`] carries a [`StatusSync`]: which tracker status
//! each Convoy status becomes, matched by name so one mapping serves every
//! Peshbar project or Linear team in it. [`mark_changes`] runs inside every
//! workspace write and notes on the task what changed ([`SyncState::pending`]),
//! so nothing is lost when the app quits before it is pushed. The app then
//! calls [`push`] with its HTTP transport and records the outcome. Nothing
//! is ever read back: the tracker follows Convoy, never the other way.

use super::notion;
use super::scopes::WORKSPACE_HEADER;
use super::{authorization, Connection, HttpRequest, ProjectLink, TrackerAuth, TrackerKind};
use crate::workspace::model::{State, Task, TaskStatus};
use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// One of a tracker's statuses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerStatus {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<StatusGroup>,
}

/// What kind of status it is, as the tracker says; drives the suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusGroup {
    Todo,
    Started,
    Done,
    Canceled,
}

/// A project's sync settings, inside its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSync {
    pub enabled: bool,
    /// The tracker status each Convoy status becomes; one left out is never pushed.
    #[serde(default)]
    pub map: BTreeMap<TaskStatus, TrackerStatus>,
    /// Puts the pull request on the issue: a Linear attachment, a Jira remote
    /// link, a Peshbar comment, a Notion URL property or comment.
    #[serde(default = "yes")]
    pub link_pr: bool,
}

fn yes() -> bool {
    true
}

/// What a task still has to tell its tracker, and how the last push went.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingSync>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<SyncResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PendingSync {
    /// The status to push; the latest wins when several changes queue up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    /// The pull request is new and not on the issue yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResult {
    pub ok: bool,
    pub message: String,
    pub at: String,
    /// The tracker status the issue was set to, when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Whether a task can be synced at all: imported, and not through the
/// agent's own MCP server, which Convoy cannot reach.
pub fn syncable(task: &Task) -> bool {
    task.source
        .as_ref()
        .is_some_and(|source| source.via_mcp != Some(true))
}

/// Notes on each imported task what changed between `before` and `after`:
/// its status, or a pull request it did not have. Runs inside every
/// workspace write, so the note is saved with the change itself. A task
/// that has just been created has nothing to push.
pub fn mark_changes(before: &State, after: &mut State) {
    for task in &mut after.tasks {
        if !syncable(task) {
            continue;
        }
        let Some(old) = before.tasks.iter().find(|old| old.id == task.id) else {
            continue;
        };
        let status = (old.status != task.status).then_some(task.status);
        let pr = task.pr_url.is_some() && task.pr_url != old.pr_url;
        if status.is_none() && !pr {
            continue;
        }
        let state = task.sync.get_or_insert_with(SyncState::default);
        let pending = state.pending.get_or_insert_with(PendingSync::default);
        if status.is_some() {
            pending.status = status;
        }
        pending.pr |= pr;
    }
}

// ------------------------------------------------------------ statuses --

/// A transport: sends a request and returns the body of a successful answer,
/// or the tracker's error as a message.
pub type Http<'a> = dyn FnMut(&HttpRequest) -> Result<String> + 'a;

/// The statuses a mapping can choose from: the linked team, Jira project,
/// Peshbar project or Notion database's, or with only a workspace linked,
/// every status in it, once each by name.
pub fn list_statuses(
    connection: &Connection,
    link: &ProjectLink,
    http: &mut Http,
) -> Result<Vec<TrackerStatus>> {
    ensure!(
        connection.auth != TrackerAuth::Mcp,
        "An MCP connection is read by the agent; Convoy cannot set its statuses."
    );
    let authorization = authorization(connection)?;
    let mut statuses = match connection.kind {
        TrackerKind::Linear => {
            let (query, variables) = match &link.workspace {
                Some(team) => (
                    "query($id:String!){ team(id:$id){ states{ nodes{ id name type position } } } }",
                    json!({ "id": team.id }),
                ),
                None => (
                    "query{ teams(first:100){ nodes{ states{ nodes{ id name type position } } } } }",
                    json!({}),
                ),
            };
            let answer = graphql(http, &authorization, query, variables)?;
            let mut nodes: Vec<Value> = Vec::new();
            if link.workspace.is_some() {
                nodes.extend(list(&answer["team"]["states"]["nodes"]));
            } else {
                for team in list(&answer["teams"]["nodes"]) {
                    nodes.extend(list(&team["states"]["nodes"]));
                }
            }
            nodes.sort_by(|a, b| {
                let position = |node: &Value| node["position"].as_f64().unwrap_or(0.0);
                position(a).total_cmp(&position(b))
            });
            nodes
                .iter()
                .filter_map(|node| {
                    Some(TrackerStatus {
                        name: text(&node["name"])?,
                        id: text(&node["id"]),
                        group: node["type"].as_str().map(|kind| match kind {
                            "started" => StatusGroup::Started,
                            "completed" => StatusGroup::Done,
                            "canceled" => StatusGroup::Canceled,
                            _ => StatusGroup::Todo,
                        }),
                    })
                })
                .collect()
        }
        TrackerKind::Jira => {
            let (site, api) = jira(connection)?;
            let url = match &link.workspace {
                Some(project) => format!(
                    "{site}{api}/project/{}/statuses",
                    project.key.as_deref().unwrap_or(&project.id)
                ),
                None => format!("{site}{api}/status"),
            };
            let answer = json_of(&http(&get(url, jira_headers(authorization)))?)?;
            let statuses: Vec<Value> = if link.workspace.is_some() {
                list(&answer)
                    .iter()
                    .flat_map(|issue_type| list(&issue_type["statuses"]))
                    .collect()
            } else {
                list(&answer)
            };
            statuses
                .iter()
                .filter_map(|status| {
                    Some(TrackerStatus {
                        name: text(&status["name"])?,
                        id: text(&status["id"]),
                        group: status["statusCategory"]["key"]
                            .as_str()
                            .map(|key| match key {
                                "done" => StatusGroup::Done,
                                "indeterminate" => StatusGroup::Started,
                                _ => StatusGroup::Todo,
                            }),
                    })
                })
                .collect()
        }
        TrackerKind::Peshbar => {
            let api = connection.peshbar_api();
            let headers = peshbar_headers(authorization, link);
            let projects: Vec<String> = match &link.project {
                Some(project) => vec![project.id.clone()],
                None => {
                    let answer = json_of(&http(&get(
                        format!("{api}/api/projects?per_page=100"),
                        headers.clone(),
                    ))?)?;
                    list(&answer["data"])
                        .iter()
                        .filter_map(|project| text(&project["id"]))
                        .take(25)
                        .collect()
                }
            };
            let mut statuses = Vec::new();
            for project in projects {
                let answer = json_of(&http(&get(
                    format!("{api}/api/projects/{project}/statuses"),
                    headers.clone(),
                ))?)?;
                let mut found = list(answer.get("data").unwrap_or(&answer));
                found.sort_by_key(|status| status["position"].as_i64().unwrap_or(0));
                statuses.extend(found.iter().filter_map(|status| {
                    Some(TrackerStatus {
                        name: text(&status["name"])?,
                        id: text(&status["id"]),
                        group: Some(if status["is_completed"] == true {
                            StatusGroup::Done
                        } else if status["is_default"] == true {
                            StatusGroup::Todo
                        } else {
                            StatusGroup::Started
                        }),
                    })
                }));
            }
            statuses
        }
        TrackerKind::Notion => {
            let Some(database) = &link.workspace else {
                bail!("Link a Notion database first; its status property holds the choices.")
            };
            let schema = json_of(&http(&get(
                format!("{}/databases/{}", notion::API, database.id),
                notion::headers(authorization),
            ))?)?;
            let Some((_, _, property)) = notion_status_property(&schema) else {
                bail!("The database has no Status or select property to set.")
            };
            let groups: Vec<(StatusGroup, Vec<String>)> = list(&property["groups"])
                .iter()
                .map(|group| {
                    let name = group["name"].as_str().unwrap_or("").to_lowercase();
                    let kind = if name.contains("complete") || name.contains("done") {
                        StatusGroup::Done
                    } else if name.contains("progress") {
                        StatusGroup::Started
                    } else {
                        StatusGroup::Todo
                    };
                    let ids = list(&group["option_ids"]).iter().filter_map(text).collect();
                    (kind, ids)
                })
                .collect();
            list(&property["options"])
                .iter()
                .filter_map(|option| {
                    let id = text(&option["id"]);
                    Some(TrackerStatus {
                        name: text(&option["name"])?,
                        group: groups
                            .iter()
                            .find(|(_, ids)| id.as_ref().is_some_and(|id| ids.contains(id)))
                            .map(|(group, _)| *group),
                        id,
                    })
                })
                .collect()
        }
    };
    // Once each, by name: a workspace repeats its statuses per project.
    let mut seen = std::collections::HashSet::new();
    statuses.retain(|status: &TrackerStatus| seen.insert(status.name.to_lowercase()));
    Ok(statuses)
}

/// The mapping a new sync starts with: each Convoy status matched to the
/// tracker status of the same meaning, by name first and then by kind.
/// Queued and Failed are left out: a tracker has no use for them.
pub fn suggest_map(statuses: &[TrackerStatus]) -> BTreeMap<TaskStatus, TrackerStatus> {
    let wanted: [(TaskStatus, &[&str], Option<StatusGroup>); 6] = [
        (
            TaskStatus::Building,
            &[
                "in progress",
                "doing",
                "in development",
                "started",
                "в работе",
            ],
            Some(StatusGroup::Started),
        ),
        (
            TaskStatus::Review,
            &["in review", "code review", "review", "на проверке", "ревью"],
            None,
        ),
        (
            TaskStatus::Pr,
            &[
                "in review",
                "code review",
                "pull request",
                "review",
                "на проверке",
            ],
            None,
        ),
        (
            TaskStatus::Testing,
            &[
                "in testing",
                "testing",
                "ready for qa",
                "in qa",
                "qa",
                "test",
                "тест",
            ],
            None,
        ),
        (
            TaskStatus::Changes,
            &[
                "changes requested",
                "needs changes",
                "rework",
                "reopened",
                "in progress",
                "в работе",
            ],
            Some(StatusGroup::Started),
        ),
        (
            TaskStatus::Done,
            &[
                "done",
                "completed",
                "complete",
                "closed",
                "resolved",
                "готово",
            ],
            Some(StatusGroup::Done),
        ),
    ];
    let mut map = BTreeMap::new();
    for (status, names, group) in wanted {
        let lower = |status: &TrackerStatus| status.name.to_lowercase();
        let found = names
            .iter()
            .find_map(|name| statuses.iter().find(|status| lower(status) == *name))
            .or_else(|| {
                names
                    .iter()
                    .find_map(|name| statuses.iter().find(|status| lower(status).contains(name)))
            })
            .or_else(|| {
                group.and_then(|group| statuses.iter().find(|status| status.group == Some(group)))
            });
        if let Some(found) = found {
            map.insert(status, found.clone());
        }
    }
    map
}

// ---------------------------------------------------------------- push --

/// Pushes what `task` has pending to its tracker and says how it went. Only
/// mapped statuses are set; the pull request is added when the link asks
/// for it. `None` when there was nothing to do (not synced, not mapped).
pub fn push(
    connection: &Connection,
    link: &ProjectLink,
    task: &Task,
    http: &mut Http,
) -> Option<SyncResult> {
    let sync = link.sync.as_ref().filter(|sync| sync.enabled)?;
    let source = task.source.as_ref().filter(|_| syncable(task))?;
    let pending = task.sync.as_ref()?.pending.as_ref()?;
    let target = pending.status.and_then(|status| sync.map.get(&status));
    let pr = task
        .pr_url
        .as_deref()
        .filter(|_| pending.pr && sync.link_pr);
    if target.is_none() && pr.is_none() {
        return None;
    }
    let outcome = (|| -> Result<Vec<String>> {
        let authorization = authorization(connection)?;
        let mut done = Vec::new();
        let issue = Issue::resolve(
            connection,
            link,
            &source.key,
            source.url.as_deref(),
            &authorization,
            http,
        )?;
        if let Some(target) = target {
            issue.set_status(connection, &authorization, &target.name, http)?;
            done.push(format!("set to {}", target.name));
        }
        if let Some(url) = pr {
            issue.link_pr(connection, &authorization, url, http)?;
            done.push("pull request linked".into());
        }
        Ok(done)
    })();
    let at = crate::time::now_iso8601();
    Some(match outcome {
        Ok(done) => SyncResult {
            ok: true,
            message: capitalize(&done.join(" · ")),
            at,
            status: target.map(|target| target.name.clone()),
        },
        Err(error) => SyncResult {
            ok: false,
            message: error.to_string(),
            at,
            status: None,
        },
    })
}

/// An issue, found in its tracker, ready to change.
enum Issue {
    Linear {
        id: String,
        states: Vec<(String, String)>,
    },
    Jira {
        key: String,
    },
    Peshbar {
        id: String,
        project: String,
        headers: Vec<(String, String)>,
    },
    Notion {
        page: String,
    },
}

impl Issue {
    fn resolve(
        connection: &Connection,
        link: &ProjectLink,
        key: &str,
        url: Option<&str>,
        authorization: &str,
        http: &mut Http,
    ) -> Result<Issue> {
        Ok(match connection.kind {
            TrackerKind::Linear => {
                // `issue(id:)` takes the identifier as readily as the uuid.
                let answer = graphql(
                    http,
                    authorization,
                    "query($id:String!){ issue(id:$id){ id team{ states{ nodes{ id name } } } } }",
                    json!({ "id": key }),
                )?;
                let issue = &answer["issue"];
                let Some(id) = text(&issue["id"]) else {
                    bail!("Linear has no issue {key}.")
                };
                let states = list(&issue["team"]["states"]["nodes"])
                    .iter()
                    .filter_map(|state| Some((text(&state["id"])?, text(&state["name"])?)))
                    .collect();
                Issue::Linear { id, states }
            }
            TrackerKind::Jira => Issue::Jira {
                key: key.to_string(),
            },
            TrackerKind::Peshbar => {
                let api = connection.peshbar_api();
                let headers = peshbar_headers(authorization.to_string(), link);
                let answer = json_of(&http(&get(
                    format!("{api}/api/search?q={}", super::percent_encode(key)),
                    headers.clone(),
                ))?)?;
                let task = list(&answer["tasks"]).into_iter().find(|task| {
                    task["code"]
                        .as_str()
                        .is_some_and(|code| code.eq_ignore_ascii_case(key))
                });
                let Some(task) = task else {
                    bail!("Peshbar has no task {key} in this workspace.")
                };
                let (Some(id), Some(project)) = (text(&task["id"]), text(&task["project_id"]))
                else {
                    bail!("Peshbar sent an unexpected response.")
                };
                Issue::Peshbar {
                    id,
                    project,
                    headers,
                }
            }
            TrackerKind::Notion => {
                let Some(page) = url
                    .and_then(notion::page_id)
                    .or_else(|| notion::page_id(key))
                else {
                    bail!("The task has no Notion page link.")
                };
                Issue::Notion { page }
            }
        })
    }

    fn set_status(
        &self,
        connection: &Connection,
        authorization: &str,
        name: &str,
        http: &mut Http,
    ) -> Result<()> {
        match self {
            Issue::Linear { id, states } => {
                let Some((state, _)) = states
                    .iter()
                    .find(|(_, state)| state.eq_ignore_ascii_case(name))
                else {
                    bail!("The issue's Linear team has no state “{name}”.")
                };
                let answer = graphql(
                    http,
                    authorization,
                    "mutation($id:String!,$s:String!){ issueUpdate(id:$id, input:{stateId:$s}){ success } }",
                    json!({ "id": id, "s": state }),
                )?;
                ensure!(
                    answer["issueUpdate"]["success"] == true,
                    "Linear did not update the issue."
                );
            }
            Issue::Jira { key } => {
                let (site, api) = jira(connection)?;
                let url = format!("{site}{api}/issue/{key}/transitions");
                let answer = json_of(&http(&get(
                    url.clone(),
                    jira_headers(authorization.to_string()),
                ))?)?;
                let transition = list(&answer["transitions"]).into_iter().find(|transition| {
                    transition["to"]["name"]
                        .as_str()
                        .is_some_and(|to| to.eq_ignore_ascii_case(name))
                });
                let Some(transition) = transition else {
                    bail!("Jira has no transition to “{name}” from {key}'s current status in this workflow.")
                };
                http(&HttpRequest {
                    method: "POST",
                    url,
                    headers: jira_headers(authorization.to_string()),
                    body: Some(json!({ "transition": { "id": transition["id"] } }).to_string()),
                })?;
            }
            Issue::Peshbar {
                id,
                project,
                headers,
            } => {
                let api = connection.peshbar_api();
                let answer = json_of(&http(&get(
                    format!("{api}/api/projects/{project}/statuses"),
                    headers.clone(),
                ))?)?;
                let status = list(answer.get("data").unwrap_or(&answer))
                    .into_iter()
                    .find(|status| {
                        status["name"]
                            .as_str()
                            .is_some_and(|status| status.eq_ignore_ascii_case(name))
                    });
                let Some(status) = status else {
                    bail!("The task's Peshbar project has no status “{name}”.")
                };
                http(&HttpRequest {
                    method: "PUT",
                    url: format!("{api}/api/projects/{project}/tasks/{id}/status"),
                    headers: headers.clone(),
                    body: Some(json!({ "status_id": status["id"] }).to_string()),
                })?;
            }
            Issue::Notion { page } => {
                let schema = notion_schema(page, authorization, http)?;
                let Some((property, kind, details)) = notion_status_property(&schema) else {
                    bail!("The page's database has no Status or select property to set.")
                };
                let known = list(&details["options"])
                    .iter()
                    .any(|option| option["name"].as_str().is_some_and(|option| option == name));
                // An unknown select name would quietly become a new option.
                ensure!(
                    known,
                    "“{name}” is not an option of the Notion property {property}."
                );
                http(&HttpRequest {
                    method: "PATCH",
                    url: format!("{}/pages/{page}", notion::API),
                    headers: notion::headers(authorization.to_string()),
                    body: Some(
                        json!({ "properties": { property: { kind: { "name": name } } } })
                            .to_string(),
                    ),
                })?;
            }
        }
        Ok(())
    }

    fn link_pr(
        &self,
        connection: &Connection,
        authorization: &str,
        url: &str,
        http: &mut Http,
    ) -> Result<()> {
        let title = pr_title(url);
        match self {
            Issue::Linear { id, .. } => {
                let answer = graphql(
                    http,
                    authorization,
                    "mutation($id:String!,$url:String!,$title:String){ attachmentLinkURL(issueId:$id, url:$url, title:$title){ success } }",
                    json!({ "id": id, "url": url, "title": title }),
                )?;
                ensure!(
                    answer["attachmentLinkURL"]["success"] == true,
                    "Linear did not attach the pull request."
                );
            }
            Issue::Jira { key } => {
                let (site, api) = jira(connection)?;
                http(&HttpRequest {
                    method: "POST",
                    url: format!("{site}{api}/issue/{key}/remotelink"),
                    headers: jira_headers(authorization.to_string()),
                    // The same globalId updates the link rather than adding another.
                    body: Some(
                        json!({ "globalId": url, "object": { "url": url, "title": title } })
                            .to_string(),
                    ),
                })?;
            }
            Issue::Peshbar { id, headers, .. } => {
                let api = connection.peshbar_api();
                let escaped = html_escape(url);
                http(&HttpRequest {
                    method: "POST",
                    url: format!("{api}/api/tasks/{id}/comments"),
                    headers: headers.clone(),
                    body: Some(
                        json!({ "body": format!("<p>{}: <a href=\"{escaped}\">{escaped}</a></p>", html_escape(&title)) })
                            .to_string(),
                    ),
                })?;
            }
            Issue::Notion { page } => {
                let schema = notion_schema(page, authorization, http)?;
                let property = schema["properties"].as_object().and_then(|properties| {
                    properties.iter().find_map(|(name, property)| {
                        let lower = name.to_lowercase();
                        (property["type"] == "url"
                            && (lower == "pr" || lower.contains("pull request")))
                        .then(|| name.clone())
                    })
                });
                let request = match property {
                    Some(property) => HttpRequest {
                        method: "PATCH",
                        url: format!("{}/pages/{page}", notion::API),
                        headers: notion::headers(authorization.to_string()),
                        body: Some(json!({ "properties": { property: { "url": url } } }).to_string()),
                    },
                    None => HttpRequest {
                        method: "POST",
                        url: format!("{}/comments", notion::API),
                        headers: notion::headers(authorization.to_string()),
                        body: Some(
                            json!({
                                "parent": { "page_id": page },
                                "rich_text": [
                                    { "type": "text", "text": { "content": format!("{title}: ") } },
                                    { "type": "text", "text": { "content": url, "link": { "url": url } } },
                                ],
                            })
                            .to_string(),
                        ),
                    },
                };
                http(&request)?;
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------- helpers --

fn graphql(http: &mut Http, authorization: &str, query: &str, variables: Value) -> Result<Value> {
    let body = http(&HttpRequest {
        method: "POST",
        url: "https://api.linear.app/graphql".into(),
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("Authorization".into(), authorization.to_string()),
        ],
        body: Some(json!({ "query": query, "variables": variables }).to_string()),
    })?;
    let answer = json_of(&body)?;
    if let Some(errors) = answer["errors"]
        .as_array()
        .filter(|errors| !errors.is_empty())
    {
        let messages: Vec<&str> = errors
            .iter()
            .filter_map(|error| error["message"].as_str())
            .collect();
        bail!("{}", messages.join("; "));
    }
    Ok(answer["data"].clone())
}

fn jira(connection: &Connection) -> Result<(String, &'static str)> {
    let Some(site) = connection.site_url() else {
        bail!("Set the Jira site URL first.")
    };
    Ok((
        site,
        if connection.is_jira_cloud() {
            "/rest/api/3"
        } else {
            "/rest/api/2"
        },
    ))
}

fn jira_headers(authorization: String) -> Vec<(String, String)> {
    vec![
        ("Accept".into(), "application/json".into()),
        ("Content-Type".into(), "application/json".into()),
        ("Authorization".into(), authorization),
    ]
}

fn peshbar_headers(authorization: String, link: &ProjectLink) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Accept".into(), "application/json".into()),
        ("Content-Type".into(), "application/json".into()),
        ("Authorization".into(), authorization),
    ];
    if let Some(workspace) = &link.workspace {
        headers.push((WORKSPACE_HEADER.into(), workspace.id.clone()));
    }
    headers
}

/// The database a Notion page sits in, read through the page.
fn notion_schema(page: &str, authorization: &str, http: &mut Http) -> Result<Value> {
    let headers = notion::headers(authorization.to_string());
    let page = json_of(&http(&get(
        format!("{}/pages/{page}", notion::API),
        headers.clone(),
    ))?)?;
    let Some(database) = page["parent"]["database_id"].as_str() else {
        bail!("The Notion page is not in a database, so it has no status to set.")
    };
    json_of(&http(&get(
        format!("{}/databases/{database}", notion::API),
        headers,
    ))?)
}

/// The property a Notion database keeps status in: a status property, else
/// a select called Status or State. Its name, kind and settings.
fn notion_status_property(schema: &Value) -> Option<(String, String, Value)> {
    let properties = schema["properties"].as_object()?;
    let found = properties
        .iter()
        .find(|(_, property)| property["type"] == "status")
        .or_else(|| {
            properties.iter().find(|(name, property)| {
                property["type"] == "select"
                    && ["status", "state"].contains(&name.to_lowercase().as_str())
            })
        })?;
    let kind = found.1["type"].as_str()?.to_string();
    Some((found.0.clone(), kind.clone(), found.1[&kind].clone()))
}

fn get(url: String, headers: Vec<(String, String)>) -> HttpRequest {
    HttpRequest {
        method: "GET",
        url,
        headers,
        body: None,
    }
}

fn json_of(body: &str) -> Result<Value> {
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    let answer: Value = serde_json::from_str(body)
        .map_err(|_| ConvoyError::message("The tracker sent an unexpected response."))?;
    if let Some(message) = notion::error(&answer) {
        bail!("{message}");
    }
    Ok(answer)
}

fn list(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// "Pull request #12" for a GitHub or GitLab link, else "Pull request".
fn pr_title(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|number| number.chars().all(|c| c.is_ascii_digit()) && !number.is_empty())
        .map(|number| format!("Pull request #{number}"))
        .unwrap_or_else(|| "Pull request".into())
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
