//! Linear and Jira import: requests, parsing, storage and the tasks it creates.

mod common;

use common::fixture;
use convoy_core::integrations::{
    http_error, issue_key, jql, linear_filter, manual_issues, origin, parse_issues, search_request,
    with_origins, Connection, Integrations, Issue, TrackerAuth, TrackerKind,
};
use convoy_core::planning::ImportOptions;
use convoy_core::workspace::model::{Agent, PublishMode, TaskStatus};
use convoy_core::workspace::Workspace;
use serde_json::Value;
use std::collections::BTreeMap;

fn connection(kind: TrackerKind, auth: TrackerAuth) -> Connection {
    Connection {
        id: "c1".into(),
        kind,
        auth,
        name: kind.label().into(),
        site: None,
        username: None,
        secret: "secret".into(),
    }
}

#[test]
fn linear_request_sends_the_raw_key_and_an_open_assigned_filter() {
    let mut linear = connection(TrackerKind::Linear, TrackerAuth::ApiKey);
    linear.secret = "lin_api_x".into();
    let request = search_request(&linear, "", true, false).unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://api.linear.app/graphql");
    assert_eq!(request.header("authorization"), Some("lin_api_x"));
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    let filter = &body["variables"]["filter"];
    assert!(filter.get("assignee").is_some());
    assert!(filter.get("title").is_none());

    let by_key = linear_filter("eng-42", true);
    assert_eq!(by_key["number"]["eq"], 42);
    assert_eq!(by_key["team"]["key"]["eq"], "ENG");
    assert!(by_key.get("assignee").is_none());
    assert_eq!(
        linear_filter("https://linear.app/acme/issue/ENG-7/fix", false)["number"]["eq"],
        7
    );
    assert!(linear_filter("fix eng-7 crash", false)
        .get("title")
        .is_some());
    assert_eq!(issue_key("fix ENG-7"), None);
}

#[test]
fn jira_cloud_server_and_token_requests() {
    let mut cloud = connection(TrackerKind::Jira, TrackerAuth::ApiKey);
    cloud.site = Some("acme.atlassian.net/".into());
    cloud.username = Some("me@acme.com".into());
    cloud.secret = "tok".into();
    let request = search_request(&cloud, "", true, false).unwrap();
    assert!(request.url.starts_with(
        "https://acme.atlassian.net/rest/api/3/search/jql?jql=assignee%20%3D%20currentUser%28%29"
    ));
    // base64("me@acme.com:tok")
    assert_eq!(
        request.header("Authorization"),
        Some("Basic bWVAYWNtZS5jb206dG9r")
    );

    let mut server = connection(TrackerKind::Jira, TrackerAuth::Password);
    server.site = Some("https://jira.acme.com".into());
    server.username = Some("bob".into());
    let request = search_request(&server, "PAY-12", true, false).unwrap();
    assert!(request
        .url
        .starts_with("https://jira.acme.com/rest/api/2/search?jql=key%20%3D%20PAY-12"));

    let mut pat = connection(TrackerKind::Jira, TrackerAuth::ApiKey);
    pat.site = Some("https://jira.acme.com".into());
    assert_eq!(
        search_request(&pat, "", false, false)
            .unwrap()
            .header("Authorization"),
        Some("Bearer secret")
    );

    let no_site = connection(TrackerKind::Jira, TrackerAuth::ApiKey);
    assert!(search_request(&no_site, "", true, false).is_err());
    assert!(search_request(
        &connection(TrackerKind::Linear, TrackerAuth::Mcp),
        "",
        true,
        false
    )
    .is_err());

    assert_eq!(
        jql("project = PAY ORDER BY created", true),
        "project = PAY ORDER BY created"
    );
    // Operators without spaces and comparisons are JQL too, not text.
    for query in [
        "project=PAY",
        "priority>=High",
        "created >= -7d",
        "summary~crash",
        "status!=Done",
        "assignee=currentUser()",
    ] {
        assert_eq!(jql(query, true), query, "{query}");
    }
    // Word operators are ambiguous in plain text; the explicit mode sends them verbatim.
    assert!(jql("crash in login", true).contains("text ~ \"crash in login\""));
    let explicit = search_request(&server, "project in (PAY, OPS)", true, true).unwrap();
    assert!(explicit
        .url
        .contains("jql=project%20in%20%28PAY%2C%20OPS%29&"));
    assert_eq!(
        jql("say \"hi\"", false),
        "statusCategory != Done AND text ~ \"say \\\"hi\\\"\" ORDER BY updated DESC"
    );
}

#[test]
fn parses_linear_answers_and_errors() {
    let body = r#"{"data":{"issues":{"nodes":[{"identifier":"ENG-1","title":"Fix login","description":"Steps","url":"https://linear.app/a/issue/ENG-1","priorityLabel":"High","state":{"name":"Todo"}}]}}}"#;
    let issues = parse_issues(TrackerKind::Linear, body, None).unwrap();
    assert_eq!(
        issues,
        vec![Issue {
            key: "ENG-1".into(),
            title: "Fix login".into(),
            details: "Steps".into(),
            url: Some("https://linear.app/a/issue/ENG-1".into()),
            status: Some("Todo".into()),
            priority: Some("High".into()),
            origin: None,
        }]
    );
    let error = parse_issues(
        TrackerKind::Linear,
        r#"{"errors":[{"message":"Bad filter"}]}"#,
        None,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Bad filter");
    let linear = connection(TrackerKind::Linear, TrackerAuth::ApiKey);
    assert!(http_error(&linear, 401, "")
        .to_string()
        .contains("rejected the credentials"));
    assert_eq!(
        http_error(&linear, 400, r#"{"errors":[{"message":"Bad filter"}]}"#).to_string(),
        "Bad filter"
    );
    assert_eq!(
        http_error(&linear, 502, "<html>").to_string(),
        "Linear returned HTTP 502."
    );
}

#[test]
fn parses_jira_document_and_plain_descriptions() {
    let body = r#"{"issues":[
      {"key":"PAY-3","fields":{"summary":"Refunds","status":{"name":"In Progress"},"priority":{"name":"P1"},
        "description":{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Handle "},{"type":"text","text":"partial"},{"type":"text","text":" refunds."}]},
          {"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},
                                          {"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}}},
      {"key":"PAY-4","fields":{"summary":"Old","description":"plain text"}}]}"#;
    let issues = parse_issues(TrackerKind::Jira, body, Some("https://acme.atlassian.net")).unwrap();
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].details, "Handle partial refunds.\n\n- one\n- two");
    assert_eq!(
        issues[0].url.as_deref(),
        Some("https://acme.atlassian.net/browse/PAY-3")
    );
    assert_eq!(issues[0].status.as_deref(), Some("In Progress"));
    assert_eq!(issues[1].details, "plain text");
    assert_eq!(
        parse_issues(TrackerKind::Jira, r#"{"errorMessages":["Bad JQL"]}"#, None)
            .unwrap_err()
            .to_string(),
        "Bad JQL"
    );
}

#[test]
fn manual_keys_for_mcp_connections() {
    let issues =
        manual_issues("eng-1 Fix login\nhttps://linear.app/a/issue/ENG-2/slug, ENG-1\nnot a key");
    let keys: Vec<&str> = issues.iter().map(|issue| issue.key.as_str()).collect();
    assert_eq!(keys, ["ENG-1", "ENG-2"]);
    assert_eq!(issues[0].title, "Fix login");
    assert_eq!(issues[1].title, "ENG-2");
    assert_eq!(
        issues[1].url.as_deref(),
        Some("https://linear.app/a/issue/ENG-2/slug")
    );
}

#[test]
fn connections_keep_secrets_owner_only_and_across_edits() {
    let fixture = fixture();
    let file = fixture.path().join("integrations.json");
    let mut integrations = Integrations::load(&file).unwrap();
    let mut jira = connection(TrackerKind::Jira, TrackerAuth::Password);
    jira.id = String::new();
    assert!(
        integrations.save(jira.clone()).is_err(),
        "a site is required"
    );
    jira.site = Some("jira.acme.com".into());
    jira.username = Some("bob".into());
    let id = integrations.save(jira).unwrap().id.clone();
    assert!(!id.is_empty());

    // Editing without retyping the password keeps it.
    let mut edited = integrations.get(&id).unwrap().clone();
    edited.name = "Work".into();
    edited.secret.clear();
    integrations.save(edited).unwrap();
    let reloaded = Integrations::load(&file).unwrap();
    assert_eq!(reloaded.get(&id).unwrap().name, "Work");
    assert_eq!(reloaded.get(&id).unwrap().secret, "secret");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let mut mcp = reloaded.get(&id).unwrap().clone();
    mcp.auth = TrackerAuth::Mcp;
    integrations.save(mcp).unwrap();
    assert!(Integrations::load(&file)
        .unwrap()
        .get(&id)
        .unwrap()
        .secret
        .is_empty());
    assert!(!std::fs::read_to_string(&file).unwrap().contains("secret"));

    integrations.remove(&id).unwrap();
    assert!(Integrations::load(&file).unwrap().connections().is_empty());
}

#[test]
fn import_creates_tasks_with_per_issue_models_and_skips_duplicates() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let linear = connection(TrackerKind::Linear, TrackerAuth::ApiKey);
    let issues = vec![
        Issue {
            key: "ENG-1".into(),
            title: "Fix login".into(),
            details: "Steps".into(),
            url: Some("https://linear.app/x".into()),
            ..Issue::default()
        },
        Issue {
            key: "ENG-2".into(),
            title: "Add export".into(),
            ..Issue::default()
        },
    ];
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "ENG-2".to_string(),
        (Agent::Codex, "gpt-5-codex".to_string()),
    );
    let options = ImportOptions {
        agent: Agent::Claude,
        model: "sonnet".into(),
        mode: PublishMode::Push,
        auto_review: false,
        overrides,
    };

    assert_eq!(
        workspace
            .import_issues(&project, &linear, &issues, &options)
            .unwrap(),
        (2, 0)
    );
    let tasks = &workspace.state().tasks;
    let one = tasks
        .iter()
        .find(|task| task.source.as_ref().unwrap().key == "ENG-1")
        .unwrap();
    assert_eq!(one.title, "ENG-1: Fix login");
    assert_eq!(
        (one.agent, one.model.as_deref(), one.mode, one.status),
        (
            Agent::Claude,
            Some("sonnet"),
            PublishMode::Push,
            TaskStatus::Queued
        )
    );
    let two = tasks
        .iter()
        .find(|task| task.source.as_ref().unwrap().key == "ENG-2")
        .unwrap();
    assert_eq!(
        (two.agent, two.model.as_deref()),
        (Agent::Codex, Some("gpt-5-codex"))
    );

    assert_eq!(
        workspace
            .import_issues(&project, &linear, &issues, &options)
            .unwrap(),
        (0, 2)
    );
    assert_eq!(workspace.state().tasks.len(), 2);

    let bad = ImportOptions {
        model: "no spaces allowed".into(),
        ..options.clone()
    };
    assert!(workspace
        .import_issues(&project, &linear, &issues, &bad)
        .is_err());

    // The saved document still loads, keeps the source and passes validation.
    let reloaded = Workspace::load(&fixture.file).unwrap();
    let saved = reloaded
        .state()
        .tasks
        .iter()
        .find(|task| task.title == "ENG-1: Fix login")
        .unwrap();
    assert_eq!(
        saved.source.as_ref().unwrap().url.as_deref(),
        Some("https://linear.app/x")
    );

    // The brief names the issue, and the session runs with the task's model.
    let id = saved.id.clone();
    workspace.prepare_task(&id, None).unwrap();
    let task = workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap();
    let session = workspace
        .state()
        .sessions
        .iter()
        .find(|session| Some(&session.id) == task.session_id.as_ref())
        .unwrap();
    assert!(session
        .prompt
        .contains("Source issue: Linear ENG-1 — https://linear.app/x"));
    assert!(!session.prompt.contains("MCP"));
    assert_eq!(session.model.as_deref(), Some("sonnet"));

    // Changing the model detaches the prepared session.
    workspace.set_task_agent(&id, Agent::Codex, "o3").unwrap();
    let task = workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap();
    assert_eq!(
        (
            task.agent,
            task.model.as_deref(),
            task.session_id.as_deref()
        ),
        (Agent::Codex, Some("o3"), None)
    );

    let mcp = connection(TrackerKind::Jira, TrackerAuth::Mcp);
    workspace
        .import_issues(&project, &mcp, &manual_issues("PAY-9"), &options)
        .unwrap();
    let id = workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.title == "PAY-9")
        .unwrap()
        .id
        .clone();
    workspace.prepare_task(&id, None).unwrap();
    let prompt = &workspace.state().sessions.last().unwrap().prompt;
    assert!(prompt.contains("with your Jira MCP tools"));
}

#[test]
fn the_same_key_on_two_sites_is_two_issues() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let mut first = connection(TrackerKind::Jira, TrackerAuth::ApiKey);
    first.site = Some("https://one.atlassian.net".into());
    let mut second = first.clone();
    second.id = "c2".into();
    second.site = Some("https://two.atlassian.net".into());
    let body = r#"{"issues":[{"key":"PROJ-1","fields":{"summary":"S"}}]}"#;
    let from_one = with_origins(
        parse_issues(TrackerKind::Jira, body, first.site_url().as_deref()).unwrap(),
        &first,
    );
    let from_two = with_origins(
        parse_issues(TrackerKind::Jira, body, second.site_url().as_deref()).unwrap(),
        &second,
    );
    assert_eq!(from_one[0].origin.as_deref(), Some("one.atlassian.net"));
    let options = ImportOptions {
        agent: Agent::Claude,
        model: String::new(),
        mode: PublishMode::Pr,
        auto_review: false,
        overrides: BTreeMap::new(),
    };
    assert_eq!(
        workspace
            .import_issues(&project, &first, &from_one, &options)
            .unwrap(),
        (1, 0)
    );
    assert_eq!(
        workspace
            .import_issues(&project, &second, &from_two, &options)
            .unwrap(),
        (1, 0)
    );
    assert_eq!(
        workspace
            .import_issues(&project, &first, &from_one, &options)
            .unwrap(),
        (0, 1)
    );

    // Links name the site: Linear by workspace slug, Jira by host and context path.
    let linear = connection(TrackerKind::Linear, TrackerAuth::Mcp);
    let pasted = manual_issues("https://linear.app/Acme/issue/ENG-2/slug\nENG-3");
    assert_eq!(origin(&pasted[0], &linear), "linear.app/acme");
    assert_eq!(origin(&pasted[1], &linear), "connection:c1");
    let mut hosted = connection(TrackerKind::Jira, TrackerAuth::Mcp);
    hosted.site = Some("https://corp.example/jira/".into());
    let jira_link = Issue {
        key: "X-1".into(),
        url: Some("https://corp.example/jira/browse/X-1".into()),
        ..Issue::default()
    };
    assert_eq!(origin(&jira_link, &hosted), "corp.example/jira");
    assert_eq!(
        origin(
            &Issue {
                key: "X-1".into(),
                ..Issue::default()
            },
            &hosted
        ),
        "corp.example/jira"
    );
}
