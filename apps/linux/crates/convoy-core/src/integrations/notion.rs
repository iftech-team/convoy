//! Notion: its pages as tasks.
//!
//! A Notion connection is an internal integration's token; it sees the pages
//! and databases shared with that integration. A project links to one
//! database, whose rows are the tasks. Pages have no issue key, so a task's
//! key is the database's ID property (`TASK-12`) where it has one, and the
//! page's short id otherwise. A page's text sits in its blocks, read once a
//! page is imported ([`content_request`]); the search shows its properties.

use super::{Connection, HttpRequest, Issue, ProjectLink};
use crate::{bail, Result};
use serde_json::{json, Value};

pub const API: &str = "https://api.notion.com/v1";
/// The API version every request names; Notion keeps old ones working.
pub const VERSION: &str = "2022-06-28";

pub(super) fn headers(authorization: String) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), authorization),
        ("Notion-Version".into(), VERSION.into()),
        ("Content-Type".into(), "application/json".into()),
    ]
}

/// A page link or id typed into the search reads that page; a linked
/// database with nothing typed lists its rows; anything else is Notion's
/// title search, kept to the linked database by [`super::narrow`].
pub(super) fn search_request(
    authorization: String,
    link: Option<&ProjectLink>,
    query: &str,
) -> HttpRequest {
    let headers = headers(authorization);
    if let Some(id) = page_id(query) {
        return HttpRequest {
            method: "GET",
            url: format!("{API}/pages/{id}"),
            headers,
            body: None,
        };
    }
    let newest = json!({ "timestamp": "last_edited_time", "direction": "descending" });
    if let (Some(database), true) = (
        link.and_then(|link| link.workspace.as_ref()),
        query.is_empty(),
    ) {
        return HttpRequest {
            method: "POST",
            url: format!("{API}/databases/{}/query", database.id),
            headers,
            body: Some(json!({ "page_size": 100, "sorts": [newest] }).to_string()),
        };
    }
    HttpRequest {
        method: "POST",
        url: format!("{API}/search"),
        headers,
        body: Some(
            json!({
                "query": query,
                "filter": { "property": "object", "value": "page" },
                "sort": newest,
                "page_size": 100,
            })
            .to_string(),
        ),
    }
}

/// The databases the integration can see.
pub(super) fn databases_request(authorization: String) -> HttpRequest {
    HttpRequest {
        method: "POST",
        url: format!("{API}/search"),
        headers: headers(authorization),
        body: Some(
            json!({ "filter": { "property": "object", "value": "database" }, "page_size": 100 })
                .to_string(),
        ),
    }
}

/// A page's own text: the blocks directly inside it.
pub fn content_request(connection: &Connection, page: &str) -> Result<HttpRequest> {
    let Some(id) = page_id(page) else {
        bail!("Not a Notion page: {page}")
    };
    Ok(HttpRequest {
        method: "GET",
        url: format!("{API}/blocks/{id}/children?page_size=100"),
        headers: headers(super::authorization(connection)?),
        body: None,
    })
}

/// Notion's own error, when the answer is one.
pub(super) fn error(json: &Value) -> Option<String> {
    (json["object"] == "error").then(|| {
        json["message"]
            .as_str()
            .unwrap_or("Notion refused the request.")
            .to_string()
    })
}

/// Status names that mean the work is over: those pages are not offered.
const FINISHED: [&str; 8] = [
    "done",
    "complete",
    "completed",
    "closed",
    "canceled",
    "cancelled",
    "archived",
    "готово",
];

pub(super) fn parse_pages(json: &Value) -> Result<Vec<Issue>> {
    if let Some(message) = error(json) {
        bail!("{message}");
    }
    let pages = match json["object"].as_str() {
        Some("page") => vec![json.clone()],
        _ => json["results"].as_array().cloned().unwrap_or_default(),
    };
    Ok(pages.iter().filter_map(page).collect())
}

fn page(page: &Value) -> Option<Issue> {
    if page["object"] != "page" || page["archived"] == true || page["in_trash"] == true {
        return None;
    }
    let id = page["id"].as_str()?;
    let properties = page["properties"].as_object()?;
    let title = properties
        .values()
        .find(|property| property["type"] == "title")
        .map(|property| plain(&property["title"]))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Untitled".into());
    let named = |names: &[&str], kinds: &[&str]| {
        properties.iter().find_map(|(name, property)| {
            let kind = property["type"].as_str()?;
            (kinds.contains(&kind)
                && (names.is_empty() || names.iter().any(|n| n.eq_ignore_ascii_case(name))))
            .then_some(property)
        })
    };
    let choice = |property: &Value| {
        let kind = property["type"].as_str()?;
        property[kind]["name"].as_str().map(str::to_string)
    };
    // A status property, else a select called Status.
    let status = named(&[], &["status"])
        .or_else(|| named(&["Status", "State"], &["select"]))
        .and_then(choice);
    if status
        .as_deref()
        .is_some_and(|status| FINISHED.contains(&status.to_lowercase().as_str()))
    {
        return None;
    }
    let key = named(&[], &["unique_id"])
        .and_then(|property| {
            let number = property["unique_id"]["number"].as_u64()?;
            Some(match property["unique_id"]["prefix"].as_str() {
                Some(prefix) if !prefix.is_empty() => format!("{prefix}-{number}"),
                _ => number.to_string(),
            })
        })
        .unwrap_or_else(|| short_id(id));
    let mut details = Vec::new();
    for (name, property) in properties {
        let kind = property["type"].as_str().unwrap_or("");
        if matches!(kind, "title" | "unique_id") {
            continue;
        }
        if let Some(value) = property_text(property).filter(|value| !value.is_empty()) {
            details.push(format!("{name}: {value}"));
        }
    }
    Some(Issue {
        key,
        title,
        details: details.join("\n"),
        url: page["url"].as_str().map(str::to_string),
        status,
        priority: named(&["Priority"], &["select", "status"]).and_then(choice),
        origin: None,
        parent: page["parent"]["database_id"].as_str().map(str::to_string),
        comments: Vec::new(),
        attachments: Vec::new(),
    })
}

/// A property as one line of text; `None` for kinds that say nothing to an agent.
fn property_text(property: &Value) -> Option<String> {
    let kind = property["type"].as_str()?;
    let value = &property[kind];
    Some(match kind {
        "rich_text" => plain(value),
        "select" | "status" => value["name"].as_str()?.to_string(),
        "multi_select" => names(value),
        "people" => value
            .as_array()?
            .iter()
            .filter_map(|person| person["name"].as_str())
            .collect::<Vec<_>>()
            .join(", "),
        "date" => match value["end"].as_str() {
            Some(end) => format!("{} → {end}", value["start"].as_str()?),
            None => value["start"].as_str()?.to_string(),
        },
        "number" => value.as_f64().map(|number| number.to_string())?,
        "checkbox" => if value.as_bool()? { "yes" } else { "no" }.into(),
        "url" | "email" | "phone_number" => value.as_str()?.to_string(),
        _ => return None,
    })
}

fn names(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Rich text as plain text.
fn plain(value: &Value) -> String {
    value
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part["plain_text"].as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}

pub(super) fn parse_databases(json: &Value) -> Result<Vec<super::Scope>> {
    if let Some(message) = error(json) {
        bail!("{message}");
    }
    Ok(json["results"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|database| database["object"] == "database" && database["archived"] != true)
        .filter_map(|database| {
            Some(super::Scope {
                id: database["id"].as_str()?.to_string(),
                name: Some(plain(&database["title"]))
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| "Untitled".into()),
                key: None,
            })
        })
        .collect())
}

/// A page's blocks as Markdown-ish text: headings, lists, to-dos, quotes and
/// code keep their shape; anything without text is skipped.
pub fn parse_content(body: &str) -> Result<String> {
    let json: Value = serde_json::from_str(body)
        .map_err(|_| crate::ConvoyError::message("Notion sent an unexpected response."))?;
    if let Some(message) = error(&json) {
        bail!("{message}");
    }
    let mut lines = Vec::new();
    let mut number = 0;
    for block in json["results"].as_array().cloned().unwrap_or_default() {
        let kind = block["type"].as_str().unwrap_or("");
        let text = plain(&block[kind]["rich_text"]);
        if kind != "numbered_list_item" {
            number = 0;
        }
        let line = match kind {
            "heading_1" => format!("# {text}"),
            "heading_2" => format!("## {text}"),
            "heading_3" => format!("### {text}"),
            "bulleted_list_item" | "toggle" => format!("- {text}"),
            "numbered_list_item" => {
                number += 1;
                format!("{number}. {text}")
            }
            "to_do" => {
                let mark = if block["to_do"]["checked"] == true {
                    "x"
                } else {
                    " "
                };
                format!("- [{mark}] {text}")
            }
            "quote" | "callout" => format!("> {text}"),
            "code" => format!(
                "```{}\n{text}\n```",
                block["code"]["language"].as_str().unwrap_or("")
            ),
            "divider" => "---".into(),
            _ if !text.is_empty() => text,
            _ => continue,
        };
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

/// The page id in a Notion link or a bare id, with its dashes.
pub fn page_id(text: &str) -> Option<String> {
    static ID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b([0-9a-f]{8})-?([0-9a-f]{4})-?([0-9a-f]{4})-?([0-9a-f]{4})-?([0-9a-f]{12})\b",
        )
        .unwrap()
    });
    let text = text.trim();
    // A link names the page in its last segment, after the title; a bare id
    // is the whole text.
    let segment = if text.contains("://") {
        text.split(['?', '#']).next()?.rsplit('/').next()?
    } else {
        text
    };
    let found = ID.captures_iter(segment).last()?;
    if !text.contains("://") && found.get(0)?.as_str().len() != text.len() {
        return None;
    }
    Some(
        (1..=5)
            .map(|index| found[index].to_lowercase())
            .collect::<Vec<_>>()
            .join("-"),
    )
}

/// The first eight characters of a page id: short enough to read, and
/// unique enough within one workspace to tell tasks apart.
pub fn short_id(id: &str) -> String {
    id.replace('-', "").chars().take(8).collect()
}

/// The database a page sits in, compared without dashes.
pub(super) fn same_id(left: &str, right: &str) -> bool {
    left.replace('-', "")
        .eq_ignore_ascii_case(&right.replace('-', ""))
}
