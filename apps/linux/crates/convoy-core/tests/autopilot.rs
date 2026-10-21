//! Phase 2, the autopilot: what the queue starts, the branch a task's
//! worktree gets, and publishing by Convoy once review and QA pass.

mod common;

use common::fixture;
use convoy_core::integrations::{IssueSource, TrackerKind};
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::pipeline::{self, Action, Context, Event};
use convoy_core::planning::TaskInput;
use convoy_core::process::{Output, ProcessRunner, ProcessSpec};
use convoy_core::publish::{self, PublishStatus};
use convoy_core::queue;
use convoy_core::stage::{self, Stage};
use convoy_core::workspace::{ProjectPatch, Workspace};
use std::sync::{Arc, Mutex};

fn idle(_: &str) -> bool {
    false
}

fn context() -> Context<'static> {
    Context {
        skills: None,
        running: &idle,
    }
}

fn add_task(workspace: &mut Workspace, project: &str, title: &str, mode: PublishMode) -> String {
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.to_string(),
            spec_id: None,
            title: title.into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode,
            auto_review: false,
            skills: None,
        })
        .unwrap();
    workspace.state().tasks.last().unwrap().id.clone()
}

fn task(workspace: &Workspace, id: &str) -> convoy_core::model::Task {
    workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap()
        .clone()
}

#[test]
fn the_queue_respects_dependencies_and_its_slots() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    // A worktree per task needs a repository.
    std::fs::create_dir_all(fixture.path().join(".git")).unwrap();
    let first = add_task(&mut workspace, &project, "First", PublishMode::None);
    let second = add_task(&mut workspace, &project, "Second", PublishMode::None);
    let third = add_task(&mut workspace, &project, "Third", PublishMode::None);
    workspace
        .set_task_dependencies(&second, vec![first.clone()])
        .unwrap();

    // One checkout: one at a time, whatever the limit says.
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                max_parallel: Some(3),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        queue::next_tasks(&workspace, &project, &idle),
        std::slice::from_ref(&first)
    );

    // A worktree each: up to the limit, the dependent one passed by.
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                task_worktrees: Some(true),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        queue::next_tasks(&workspace, &project, &idle),
        [first.clone(), third.clone()]
    );

    // A running builder takes a slot.
    workspace.prepare_task(&first, None).unwrap();
    let builder = task(&workspace, &first).session_id.unwrap();
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Building;
            Ok(())
        })
        .unwrap();
    let running = |id: &str| id == builder;
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                max_parallel: Some(2),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert_eq!(
        queue::next_tasks(&workspace, &project, &running),
        std::slice::from_ref(&third)
    );

    // Once the first is done, the second may run.
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Done;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        queue::next_tasks(&workspace, &project, &idle),
        [second.clone(), third.clone()]
    );
}

#[test]
fn a_task_branch_is_named_after_its_issue() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let id = add_task(
        &mut workspace,
        &project,
        "LOYAL-523: Refresh the token on login, every time!",
        PublishMode::Pr,
    );
    workspace
        .update(|state| {
            state.tasks[0].source = Some(IssueSource {
                tracker: TrackerKind::Jira,
                key: "LOYAL-523".into(),
                origin: None,
                url: None,
                via_mcp: None,
            });
            Ok(())
        })
        .unwrap();
    assert_eq!(
        queue::task_branch(&workspace, &task(&workspace, &id)),
        "convoy/LOYAL-523-refresh-the-token-on-login-every-time"
    );
    // A title that does not spell in ASCII leaves the key alone; no key, an id.
    let russian = add_task(&mut workspace, &project, "Кэшбек по АЗС", PublishMode::None);
    let branch = queue::task_branch(&workspace, &task(&workspace, &russian));
    assert_eq!(branch, format!("convoy/task-{}", &russian[..8]));
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                branch_prefix: Some("feature".into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert!(
        queue::task_branch(&workspace, &task(&workspace, &id)).starts_with("feature/LOYAL-523-")
    );

    // Only a repository that asks for them gives tasks worktrees.
    workspace.prepare_task(&id, None).unwrap();
    assert_eq!(queue::wants_worktree(&workspace, &id), None);
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                task_worktrees: Some(true),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    // A folder of repositories has nothing to make a worktree of: its tasks
    // work in the folder, one at a time.
    assert_eq!(queue::wants_worktree(&workspace, &id), None);
    std::fs::create_dir_all(fixture.path().join(".git")).unwrap();
    assert!(queue::wants_worktree(&workspace, &id).is_some());
}

/// Answers git, gh and glab as a repository on branch `convoy/x` would.
struct Remote {
    host: &'static str,
    dirty: bool,
    calls: Mutex<Vec<String>>,
}

impl ProcessRunner for Remote {
    fn run(&self, spec: &ProcessSpec) -> convoy_core::Result<Output> {
        let line = format!("{} {}", spec.file, spec.args.join(" "));
        self.calls.lock().unwrap().push(line.clone());
        // gh and glab go through the login shell, every argument quoted.
        let line = line.replace('\'', "");
        let stdout = if line.contains("status --porcelain") {
            if self.dirty { " M src/app.rs\n" } else { "" }.to_string()
        } else if line.contains("rev-parse --abbrev-ref HEAD") {
            "convoy/x\n".into()
        } else if line.contains("remote get-url origin") {
            format!("git@{}:team/app.git\n", self.host)
        } else if line.contains("mr create") {
            format!(
                "Creating merge request\nhttps://{}/team/app/-/merge_requests/7\n",
                self.host
            )
        } else if line.contains("pr create") {
            format!("https://{}/team/app/pull/42\n", self.host)
        } else {
            String::new()
        };
        Ok(Output {
            status: Some(0),
            stdout,
            stderr: String::new(),
        })
    }
}

fn publishing_project(mode: PublishMode) -> (common::Fixture, Workspace, String) {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    std::fs::create_dir_all(fixture.path().join(".git")).unwrap();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                convoy_publishes: Some(true),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let id = add_task(&mut workspace, &project, "Refresh tokens", mode);
    workspace.prepare_task(&id, None).unwrap();
    (fixture, workspace, id)
}

#[test]
fn a_builder_commits_and_leaves_publishing_to_convoy() {
    let (_fixture, workspace, id) = publishing_project(PublishMode::Pr);
    let builder = task(&workspace, &id).session_id.unwrap();
    let brief = workspace.session(&builder).unwrap().prompt.clone();
    assert!(brief.contains(publish::BUILDER_NOTE));
    assert!(!brief.contains("gh pr create"));
}

#[test]
fn a_ready_task_is_written_up_then_published_on_gitlab() {
    let (_fixture, mut workspace, id) = publishing_project(PublishMode::Pr);
    let actions = pipeline::handle(
        &mut workspace,
        &context(),
        &id,
        Event::Qa(Some(true), String::new()),
    )
    .unwrap();
    let author = actions
        .iter()
        .find_map(|action| match action {
            Action::Start { session, .. } => Some(session.clone()),
            _ => None,
        })
        .expect("the author starts");
    assert!(workspace
        .session(&author)
        .unwrap()
        .title
        .starts_with("PR: "));
    let run = task(&workspace, &id).publish_run.unwrap();
    assert_eq!(run.status, PublishStatus::Writing);

    let file = stage::file(&workspace, Stage::Pr, &run.id);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        r###"{"title": "Refresh tokens on login", "body": "## What\nRefreshes."}"###,
    )
    .unwrap();
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert!(actions.contains(&Action::Publish { task: id.clone() }));
    assert_eq!(
        task(&workspace, &id).pr_title.as_deref(),
        Some("Refresh tokens on login")
    );

    let plan = publish::plan(&workspace, &id).unwrap();
    let remote = Arc::new(Remote {
        host: "gitlab.eskhata.com",
        dirty: false,
        calls: Mutex::new(Vec::new()),
    });
    let outcome = publish::run(&plan, remote.clone()).map_err(|error| error.to_string());
    let calls = remote.calls.lock().unwrap().clone();
    assert!(calls
        .iter()
        .any(|call| call.contains("push -u origin convoy/x")));
    // Through the login shell every argument is quoted; Windows runs the
    // tool directly and has nothing to quote.
    let create = calls
        .iter()
        .find(|call| call.replace('\'', "").contains("mr create"))
        .unwrap();
    assert!(
        create.contains("glab"),
        "GitLab is opened with glab: {create}"
    );
    if !cfg!(windows) {
        assert!(
            create.contains("'Refresh tokens on login'"),
            "the title is quoted: {create}"
        );
    }

    publish::finish(&mut workspace, &context(), &id, &plan.run_id, outcome).unwrap();
    let done = task(&workspace, &id);
    assert_eq!(
        done.pr_url.as_deref(),
        Some("https://gitlab.eskhata.com/team/app/-/merge_requests/7")
    );
    assert_eq!(done.status, TaskStatus::Pr);
    assert_eq!(done.publish_run.unwrap().status, PublishStatus::Published);
}

#[test]
fn uncommitted_work_is_never_published() {
    let (_fixture, mut workspace, id) = publishing_project(PublishMode::Push);
    let actions = pipeline::handle(
        &mut workspace,
        &context(),
        &id,
        Event::Review(Some(true), String::new()),
    )
    .unwrap();
    assert!(actions.contains(&Action::Publish { task: id.clone() }));
    let plan = publish::plan(&workspace, &id).unwrap();
    let remote = Arc::new(Remote {
        host: "github.com",
        dirty: true,
        calls: Mutex::new(Vec::new()),
    });
    let outcome = publish::run(&plan, remote.clone()).map_err(|error| error.to_string());
    assert!(outcome.as_ref().unwrap_err().contains("uncommitted"));
    assert!(!remote
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call.contains("push")));
    publish::finish(&mut workspace, &context(), &id, &plan.run_id, outcome).unwrap();
    let stuck = task(&workspace, &id);
    assert_eq!(stuck.status, TaskStatus::Attention);
    assert!(stuck
        .last_error
        .clone()
        .unwrap()
        .starts_with("Not published."));
    // It can be published again once fixed.
    assert!(publish::due(&workspace, &stuck));
}

#[test]
fn the_tool_follows_the_remote() {
    assert_eq!(
        publish::cli_for("git@gitlab.eskhata.com:wallet/api.git"),
        "glab"
    );
    assert_eq!(publish::cli_for("https://gitlab.com/a/b.git"), "glab");
    assert_eq!(publish::cli_for("git@github.com:a/b.git"), "gh");
    assert_eq!(publish::cli_for("https://git.corp.example/a/b.git"), "gh");
}

#[test]
fn a_folder_of_repositories_is_published_by_its_builder() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                convoy_publishes: Some(true),
                role_instructions: Some(std::collections::BTreeMap::from([(
                    convoy_core::model::Role::Pr,
                    "Use the eskhata-pr skill: the house template, in Russian.".to_string(),
                )])),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let id = add_task(
        &mut workspace,
        &project,
        "Across the folder",
        PublishMode::Pr,
    );
    workspace.prepare_task(&id, None).unwrap();
    // No git here: the builder is told to publish, and Convoy does not.
    let builder = task(&workspace, &id).session_id.unwrap();
    let brief = workspace.session(&builder).unwrap().prompt.clone();
    assert!(!brief.contains(publish::BUILDER_NOTE));
    assert!(brief.contains("a merge request with glab on GitLab"));
    assert!(!brief.contains("--fill"));
    // It opens the request the project's way.
    assert!(brief.contains("## The pull request"));
    assert!(brief.contains("Use the eskhata-pr skill: the house template, in Russian."));
    assert!(!publish::due(&workspace, &task(&workspace, &id)));
    let actions = pipeline::handle(
        &mut workspace,
        &context(),
        &id,
        Event::Review(Some(true), String::new()),
    )
    .unwrap();
    assert!(!actions
        .iter()
        .any(|action| matches!(action, Action::Publish { .. } | Action::Start { .. })));
    assert!(task(&workspace, &id).publish_run.is_none());
}

#[test]
fn a_failed_review_ends_the_qa_run_that_waited_for_it() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let id = add_task(&mut workspace, &project, "Waits", PublishMode::None);
    workspace.prepare_task(&id, None).unwrap();
    workspace
        .update(|state| {
            state.tasks[0].qa = Some(convoy_core::qa::QaRun {
                id: "q".into(),
                status: convoy_core::qa::QaStatus::Waiting,
                checks: Vec::new(),
                session_id: None,
                findings: String::new(),
                started: String::new(),
                finished: None,
            });
            Ok(())
        })
        .unwrap();
    pipeline::handle(
        &mut workspace,
        &context(),
        &id,
        Event::Review(Some(false), "1. Wrong.".into()),
    )
    .unwrap();
    assert_eq!(task(&workspace, &id).round, 1);
    assert!(
        task(&workspace, &id).qa.is_none(),
        "no stale QA wait on the card"
    );
}
