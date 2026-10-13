//! Linear, Jira, Peshbar and Notion: saved connections, the search request for each, and the
//! parsing of what comes back. Port of the macOS `IssueTrackers.swift`.
//!
//! Nothing here touches the network. [`search_request`] describes the HTTP
//! call and [`parse_issues`] reads the answer, so the front end owns the
//! transport and every rule stays testable without a tracker account.

use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub mod notion;
pub mod scopes;
pub mod sync;
pub use scopes::{narrow, parse_scopes, scopes_request, ProjectLink, ProjectLinks, Scope};

pub const PAGE_SIZE: usize = 50;

/// The Peshbar Management API a connection without a site talks to.
pub const PESHBAR_API: &str = "https://management-api.peshbar.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerKind {
    Linear,
    Jira,
    Peshbar,
    Notion,
}

impl TrackerKind {
    pub fn label(self) -> &'static str {
        match self {
            TrackerKind::Linear => "Linear",
            TrackerKind::Jira => "Jira",
            TrackerKind::Peshbar => "Peshbar",
            TrackerKind::Notion => "Notion",
        }
    }
}

/// How the tracker is reached. `Mcp` stores no secret and makes no request:
/// the agent fetches each issue itself through its own MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackerAuth {
    ApiKey,
    Password,
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub kind: TrackerKind,
    pub auth: TrackerAuth,
    pub name: String,
    /// Jira site, e.g. `https://acme.atlassian.net` or `https://jira.acme.com`;
    /// for Peshbar the API, [`PESHBAR_API`] when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
    /// Jira Cloud email or Server username. Empty with a token means a bearer PAT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret: String,
}

impl Connection {
    /// The site with a scheme and without a trailing slash.
    pub fn site_url(&self) -> Option<String> {
        let raw = self.site.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        let mut url = if raw.contains("://") {
            raw.to_string()
        } else {
            format!("https://{raw}")
        };
        while url.ends_with('/') {
            url.pop();
        }
        Some(url)
    }

    /// The Peshbar API this connection talks to: its site, or [`PESHBAR_API`].
    pub fn peshbar_api(&self) -> String {
        self.site_url().unwrap_or_else(|| PESHBAR_API.to_string())
    }

    /// Atlassian Cloud dropped the v2 search; Server and Data Center only have it.
    pub fn is_jira_cloud(&self) -> bool {
        self.site_url()
            .and_then(|url| {
                url.split("://")
                    .nth(1)
                    .map(|rest| rest.split(['/', ':']).next().unwrap_or("").to_string())
            })
            .is_some_and(|host| host.ends_with(".atlassian.net"))
    }
}

/// Where an imported task came from: dedupe, the prompt and the link back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueSource {
    pub tracker: TrackerKind,
    pub key: String,
    /// The site or workspace the key belongs to (`acme.atlassian.net`,
    /// `linear.app/acme`): the same key on two sites is two issues.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "viaMCP", default, skip_serializing_if = "Option::is_none")]
    pub via_mcp: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Issue {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    /// Filled by [`with_origins`]; see [`IssueSource::origin`].
    #[serde(default)]
    pub origin: Option<String>,
    /// The Notion database a page sits in, for [`narrow`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// `host[/path]` of a link, where the path is the part that names the site:
/// the Linear workspace slug, or a Jira context path before `/browse/`.
fn url_origin(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    if host.is_empty() {
        return None;
    }
    if host == "linear.app" {
        let workspace = path.split('/').next().filter(|slug| !slug.is_empty())?;
        return Some(format!("{host}/{}", workspace.to_lowercase()));
    }
    let path = path
        .split("/browse/")
        .next()
        .unwrap_or("")
        .trim_matches('/');
    Some(if path.is_empty() || path.starts_with("browse") {
        host
    } else {
        format!("{host}/{path}")
    })
}

/// Which site an issue lives on: from its link, else the connection's Jira
/// site, else the connection itself (a key typed for MCP with no link).
/// Peshbar task links carry the workspace slug and the task id rather than the
/// key, so a Peshbar issue always belongs to its connection's API.
pub fn origin(issue: &Issue, connection: &Connection) -> String {
    // Page ids are unique across Notion, so its pages share one origin.
    if connection.kind == TrackerKind::Notion {
        return "notion.so".into();
    }
    if connection.kind == TrackerKind::Peshbar {
        if let Some(origin) = url_origin(&connection.peshbar_api()) {
            return origin;
        }
    }
    issue
        .url
        .as_deref()
        .and_then(url_origin)
        .or_else(|| connection.site_url().as_deref().and_then(url_origin))
        .unwrap_or_else(|| format!("connection:{}", connection.id))
}

pub fn with_origins(mut issues: Vec<Issue>, connection: &Connection) -> Vec<Issue> {
    for issue in &mut issues {
        issue.origin = Some(origin(issue, connection));
    }
    issues
}

// ------------------------------------------------------------ requests --

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The search for `query`: free text, an issue key or link, or (Jira) JQL.
/// `raw_jql` sends the query to Jira verbatim. Empty means open issues.
pub fn search_request(
    connection: &Connection,
    query: &str,
    mine_only: bool,
    raw_jql: bool,
) -> Result<HttpRequest> {
    search_request_in(connection, None, query, mine_only, raw_jql)
}

/// [`search_request`] kept inside a project's linked workspace and project.
/// Raw JQL is still sent as written; a Peshbar project is applied to the
/// answer by [`narrow`].
pub fn search_request_in(
    connection: &Connection,
    link: Option<&ProjectLink>,
    query: &str,
    mine_only: bool,
    raw_jql: bool,
) -> Result<HttpRequest> {
    ensure!(
        connection.auth != TrackerAuth::Mcp,
        "MCP connections are read by the agent, not searched."
    );
    let authorization = authorization(connection)?;
    let query = query.trim();
    match connection.kind {
        TrackerKind::Linear => {
            let filter = scopes::linear_scoped(linear_filter(query, mine_only), link);
            let body = json!({
                "query": LINEAR_QUERY,
                "variables": { "first": PAGE_SIZE, "filter": filter },
            });
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
            let Some(site) = connection.site_url() else {
                bail!("Set the Jira site URL first.")
            };
            let path = if connection.is_jira_cloud() {
                "/rest/api/3/search/jql"
            } else {
                "/rest/api/2/search"
            };
            let jql = if raw_jql && !query.is_empty() {
                query.to_string()
            } else {
                scopes::jira_scoped(jql(query, mine_only), link)
            };
            let url = format!(
                "{site}{path}?jql={}&maxResults={PAGE_SIZE}&fields=summary,description,status,priority",
                percent_encode(&jql)
            );
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
        TrackerKind::Notion => Ok(notion::search_request(authorization, link, query)),
        TrackerKind::Peshbar => {
            // A key or text goes to the global search, which also matches
            // `MOB-42`; the search needs two characters, so an empty query
            // lists the user's own tasks instead.
            let api = connection.peshbar_api();
            let url = if let Some(url) = scopes::peshbar_project_tasks(&api, link, query, mine_only)
            {
                url
            } else if query.chars().count() < 2 && issue_key(query).is_none() {
                format!("{api}/api/me/tasks")
            } else {
                let query = issue_key(query)
                    .map(|(project, number)| format!("{project}-{number}"))
                    .unwrap_or_else(|| query.to_string());
                format!("{api}/api/search?q={}", percent_encode(&query))
            };
            let mut headers = vec![
                ("Accept".into(), "application/json".into()),
                ("Authorization".into(), authorization),
            ];
            headers.extend(scopes::peshbar_headers(link));
            Ok(HttpRequest {
                method: "GET",
                url,
                headers,
                body: None,
            })
        }
    }
}

/// The `Authorization` header a connection sends, whatever it asks for.
fn authorization(connection: &Connection) -> Result<String> {
    ensure!(
        !connection.secret.is_empty(),
        "No saved credentials for “{}”.",
        connection.name
    );
    let secret = &connection.secret;
    match connection.kind {
        // Personal keys go in raw; OAuth tokens need the Bearer scheme.
        TrackerKind::Linear if secret.starts_with("lin_oauth_") => Ok(format!("Bearer {secret}")),
        TrackerKind::Linear => Ok(secret.clone()),
        TrackerKind::Jira => {
            let user = connection.username.as_deref().unwrap_or("").trim();
            if user.is_empty() {
                ensure!(
                    connection.auth == TrackerAuth::ApiKey,
                    "Enter the Jira username."
                );
                Ok(format!("Bearer {secret}")) // Data Center personal access token
            } else {
                Ok(format!(
                    "Basic {}",
                    base64(format!("{user}:{secret}").as_bytes())
                ))
            }
        }
        TrackerKind::Peshbar | TrackerKind::Notion => Ok(format!("Bearer {secret}")),
    }
}

pub const LINEAR_QUERY: &str = "query Issues($filter: IssueFilter, $first: Int) {
  issues(filter: $filter, first: $first, orderBy: updatedAt) {
    nodes { identifier title description url priorityLabel state { name } }
  }
}";

pub fn linear_filter(query: &str, mine_only: bool) -> Value {
    if let Some((team, number)) = issue_key(query) {
        return json!({ "team": { "key": { "eq": team } }, "number": { "eq": number } });
    }
    let mut filter = serde_json::Map::new();
    if mine_only {
        filter.insert("assignee".into(), json!({ "isMe": { "eq": true } }));
    }
    filter.insert(
        "state".into(),
        json!({ "type": { "nin": ["completed", "canceled"] } }),
    );
    if !query.is_empty() {
        filter.insert("title".into(), json!({ "containsIgnoreCase": query }));
    }
    Value::Object(filter)
}

pub fn jql(query: &str, mine_only: bool) -> String {
    if looks_like_jql(query) {
        return query.to_string();
    }
    if let Some((project, number)) = issue_key(query) {
        return format!("key = {project}-{number}");
    }
    let mut clauses = Vec::new();
    if mine_only {
        clauses.push("assignee = currentUser()".to_string());
    }
    clauses.push("statusCategory != Done".to_string());
    if !query.is_empty() {
        let quoted = query.replace('\\', "\\\\").replace('"', "\\\"");
        clauses.push(format!("text ~ \"{quoted}\""));
    }
    format!("{} ORDER BY updated DESC", clauses.join(" AND "))
}

/// Symbolic operators (`project=PAY`, `created >= -7d`, `summary ~ x`), an
/// `ORDER BY`, or a function call. Word operators such as `in` and `is` also
/// occur in plain text, so those need the explicit JQL mode.
fn looks_like_jql(query: &str) -> bool {
    static JQL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)[\w\]\)\x22]\s*(!=|!~|>=|<=|=|~|<|>)\s*\S|\bORDER\s+BY\b|\b\w+\(\s*\)",
        )
        .unwrap()
    });
    JQL.is_match(query)
}

static KEY: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\b[A-Za-z][A-Za-z0-9_]*-\d+\b").unwrap());

/// `ENG-123`, or a Linear/Jira link that contains one. Free text that merely
/// mentions a key is a search, not a key.
pub fn issue_key(text: &str) -> Option<(String, u64)> {
    let text = text.trim();
    let found = KEY.find(text)?;
    if found.as_str().len() != text.len() && !text.contains("://") {
        return None;
    }
    let (prefix, number) = found.as_str().split_once('-')?;
    Some((prefix.to_uppercase(), number.parse().ok()?))
}

// ------------------------------------------------------------- answers --

pub fn parse_issues(kind: TrackerKind, body: &str, site: Option<&str>) -> Result<Vec<Issue>> {
    let json: Value = serde_json::from_str(body).map_err(|_| {
        ConvoyError::message(format!("{} sent an unexpected response.", kind.label()))
    })?;
    let text = |value: &Value| value.as_str().map(str::to_string);
    match kind {
        TrackerKind::Linear => {
            if let Some(errors) = json["errors"]
                .as_array()
                .filter(|errors| !errors.is_empty())
            {
                let messages: Vec<&str> = errors
                    .iter()
                    .filter_map(|error| error["message"].as_str())
                    .collect();
                bail!("{}", messages.join("; "));
            }
            let nodes = json["data"]["issues"]["nodes"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            Ok(nodes
                .iter()
                .filter_map(|node| {
                    Some(Issue {
                        key: text(&node["identifier"])?,
                        title: text(&node["title"])?,
                        details: text(&node["description"]).unwrap_or_default(),
                        url: text(&node["url"]),
                        status: text(&node["state"]["name"]),
                        priority: text(&node["priorityLabel"]),
                        origin: None,
                        parent: None,
                    })
                })
                .collect())
        }
        TrackerKind::Jira => {
            if let Some(messages) = json["errorMessages"].as_array().filter(|m| !m.is_empty()) {
                if json.get("issues").is_none() {
                    let messages: Vec<&str> = messages.iter().filter_map(Value::as_str).collect();
                    bail!("{}", messages.join("; "));
                }
            }
            let issues = json["issues"].as_array().cloned().unwrap_or_default();
            Ok(issues
                .iter()
                .filter_map(|issue| {
                    let key = text(&issue["key"])?;
                    let fields = issue.get("fields")?;
                    let details = match &fields["description"] {
                        Value::String(value) => value.clone(),
                        document @ Value::Object(_) => adf_text(document, 0).trim().to_string(),
                        _ => String::new(),
                    };
                    Some(Issue {
                        title: text(&fields["summary"]).unwrap_or_else(|| key.clone()),
                        url: site.map(|site| format!("{site}/browse/{key}")),
                        status: text(&fields["status"]["name"]),
                        priority: text(&fields["priority"]["name"]),
                        origin: None,
                        parent: None,
                        details,
                        key,
                    })
                })
                .collect())
        }
        TrackerKind::Notion => notion::parse_pages(&json),
        TrackerKind::Peshbar => {
            if let Some(message) = json["message"]
                .as_str()
                .filter(|_| json.get("tasks").is_none() && json.get("data").is_none())
            {
                bail!("{message}");
            }
            // `/api/search` answers `{tasks, projects}`, `/api/me/tasks` `{data}`.
            let tasks = json
                .get("tasks")
                .or_else(|| json.get("data"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(tasks
                .iter()
                .filter(|task| !task["status"]["is_completed"].as_bool().unwrap_or(false))
                .filter_map(|task| {
                    Some(Issue {
                        key: text(&task["code"])?,
                        title: text(&task["title"])?,
                        details: html_text(task["description"].as_str().unwrap_or("")),
                        url: text(&task["url"]),
                        status: text(&task["status"]["name"]),
                        priority: text(&task["priority"]["label"]),
                        origin: None,
                        parent: None,
                    })
                })
                .collect())
        }
    }
}

/// Plain text from the HTML Peshbar stores descriptions in: block ends become
/// line breaks, tags go, the common entities are decoded.
pub fn html_text(html: &str) -> String {
    static BLOCK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)<br\s*/?>|</(p|div|li|h[1-6]|blockquote|pre)>").unwrap()
    });
    static ITEM: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)<li[^>]*>").unwrap());
    static TAG: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"<[^>]*>").unwrap());
    static BLANK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());
    let text = BLOCK.replace_all(html, "\n");
    let text = ITEM.replace_all(&text, "- ");
    let text = TAG.replace_all(&text, "");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    BLANK.replace_all(text.trim(), "\n\n").into_owned()
}

/// What a failed request tells the user. The body is read first because bad
/// JQL and GraphQL validation both arrive as error statuses with a message.
pub fn http_error(connection: &Connection, status: u16, body: &str) -> ConvoyError {
    let kind = connection.kind.label();
    if status == 401 || status == 403 {
        return ConvoyError::message(format!(
            "{kind} rejected the credentials (HTTP {status}). Check the saved key or login."
        ));
    }
    match parse_issues(connection.kind, body, None) {
        Err(ConvoyError::Message(message)) if !message.ends_with("unexpected response.") => {
            ConvoyError::message(message)
        }
        _ => ConvoyError::message(format!("{kind} returned HTTP {status}.")),
    }
}

/// Plain text from Atlassian Document Format, which Jira Cloud's v3 API uses
/// for descriptions.
pub fn adf_text(node: &Value, depth: usize) -> String {
    let attrs = &node["attrs"];
    let children = node["content"].as_array().cloned().unwrap_or_default();
    let inner = |depth| {
        children
            .iter()
            .map(|child| adf_text(child, depth))
            .collect::<String>()
    };
    let attr = |name: &str| attrs[name].as_str().unwrap_or("").to_string();
    match node["type"].as_str().unwrap_or("") {
        "text" => node["text"].as_str().unwrap_or("").to_string(),
        "hardBreak" => "\n".into(),
        "mention" | "emoji" => attr("text"),
        "inlineCard" | "blockCard" => attr("url"),
        "paragraph" => format!("{}\n\n", inner(depth)),
        "heading" => {
            let level = attrs["level"].as_u64().unwrap_or(2) as usize;
            format!("{} {}\n\n", "#".repeat(level), inner(depth))
        }
        "codeBlock" => format!("```\n{}\n```\n\n", inner(depth)),
        "rule" => "---\n\n".into(),
        kind @ ("bulletList" | "orderedList") => {
            let items: Vec<String> = children
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let marker = if kind == "orderedList" {
                        format!("{}. ", index + 1)
                    } else {
                        "- ".into()
                    };
                    format!(
                        "{}{marker}{}",
                        "  ".repeat(depth),
                        adf_text(item, depth + 1).trim()
                    )
                })
                .collect();
            format!("{}\n\n", items.join("\n"))
        }
        "listItem" => inner(depth).replace("\n\n", "\n"),
        _ => inner(depth),
    }
}

/// Keys or links typed for an MCP connection, one per line or comma separated.
/// Text after a key on the same line becomes the title.
pub fn manual_issues(text: &str) -> Vec<Issue> {
    let mut seen = std::collections::HashSet::new();
    text.split(['\n', ','])
        .filter_map(|raw| {
            let line = raw.trim();
            // A Notion link names a page, not an issue key.
            if let (Some(id), true) = (notion::page_id(line), line.contains("notion.")) {
                let key = notion::short_id(&id);
                return seen.insert(key.clone()).then(|| Issue {
                    title: key.clone(),
                    url: Some(line.to_string()),
                    key,
                    ..Issue::default()
                });
            }
            let found = KEY.find(line)?;
            let key = found.as_str().to_uppercase();
            if !seen.insert(key.clone()) {
                return None;
            }
            let is_link = line.contains("://");
            let rest = if is_link {
                ""
            } else {
                line[found.end()..]
                    .trim_matches(|c: char| c.is_whitespace() || c == ':' || c == '-' || c == '—')
            };
            Some(Issue {
                title: if rest.is_empty() {
                    key.clone()
                } else {
                    rest.to_string()
                },
                url: is_link.then(|| line.to_string()),
                key,
                ..Issue::default()
            })
        })
        .collect()
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Standard base64, for the few places that need it without a dependency.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ------------------------------------------------------------- storage --

/// Connections live beside `workspace.json`, not in it: the workspace is shared
/// with the other builds and validated field by field, and secrets have no
/// business in a file that gets copied around. Written owner-only.
pub struct Integrations {
    file: PathBuf,
    connections: Vec<Connection>,
}

impl Integrations {
    pub fn load(file: impl AsRef<Path>) -> Result<Self> {
        let file = file.as_ref().to_path_buf();
        let connections = match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Integrations { file, connections })
    }

    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    pub fn get(&self, id: &str) -> Result<&Connection> {
        self.connections
            .iter()
            .find(|connection| connection.id == id)
            .ok_or_else(|| ConvoyError::message("Connection not found."))
    }

    /// Saves a connection. An empty secret keeps the stored one when editing;
    /// switching to MCP drops it.
    pub fn save(&mut self, mut connection: Connection) -> Result<&Connection> {
        ensure!(
            !connection.name.trim().is_empty(),
            "Give the connection a name."
        );
        let existing = self
            .connections
            .iter()
            .position(|item| item.id == connection.id);
        if connection.auth == TrackerAuth::Mcp {
            connection.secret.clear();
        } else {
            if connection.secret.is_empty() {
                if let Some(index) = existing {
                    connection.secret = self.connections[index].secret.clone();
                }
            }
            ensure!(
                !connection.secret.is_empty(),
                "Enter the key, token or password."
            );
            if connection.kind == TrackerKind::Jira {
                ensure!(
                    connection.site_url().is_some(),
                    "Set the Jira site URL first."
                );
                let user = connection.username.as_deref().unwrap_or("").trim();
                ensure!(
                    connection.auth != TrackerAuth::Password || !user.is_empty(),
                    "Enter the Jira username."
                );
            } else {
                ensure!(
                    connection.auth != TrackerAuth::Password,
                    "{} has no password login; use an API key.",
                    connection.kind.label()
                );
            }
        }
        if connection.id.is_empty() {
            connection.id = crate::workspace::new_id();
        }
        let index = match existing {
            Some(index) => {
                self.connections[index] = connection;
                index
            }
            None => {
                self.connections.push(connection);
                self.connections.len() - 1
            }
        };
        self.write()?;
        Ok(&self.connections[index])
    }

    /// Adds a connection brought over from another app, keeping any that is
    /// already here. Unlike [`Integrations::save`] it accepts one without its
    /// secret: the user enters the key when first using it.
    pub fn import(&mut self, connection: Connection) -> Result<bool> {
        if connection.id.is_empty() || self.connections.iter().any(|item| item.id == connection.id)
        {
            return Ok(false);
        }
        self.connections.push(connection);
        self.write()?;
        Ok(true)
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.connections.retain(|connection| connection.id != id);
        self.write()
    }

    fn write(&self) -> Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = self.file.with_extension("json.tmp");
        let _ = std::fs::remove_file(&temporary);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        // Owner-only from the first byte, so the secrets are never readable
        // by anyone else even for a moment.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut handle = options.open(&temporary)?;
        std::io::Write::write_all(
            &mut handle,
            serde_json::to_string_pretty(&self.connections)?.as_bytes(),
        )?;
        handle.sync_all()?;
        drop(handle);
        std::fs::rename(&temporary, &self.file)?;
        Ok(())
    }
}
