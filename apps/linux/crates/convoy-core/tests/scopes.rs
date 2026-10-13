//! Linking a project to a workspace and project inside a tracker, and the
//! searches that stay there.

mod common;

use common::fixture;
use convoy_core::integrations::scopes::WORKSPACE_HEADER;
use convoy_core::integrations::{
    narrow, parse_scopes, scopes_request, search_request_in, Connection, Issue, ProjectLink,
    ProjectLinks, Scope, TrackerAuth, TrackerKind,
};
use serde_json::Value;

fn connection(kind: TrackerKind) -> Connection {
    Connection {
        id: "c1".into(),
        kind,
        auth: TrackerAuth::ApiKey,
        name: kind.label().into(),
        site: (kind == TrackerKind::Jira).then(|| "acme.atlassian.net".into()),
        username: (kind == TrackerKind::Jira).then(|| "me@acme.com".into()),
        secret: "secret".into(),
    }
}

fn scope(id: &str, name: &str, key: Option<&str>) -> Scope {
    Scope {
        id: id.into(),
        name: name.into(),
        key: key.map(Into::into),
    }
}

fn link(workspace: Option<Scope>, project: Option<Scope>) -> ProjectLink {
    ProjectLink {
        connection_id: "c1".into(),
        workspace,
        project,
    }
}

fn issue(key: &str) -> Issue {
    Issue {
        key: key.into(),
        title: key.into(),
        ..Issue::default()
    }
}

#[test]
fn a_linear_search_stays_in_the_linked_team_and_project() {
    let linked = link(
        Some(scope("t1", "Mobile", Some("MOB"))),
        Some(scope("p1", "Checkout", None)),
    );
    let request = search_request_in(
        &connection(TrackerKind::Linear),
        Some(&linked),
        "crash",
        true,
        false,
    )
    .unwrap();
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    let parts = body["variables"]["filter"]["and"].as_array().unwrap();
    assert_eq!(parts[0]["title"]["containsIgnoreCase"], "crash");
    assert_eq!(parts[1]["team"]["id"]["eq"], "t1");
    assert_eq!(parts[1]["project"]["id"]["eq"], "p1");

    // Unlinked, the filter is what it always was.
    let request =
        search_request_in(&connection(TrackerKind::Linear), None, "crash", true, false).unwrap();
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    assert!(body["variables"]["filter"].get("and").is_none());
}

#[test]
fn a_jira_search_stays_in_the_linked_project_but_raw_jql_does_not() {
    let linked = link(Some(scope("10001", "Payments", Some("PAY"))), None);
    let jira = connection(TrackerKind::Jira);
    let jql = |query: &str, raw: bool| {
        let request = search_request_in(&jira, Some(&linked), query, true, raw).unwrap();
        let encoded = request
            .url
            .split("jql=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string();
        percent_decode(&encoded)
    };
    assert_eq!(
        jql("", false),
        "project = \"PAY\" AND (assignee = currentUser() AND statusCategory != Done) ORDER BY updated DESC"
    );
    assert_eq!(jql("PAY-12", false), "project = \"PAY\" AND (key = PAY-12)");
    assert_eq!(jql("status = Open", true), "status = Open");
}

#[test]
fn a_peshbar_search_runs_in_the_linked_workspace() {
    let linked = link(Some(scope("7", "Tojiron", None)), None);
    let peshbar = connection(TrackerKind::Peshbar);
    let request = search_request_in(&peshbar, Some(&linked), "invoice", false, false).unwrap();
    assert_eq!(request.header(WORKSPACE_HEADER), Some("7"));
    let request = search_request_in(&peshbar, None, "invoice", false, false).unwrap();
    assert_eq!(request.header(WORKSPACE_HEADER), None);
}

#[test]
fn a_linked_peshbar_project_lists_its_own_tasks() {
    let linked = link(
        Some(scope("7", "Peshbar", None)),
        Some(scope("3", "Amir cargo", Some("AMR"))),
    );
    let peshbar = connection(TrackerKind::Peshbar);
    let url = |query: &str, mine: bool| {
        search_request_in(&peshbar, Some(&linked), query, mine, false)
            .unwrap()
            .url
    };
    let api = "https://management-api.peshbar.com/api";
    assert_eq!(url("", false), format!("{api}/projects/3/tasks?view=board"));
    assert_eq!(
        url("invoice", false),
        format!("{api}/projects/3/tasks?view=board&filter%5Bsearch%5D=invoice")
    );
    // A key, or only my own tasks, keep the global lists and are narrowed.
    assert_eq!(url("AMR-4", false), format!("{api}/search?q=AMR-4"));
    assert_eq!(url("", true), format!("{api}/me/tasks"));
}

#[test]
fn a_peshbar_project_keeps_only_its_own_tasks() {
    let linked = link(
        Some(scope("7", "Peshbar", None)),
        Some(scope("3", "Amir cargo", Some("AMR"))),
    );
    let found = narrow(
        TrackerKind::Peshbar,
        vec![issue("AMR-1"), issue("amr-2"), issue("MOB-3")],
        Some(&linked),
    );
    let keys: Vec<&str> = found.iter().map(|issue| issue.key.as_str()).collect();
    assert_eq!(keys, ["AMR-1", "amr-2"]);
    // A workspace alone keeps everything the search found there.
    let whole = link(Some(scope("7", "Peshbar", None)), None);
    assert_eq!(
        narrow(TrackerKind::Peshbar, vec![issue("MOB-3")], Some(&whole)).len(),
        1
    );
}

#[test]
fn each_tracker_lists_its_levels() {
    let teams = scopes_request(&connection(TrackerKind::Linear), None).unwrap();
    assert!(teams.body.as_deref().unwrap().contains("teams"));
    let projects = scopes_request(
        &connection(TrackerKind::Linear),
        Some(&scope("t1", "Mobile", None)),
    )
    .unwrap();
    let body: Value = serde_json::from_str(projects.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["variables"]["id"], "t1");

    let jira = scopes_request(&connection(TrackerKind::Jira), None).unwrap();
    assert_eq!(
        jira.url,
        "https://acme.atlassian.net/rest/api/3/project/search?maxResults=100&orderBy=name"
    );
    assert!(scopes_request(
        &connection(TrackerKind::Jira),
        Some(&scope("1", "PAY", None))
    )
    .is_err());

    let workspaces = scopes_request(&connection(TrackerKind::Peshbar), None).unwrap();
    assert_eq!(workspaces.url, "https://management-api.peshbar.com/api/me");
    assert_eq!(workspaces.header(WORKSPACE_HEADER), None);
    let projects = scopes_request(
        &connection(TrackerKind::Peshbar),
        Some(&scope("7", "Tojiron", None)),
    )
    .unwrap();
    assert_eq!(
        projects.url,
        "https://management-api.peshbar.com/api/projects?per_page=100"
    );
    assert_eq!(projects.header(WORKSPACE_HEADER), Some("7"));

    let mut mcp = connection(TrackerKind::Peshbar);
    mcp.auth = TrackerAuth::Mcp;
    assert!(scopes_request(&mcp, None).is_err());
}

#[test]
fn levels_are_read_from_each_answer_by_name() {
    let teams = parse_scopes(
        TrackerKind::Linear,
        false,
        r#"{"data":{"teams":{"nodes":[{"id":"b","key":"WEB","name":"web"},{"id":"a","key":"MOB","name":"Mobile"}]}}}"#,
    )
    .unwrap();
    assert_eq!(
        teams,
        [
            scope("a", "Mobile", Some("MOB")),
            scope("b", "web", Some("WEB"))
        ]
    );
    assert!(parse_scopes(
        TrackerKind::Linear,
        false,
        r#"{"errors":[{"message":"Bad key"}]}"#
    )
    .is_err());

    let cloud = parse_scopes(
        TrackerKind::Jira,
        false,
        r#"{"values":[{"id":"10001","key":"PAY","name":"Payments"}]}"#,
    )
    .unwrap();
    let server = parse_scopes(
        TrackerKind::Jira,
        false,
        r#"[{"id":"10001","key":"PAY","name":"Payments"}]"#,
    )
    .unwrap();
    assert_eq!(cloud, server);

    let workspaces = parse_scopes(
        TrackerKind::Peshbar,
        false,
        r#"{"data":{"id":1,"name":"Me","companies":[{"id":9,"name":"Tojiron"},{"id":4,"name":"Logistics"}]}}"#,
    )
    .unwrap();
    assert_eq!(
        workspaces,
        [scope("4", "Logistics", None), scope("9", "Tojiron", None)]
    );
}

#[test]
fn a_projects_link_is_kept_and_follows_its_connection() {
    let fixture = fixture();
    let file = fixture.path().join("integration-links.json");
    let mut links = ProjectLinks::load(&file).unwrap();
    let workspace = Some(scope("7", "Peshbar", None));
    links
        .set(
            "app",
            Some(link(
                workspace.clone(),
                Some(scope("3", "Amir cargo", Some("AMR"))),
            )),
        )
        .unwrap();
    assert!(links
        .set(
            "api",
            Some(link(None, Some(scope("3", "Amir cargo", None))))
        )
        .is_err());

    let reloaded = ProjectLinks::load(&file).unwrap();
    assert_eq!(
        reloaded.get("app").unwrap().project.as_ref().unwrap().name,
        "Amir cargo"
    );

    let mut reloaded = reloaded;
    reloaded.forget_connection("c1").unwrap();
    assert!(ProjectLinks::load(&file).unwrap().get("app").is_none());
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() + 1 {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
            out.push(u8::from_str_radix(hex, 16).unwrap());
            index += 3;
        } else {
            out.push(if bytes[index] == b'+' {
                b' '
            } else {
                bytes[index]
            });
            index += 1;
        }
    }
    String::from_utf8(out).unwrap()
}
