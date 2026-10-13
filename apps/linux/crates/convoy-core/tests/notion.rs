//! Notion pages as tasks: the requests, reading pages and their text, and
//! keeping a search to the linked database.

use convoy_core::integrations::notion::{page_id, parse_content, short_id};
use convoy_core::integrations::{
    manual_issues, narrow, origin, parse_issues, parse_scopes, scopes_request, search_request_in,
    Connection, ProjectLink, Scope, TrackerAuth, TrackerKind,
};
use serde_json::{json, Value};

const PAGE: &str = "1a2b3c4d5e6f47a8b9c0d1e2f3a4b5c6";
const DATABASE: &str = "0f9e8d7c-6b5a-4938-8271-605f4e3d2c1b";

fn notion() -> Connection {
    Connection {
        id: "n1".into(),
        kind: TrackerKind::Notion,
        auth: TrackerAuth::ApiKey,
        name: "Notion".into(),
        site: None,
        username: None,
        secret: "ntn_secret".into(),
    }
}

fn linked() -> ProjectLink {
    ProjectLink {
        connection_id: "n1".into(),
        workspace: Some(Scope {
            id: DATABASE.into(),
            name: "Tasks".into(),
            key: None,
        }),
        project: None,
    }
}

fn page(id: &str, title: &str, status: &str, extra: Value) -> Value {
    let mut properties = json!({
        "Name": { "type": "title", "title": [{ "plain_text": title }] },
        "Status": { "type": "status", "status": { "name": status } },
    });
    for (name, value) in extra.as_object().unwrap() {
        properties[name] = value.clone();
    }
    json!({
        "object": "page",
        "id": id,
        "url": format!("https://www.notion.so/acme/{}-{}", title.replace(' ', "-"), id.replace('-', "")),
        "parent": { "type": "database_id", "database_id": DATABASE },
        "archived": false,
        "properties": properties,
    })
}

#[test]
fn a_notion_search_reads_a_linked_database_a_page_link_or_the_title_search() {
    let connection = notion();
    let request = search_request_in(&connection, Some(&linked()), "", true, false).unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.url,
        format!("https://api.notion.com/v1/databases/{DATABASE}/query")
    );
    assert_eq!(request.header("authorization"), Some("Bearer ntn_secret"));
    assert_eq!(request.header("notion-version"), Some("2022-06-28"));

    let request = search_request_in(&connection, Some(&linked()), "invoice", true, false).unwrap();
    assert_eq!(request.url, "https://api.notion.com/v1/search");
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["query"], "invoice");
    assert_eq!(body["filter"]["value"], "page");

    let link = format!("https://www.notion.so/acme/Fix-login-{PAGE}?pvs=4");
    let request = search_request_in(&connection, None, &link, true, false).unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.url,
        "https://api.notion.com/v1/pages/1a2b3c4d-5e6f-47a8-b9c0-d1e2f3a4b5c6"
    );
}

#[test]
fn pages_become_tasks_and_finished_ones_are_left_out() {
    let answer = json!({
        "object": "list",
        "results": [
            page(PAGE, "Fix login", "In progress", json!({
                "ID": { "type": "unique_id", "unique_id": { "prefix": "TASK", "number": 12 } },
                "Priority": { "type": "select", "select": { "name": "High" } },
                "Owner": { "type": "people", "people": [{ "name": "Amir" }] },
                "Notes": { "type": "rich_text", "rich_text": [{ "plain_text": "Only on Safari" }] },
            })),
            page("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee", "Untagged", "Not started", json!({})),
            page("bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee", "Shipped", "Done", json!({})),
        ]
    });
    let issues = parse_issues(TrackerKind::Notion, &answer.to_string(), None).unwrap();
    assert_eq!(issues.len(), 2);
    let first = &issues[0];
    assert_eq!(first.key, "TASK-12");
    assert_eq!(first.title, "Fix login");
    assert_eq!(first.status.as_deref(), Some("In progress"));
    assert_eq!(first.priority.as_deref(), Some("High"));
    assert!(first.details.contains("Owner: Amir"));
    assert!(first.details.contains("Notes: Only on Safari"));
    assert_eq!(first.parent.as_deref(), Some(DATABASE));
    // Without an ID property the key is the page's short id.
    assert_eq!(issues[1].key, "aaaaaaaa");

    let refused = json!({ "object": "error", "status": 401, "message": "API token is invalid." });
    let error = parse_issues(TrackerKind::Notion, &refused.to_string(), None).unwrap_err();
    assert_eq!(error.to_string(), "API token is invalid.");
}

#[test]
fn a_search_keeps_only_the_linked_databases_pages() {
    let mut elsewhere = page(
        "cccccccc-bbbb-4ccc-8ddd-eeeeeeeeeeee",
        "Elsewhere",
        "Not started",
        json!({}),
    );
    elsewhere["parent"] = json!({ "type": "page_id", "page_id": "x" });
    let answer =
        json!({ "results": [page(PAGE, "Fix login", "Not started", json!({})), elsewhere] });
    let issues = parse_issues(TrackerKind::Notion, &answer.to_string(), None).unwrap();
    // Linked by its dashed id, read back from pages in either form.
    let kept = narrow(TrackerKind::Notion, issues.clone(), Some(&linked()));
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].title, "Fix login");
    assert_eq!(narrow(TrackerKind::Notion, issues, None).len(), 2);
}

#[test]
fn page_ids_are_read_from_links_and_bare_ids() {
    let dashed = "1a2b3c4d-5e6f-47a8-b9c0-d1e2f3a4b5c6";
    assert_eq!(
        page_id(&format!("https://www.notion.so/acme/Fix-login-{PAGE}")).as_deref(),
        Some(dashed)
    );
    assert_eq!(
        page_id(&format!("https://www.notion.so/{dashed}?pvs=4")).as_deref(),
        Some(dashed)
    );
    assert_eq!(page_id(PAGE).as_deref(), Some(dashed));
    assert_eq!(page_id("fix the login page"), None);
    assert_eq!(short_id(dashed), "1a2b3c4d");
}

#[test]
fn a_pages_blocks_read_as_text() {
    let blocks = json!({ "results": [
        { "type": "heading_2", "heading_2": { "rich_text": [{ "plain_text": "Steps" }] } },
        { "type": "numbered_list_item", "numbered_list_item": { "rich_text": [{ "plain_text": "Open Safari" }] } },
        { "type": "numbered_list_item", "numbered_list_item": { "rich_text": [{ "plain_text": "Sign in" }] } },
        { "type": "to_do", "to_do": { "checked": true, "rich_text": [{ "plain_text": "Reproduced" }] } },
        { "type": "code", "code": { "language": "bash", "rich_text": [{ "plain_text": "npm test" }] } },
        { "type": "image", "image": {} },
    ]});
    assert_eq!(
        parse_content(&blocks.to_string()).unwrap(),
        "## Steps\n1. Open Safari\n2. Sign in\n- [x] Reproduced\n```bash\nnpm test\n```"
    );
}

#[test]
fn a_connection_lists_its_databases() {
    let request = scopes_request(&notion(), None).unwrap();
    assert_eq!(request.url, "https://api.notion.com/v1/search");
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["filter"]["value"], "database");
    assert!(scopes_request(&notion(), Some(&linked().workspace.unwrap())).is_err());

    let answer = json!({ "results": [
        { "object": "database", "id": "d2", "title": [{ "plain_text": "Sprint" }] },
        { "object": "database", "id": "d1", "title": [{ "plain_text": "Backlog" }] },
        { "object": "database", "id": "d3", "archived": true, "title": [{ "plain_text": "Old" }] },
    ]});
    let databases = parse_scopes(TrackerKind::Notion, false, &answer.to_string()).unwrap();
    let names: Vec<&str> = databases.iter().map(|scope| scope.name.as_str()).collect();
    assert_eq!(names, ["Backlog", "Sprint"]);
}

#[test]
fn pasted_notion_links_become_pages_for_an_mcp_connection() {
    let issues = manual_issues(&format!(
        "https://www.notion.so/acme/Fix-login-{PAGE}\nENG-4 Typed key"
    ));
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].key, "1a2b3c4d");
    assert!(issues[0].url.as_deref().unwrap().contains(PAGE));
    assert_eq!(issues[1].key, "ENG-4");
    assert_eq!(origin(&issues[0], &notion()), "notion.so");
}
