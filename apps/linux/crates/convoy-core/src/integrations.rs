//! Linear and Jira: saved connections, the search request for each, and the
//! parsing of what comes back. Port of the macOS `IssueTrackers.swift`.
//!
//! Nothing here touches the network. [`search_request`] describes the HTTP
//! call and [`parse_issues`] reads the answer, so the front end owns the
//! transport and every rule stays testable without a tracker account.

use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerKind {
    Linear,
    Jira,
}

impl TrackerKind {
    pub fn label(self) -> &'static str {
        match self {
            TrackerKind::Linear => "Linear",
            TrackerKind::Jira => "Jira",
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
    /// Jira site, e.g. `https://acme.atlassian.net` or `https://jira.acme.com`.
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
pub fn origin(issue: &Issue, connection: &Connection) -> String {
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
    ensure!(
        connection.auth != TrackerAuth::Mcp,
        "MCP connections are read by the agent, not searched."
    );
    ensure!(
        !connection.secret.is_empty(),
        "No saved credentials for “{}”.",
        connection.name
    );
    let query = query.trim();
    match connection.kind {
        TrackerKind::Linear => {
            let secret = &connection.secret;
            // Personal keys go in raw; OAuth tokens need the Bearer scheme.
            let authorization = if secret.starts_with("lin_oauth_") {
                format!("Bearer {secret}")
            } else {
                secret.clone()
            };
            let body = json!({
                "query": LINEAR_QUERY,
                "variables": { "first": PAGE_SIZE, "filter": linear_filter(query, mine_only) },
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
            let url = format!(
                "{site}{path}?jql={}&maxResults={PAGE_SIZE}&fields=summary,description,status,priority",
                percent_encode(&if raw_jql && !query.is_empty() { query.to_string() } else { jql(query, mine_only) })
            );
            let user = connection.username.as_deref().unwrap_or("").trim();
            let authorization = if user.is_empty() {
                ensure!(
                    connection.auth == TrackerAuth::ApiKey,
                    "Enter the Jira username."
                );
                format!("Bearer {}", connection.secret) // Data Center personal access token
            } else {
                format!(
                    "Basic {}",
                    base64(format!("{user}:{}", connection.secret).as_bytes())
                )
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
                        details,
                        key,
                    })
                })
                .collect())
        }
    }
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

fn base64(bytes: &[u8]) -> String {
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
                    "Linear has no password login; use an API key."
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
        std::fs::rename(&temporary, &self.file)?;
        Ok(())
    }
}
