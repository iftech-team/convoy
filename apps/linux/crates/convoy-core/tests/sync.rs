//! Status sync: what is noted when a task changes, the statuses each tracker
//! offers, the mapping suggested from them, and the calls a push makes.

mod common;

use common::fixture;
use convoy_core::integrations::sync::{
    list_statuses, push, suggest_map, PendingSync, StatusGroup, StatusSync, SyncState,
    TrackerStatus,
};
use convoy_core::integrations::{
    manual_issues, Connection, HttpRequest, Issue, ProjectLink, Scope, TrackerAuth, TrackerKind,
};
use convoy_core::planning::ImportOptions;
use convoy_core::workspace::model::{Agent, PublishMode, Task, TaskStatus};
use convoy_core::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

fn scope(id: &str, name: &str) -> Scope {
    Scope {
        id: id.into(),
        name: name.into(),
        key: None,
    }
}

fn status(name: &str) -> TrackerStatus {
    TrackerStatus {
        name: name.into(),
        id: None,
        group: None,
    }
}

/// A link with sync on: Running → In Progress, Pull request → In Review,
/// Done → Done.
fn linked(workspace: Option<Scope>) -> ProjectLink {
    let mut map = BTreeMap::new();
    map.insert(TaskStatus::Building, status("In Progress"));
    map.insert(TaskStatus::Pr, status("In Review"));
    map.insert(TaskStatus::Done, status("Done"));
    ProjectLink {
        connection_id: "c1".into(),
        workspace,
        project: None,
        sync: Some(StatusSync {
            enabled: true,
            map,
            link_pr: true,
        }),
    }
}

fn task(kind: TrackerKind, key: &str, url: Option<&str>, pending: PendingSync) -> Task {
    let value = json!({
        "id": "t1", "projectID": "p", "title": key, "details": "", "findings": "",
        "agent": "claude", "status": "pr", "prURL": "https://github.com/acme/app/pull/12",
        "source": { "tracker": kind.label().to_lowercase(), "key": key, "url": url },
    });
    let mut task: Task = serde_json::from_value(value).unwrap();
    task.sync = Some(SyncState {
        pending: Some(pending),
        last: None,
    });
    task
}

/// Answers each request with the first canned body whose key is in its
/// method and URL, and keeps every request it was sent.
struct Fake {
    answers: Vec<(&'static str, String)>,
    sent: Vec<HttpRequest>,
}

impl Fake {
    fn new(answers: Vec<(&'static str, Value)>) -> Fake {
        Fake {
            answers: answers
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        if value.is_null() {
                            String::new()
                        } else {
                            value.to_string()
                        },
                    )
                })
                .collect(),
            sent: Vec::new(),
        }
    }

    fn call(&mut self, request: &HttpRequest) -> Result<String> {
        self.sent.push(request.clone());
        let line = format!("{} {}", request.method, request.url);
        for (key, answer) in &self.answers {
            if line.contains(key) {
                return Ok(answer.clone());
            }
        }
        panic!("no answer for {line}");
    }

    fn lines(&self) -> Vec<String> {
        self.sent
            .iter()
            .map(|request| format!("{} {}", request.method, request.url))
            .collect()
    }

    fn body(&self, key: &str) -> Value {
        let request = self
            .sent
            .iter()
            .find(|request| format!("{} {}", request.method, request.url).contains(key))
            .unwrap_or_else(|| panic!("nothing sent to {key}"));
        serde_json::from_str(request.body.as_deref().unwrap()).unwrap()
    }
}

fn pending(status: Option<TaskStatus>, pr: bool) -> PendingSync {
    PendingSync { status, pr }
}

#[test]
fn a_status_change_or_a_new_pull_request_is_noted_for_the_tracker() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let peshbar = connection(TrackerKind::Peshbar);
    let mut mcp = connection(TrackerKind::Linear);
    mcp.auth = TrackerAuth::Mcp;
    let options = ImportOptions {
        skills: Vec::new(),
        agent: Agent::Claude,
        model: String::new(),
        mode: PublishMode::Pr,
        auto_review: false,
        overrides: BTreeMap::new(),
    };
    let issue = Issue {
        key: "MOB-4".into(),
        title: "Fix login".into(),
        ..Issue::default()
    };
    workspace
        .import_issues(&project, &peshbar, &[issue], &options)
        .unwrap();
    workspace
        .import_issues(&project, &mcp, &manual_issues("ENG-9"), &options)
        .unwrap();
    let id_of = |key: &str, workspace: &convoy_core::Workspace| {
        workspace
            .state()
            .tasks
            .iter()
            .find(|task| task.source.as_ref().unwrap().key == key)
            .unwrap()
            .id
            .clone()
    };
    let synced = id_of("MOB-4", &workspace);
    let agent_only = id_of("ENG-9", &workspace);
    // Imported: nothing to push yet.
    assert!(workspace
        .state()
        .tasks
        .iter()
        .all(|task| task.sync.is_none()));

    workspace
        .set_task_status(&synced, TaskStatus::Review)
        .unwrap();
    workspace
        .set_task_status(&agent_only, TaskStatus::Review)
        .unwrap();
    workspace
        .update(|state| {
            let task = state
                .tasks
                .iter_mut()
                .find(|task| task.id == synced)
                .unwrap();
            task.pr_url = Some("https://github.com/acme/app/pull/12".into());
            task.status = TaskStatus::Pr;
            Ok(())
        })
        .unwrap();
    fn find(workspace: &convoy_core::Workspace, id: &str) -> Task {
        workspace
            .state()
            .tasks
            .iter()
            .find(|task| task.id == id)
            .unwrap()
            .clone()
    }
    assert_eq!(
        find(&workspace, &synced).sync.unwrap().pending,
        Some(pending(Some(TaskStatus::Pr), true)),
        "the latest status wins and the pull request is kept"
    );
    assert!(
        find(&workspace, &agent_only).sync.is_none(),
        "an MCP task cannot be reached"
    );

    // Recording the outcome changes no status, so nothing new is noted.
    workspace
        .update(|state| {
            let task = state
                .tasks
                .iter_mut()
                .find(|task| task.id == synced)
                .unwrap();
            task.sync.as_mut().unwrap().pending = None;
            Ok(())
        })
        .unwrap();
    assert_eq!(find(&workspace, &synced).sync.unwrap().pending, None);
}

#[test]
fn a_mapping_is_suggested_from_the_trackers_statuses() {
    let statuses = vec![
        TrackerStatus {
            name: "Backlog".into(),
            id: None,
            group: Some(StatusGroup::Todo),
        },
        TrackerStatus {
            name: "Doing".into(),
            id: None,
            group: Some(StatusGroup::Started),
        },
        TrackerStatus {
            name: "Code Review".into(),
            id: None,
            group: Some(StatusGroup::Started),
        },
        TrackerStatus {
            name: "QA".into(),
            id: None,
            group: Some(StatusGroup::Started),
        },
        TrackerStatus {
            name: "Shipped".into(),
            id: None,
            group: Some(StatusGroup::Done),
        },
    ];
    let map = suggest_map(&statuses);
    let name = |status: TaskStatus| map.get(&status).map(|found| found.name.as_str());
    assert_eq!(name(TaskStatus::Building), Some("Doing"));
    assert_eq!(name(TaskStatus::Review), Some("Code Review"));
    assert_eq!(name(TaskStatus::Pr), Some("Code Review"));
    assert_eq!(name(TaskStatus::Testing), Some("QA"));
    assert_eq!(
        name(TaskStatus::Done),
        Some("Shipped"),
        "by kind when no name matches"
    );
    assert_eq!(name(TaskStatus::Queued), None);
    assert_eq!(name(TaskStatus::Failed), None);
}

#[test]
fn each_tracker_lists_its_statuses_once_each() {
    // Linear, every team: the same name twice is listed once.
    let mut fake = Fake::new(vec![(
        "graphql",
        json!({ "data": { "teams": { "nodes": [
            { "states": { "nodes": [
                { "id": "s2", "name": "In Progress", "type": "started", "position": 2 },
                { "id": "s1", "name": "Todo", "type": "unstarted", "position": 1 },
            ] } },
            { "states": { "nodes": [{ "id": "s9", "name": "In Progress", "type": "started", "position": 2 }] } },
        ] } } }),
    )]);
    let found = list_statuses(
        &connection(TrackerKind::Linear),
        &linked(None),
        &mut |request| fake.call(request),
    )
    .unwrap();
    let names: Vec<&str> = found.iter().map(|status| status.name.as_str()).collect();
    assert_eq!(names, ["Todo", "In Progress"]);
    assert_eq!(found[1].group, Some(StatusGroup::Started));

    // Jira, a linked project: its statuses across issue types.
    let mut fake = Fake::new(vec![(
        "/project/PAY/statuses",
        json!([
            { "name": "Task", "statuses": [{ "id": "1", "name": "To Do", "statusCategory": { "key": "new" } }] },
            { "name": "Bug", "statuses": [
                { "id": "1", "name": "To Do", "statusCategory": { "key": "new" } },
                { "id": "3", "name": "Done", "statusCategory": { "key": "done" } },
            ] },
        ]),
    )]);
    let mut link = linked(Some(Scope {
        id: "10001".into(),
        name: "Payments".into(),
        key: Some("PAY".into()),
    }));
    let found = list_statuses(&connection(TrackerKind::Jira), &link, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].group, Some(StatusGroup::Done));
    assert!(fake.lines()[0].contains("acme.atlassian.net/rest/api/3/project/PAY/statuses"));

    // Peshbar, a workspace without a project: every project's columns.
    let mut fake = Fake::new(vec![
        (
            "/api/projects?per_page=100",
            json!({ "data": [{ "id": 3 }, { "id": 4 }] }),
        ),
        (
            "/api/projects/3/statuses",
            json!({ "data": [
            { "id": 11, "name": "To do", "position": 0, "is_default": true },
            { "id": 12, "name": "Done", "position": 1, "is_completed": true },
        ] }),
        ),
        (
            "/api/projects/4/statuses",
            json!({ "data": [{ "id": 21, "name": "QA", "position": 0 }] }),
        ),
    ]);
    link = linked(Some(scope("7", "Peshbar")));
    let found = list_statuses(&connection(TrackerKind::Peshbar), &link, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    let names: Vec<&str> = found.iter().map(|status| status.name.as_str()).collect();
    assert_eq!(names, ["To do", "Done", "QA"]);
    assert!(fake
        .sent
        .iter()
        .all(|request| request.header("X-Workspace") == Some("7")));

    // Notion: the status property's options, grouped.
    let mut fake = Fake::new(vec![(
        "/databases/db1",
        json!({ "properties": {
            "Name": { "type": "title" },
            "Status": { "type": "status", "status": {
                "options": [{ "id": "o1", "name": "Not started" }, { "id": "o2", "name": "Done" }],
                "groups": [
                    { "name": "To-do", "option_ids": ["o1"] },
                    { "name": "Complete", "option_ids": ["o2"] },
                ],
            } },
        } }),
    )]);
    link = linked(Some(scope("db1", "Tasks")));
    let found = list_statuses(&connection(TrackerKind::Notion), &link, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert_eq!(found[1].group, Some(StatusGroup::Done));
    assert!(list_statuses(
        &connection(TrackerKind::Notion),
        &linked(None),
        &mut |request| fake.call(request)
    )
    .is_err());
}

#[test]
fn a_peshbar_task_moves_column_and_gets_the_pull_request_as_a_comment() {
    let mut fake = Fake::new(vec![
        (
            "GET https://management-api.peshbar.com/api/search",
            json!({ "tasks": [{ "id": 40, "project_id": 3, "code": "MOB-4" }] }),
        ),
        (
            "/api/projects/3/statuses",
            json!({ "data": [{ "id": 11, "name": "To do" }, { "id": 12, "name": "In review" }] }),
        ),
        ("PUT", json!({ "data": {} })),
        ("/api/tasks/40/comments", json!({ "data": {} })),
    ]);
    let link = linked(Some(scope("7", "Peshbar")));
    let task = task(
        TrackerKind::Peshbar,
        "MOB-4",
        None,
        pending(Some(TaskStatus::Pr), true),
    );
    let result = push(
        &connection(TrackerKind::Peshbar),
        &link,
        &task,
        &mut |request| fake.call(request),
    )
    .unwrap();
    assert!(result.ok, "{}", result.message);
    assert_eq!(result.message, "Set to In Review · pull request linked");
    assert_eq!(result.status.as_deref(), Some("In Review"));
    assert_eq!(fake.body("PUT")["status_id"], 12);
    assert!(fake
        .lines()
        .iter()
        .any(|line| line.ends_with("/api/projects/3/tasks/40/status")));
    let comment = fake.body("/comments")["body"].as_str().unwrap().to_string();
    assert!(
        comment.contains("Pull request #12")
            && comment.contains("<a href=\"https://github.com/acme/app/pull/12\">")
    );
}

#[test]
fn a_linear_issue_changes_state_and_attaches_the_pull_request() {
    let mut fake = Fake::new(vec![]);
    let answers = [
        json!({ "data": { "issue": { "id": "uuid-1", "team": { "states": { "nodes": [
            { "id": "s1", "name": "Todo" }, { "id": "s3", "name": "In Review" },
        ] } } } } }),
        json!({ "data": { "issueUpdate": { "success": true } } }),
        json!({ "data": { "attachmentLinkURL": { "success": true } } }),
    ];
    let mut index = 0;
    let task = task(
        TrackerKind::Linear,
        "ENG-12",
        None,
        pending(Some(TaskStatus::Pr), true),
    );
    let result = push(
        &connection(TrackerKind::Linear),
        &linked(None),
        &task,
        &mut |request| {
            fake.sent.push(request.clone());
            index += 1;
            Ok(answers[index - 1].to_string())
        },
    )
    .unwrap();
    assert!(result.ok, "{}", result.message);
    let variables = |at: usize| -> Value {
        serde_json::from_str::<Value>(fake.sent[at].body.as_deref().unwrap()).unwrap()["variables"]
            .clone()
    };
    assert_eq!(variables(0)["id"], "ENG-12");
    assert_eq!(variables(1), json!({ "id": "uuid-1", "s": "s3" }));
    assert_eq!(variables(2)["url"], "https://github.com/acme/app/pull/12");
}

#[test]
fn jira_moves_through_a_transition_and_says_when_there_is_none() {
    let mut fake = Fake::new(vec![
        (
            "GET https://acme.atlassian.net/rest/api/3/issue/PAY-7/transitions",
            json!({ "transitions": [
            { "id": "21", "to": { "name": "In Progress" } },
            { "id": "31", "to": { "name": "In Review" } },
        ] }),
        ),
        (
            "POST https://acme.atlassian.net/rest/api/3/issue/PAY-7/transitions",
            Value::Null,
        ),
        ("/remotelink", json!({ "id": 1 })),
    ]);
    let jira = connection(TrackerKind::Jira);
    let task_in_review = task(
        TrackerKind::Jira,
        "PAY-7",
        None,
        pending(Some(TaskStatus::Pr), true),
    );
    let result = push(&jira, &linked(None), &task_in_review, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert!(result.ok, "{}", result.message);
    assert_eq!(
        fake.body("POST https://acme.atlassian.net/rest/api/3/issue/PAY-7/transitions")
            ["transition"]["id"],
        "31"
    );
    assert_eq!(
        fake.body("/remotelink")["globalId"],
        "https://github.com/acme/app/pull/12"
    );

    let done = task(
        TrackerKind::Jira,
        "PAY-7",
        None,
        pending(Some(TaskStatus::Done), false),
    );
    let result = push(&jira, &linked(None), &done, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert!(!result.ok);
    assert!(
        result.message.contains("no transition to “Done”"),
        "{}",
        result.message
    );
}

#[test]
fn a_notion_page_takes_a_known_option_only() {
    const PAGE: &str = "1a2b3c4d5e6f47a8b9c0d1e2f3a4b5c6";
    let schema = json!({ "properties": {
        "Stage": { "type": "select", "select": { "options": [{ "name": "In Review" }, { "name": "Shipped" }] } },
        "Status": { "type": "select", "select": { "options": [{ "name": "In Review" }, { "name": "Shipped" }] } },
        "PR": { "type": "url" },
    } });
    let mut fake = Fake::new(vec![
        (
            "GET https://api.notion.com/v1/pages/",
            json!({ "object": "page", "parent": { "database_id": "db1" } }),
        ),
        ("/databases/db1", schema),
        ("PATCH", json!({ "object": "page" })),
    ]);
    let url = format!("https://www.notion.so/acme/Fix-login-{PAGE}");
    let notion = connection(TrackerKind::Notion);
    let review = task(
        TrackerKind::Notion,
        "TASK-3",
        Some(&url),
        pending(Some(TaskStatus::Pr), true),
    );
    let result = push(&notion, &linked(None), &review, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert!(result.ok, "{}", result.message);
    let patches: Vec<Value> = fake
        .sent
        .iter()
        .filter(|request| request.method == "PATCH")
        .map(|request| serde_json::from_str(request.body.as_deref().unwrap()).unwrap())
        .collect();
    assert_eq!(
        patches[0],
        json!({ "properties": { "Status": { "select": { "name": "In Review" } } } })
    );
    assert_eq!(
        patches[1],
        json!({ "properties": { "PR": { "url": "https://github.com/acme/app/pull/12" } } })
    );

    // "Done" is not one of the options: refused rather than created.
    let done = task(
        TrackerKind::Notion,
        "TASK-3",
        Some(&url),
        pending(Some(TaskStatus::Done), false),
    );
    let result = push(&notion, &linked(None), &done, &mut |request| {
        fake.call(request)
    })
    .unwrap();
    assert!(!result.ok);
    assert!(
        result.message.contains("not an option"),
        "{}",
        result.message
    );
}

#[test]
fn nothing_is_pushed_when_sync_is_off_or_the_status_is_not_mapped() {
    let mut fake = Fake::new(vec![]);
    let peshbar = connection(TrackerKind::Peshbar);
    let mut off = linked(None);
    off.sync.as_mut().unwrap().enabled = false;
    let changed = task(
        TrackerKind::Peshbar,
        "MOB-4",
        None,
        pending(Some(TaskStatus::Pr), false),
    );
    assert!(push(&peshbar, &off, &changed, &mut |request| fake.call(request)).is_none());
    let unmapped = task(
        TrackerKind::Peshbar,
        "MOB-4",
        None,
        pending(Some(TaskStatus::Queued), false),
    );
    assert!(push(&peshbar, &linked(None), &unmapped, &mut |request| fake
        .call(request))
    .is_none());
    assert!(fake.sent.is_empty());
}
