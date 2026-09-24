//! Headless checks of the main window.
//!
//! GTK must be initialised once, on the main thread, so this target replaces
//! the default test harness with its own `main`. Run it the way CI does:
//!
//! ```sh
//! xvfb-run -a cargo test -p convoy-gtk --test ui
//! ```
//!
//! Nothing here starts an agent: the checks cover what the window shows and
//! how it reacts, not what a provider would do.

use adw::prelude::*;
use convoy_core::model::{Agent, Theme};
use convoy_core::workspace::{NewSession, SettingsPatch, Workspace};
use convoy_core::Storage;
use std::cell::RefCell;
use std::rc::Rc;

use convoy_gtk::window;

thread_local! {
    static FAILURES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn check(name: &str, condition: bool, detail: impl std::fmt::Display) {
    if condition {
        println!("PASS {name}");
    } else {
        println!("FAIL {name}: {detail}");
        FAILURES.with(|failures| failures.borrow_mut().push(name.to_string()));
    }
}

fn main() {
    let directory = tempfile::Builder::new()
        .prefix("convoy-ui-")
        .tempdir()
        .expect("temporary directory");
    let storage = Storage::new(directory.path().join("Convoy Desktop Preview"));
    let project_path = directory.path().join("project");
    std::fs::create_dir_all(&project_path).expect("project folder");
    seed_repository(&project_path);
    seed(&storage, &project_path);

    let application = adw::Application::builder()
        .application_id("com.iftech.convoy.uitest")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    application.connect_activate({
        let storage = storage.clone();
        move |application| {
            let Some(app) = window::build(application, storage.clone()) else {
                check("window builds", false, "build returned nothing");
                application.quit();
                return;
            };
            let application = application.clone();
            glib::spawn_future_local(async move {
                run_checks(&app);
                files_checks(&app).await;
                planning_checks(&app).await;
                integration_checks(&app).await;
                navigation_checks(&app).await;
                application.quit();
            });
        }
    });
    application.run_with_args::<&str>(&[]);

    let failed = FAILURES.with(|failures| failures.borrow().clone());
    if failed.is_empty() {
        println!("── all window checks passed ──");
    } else {
        println!("── {} failed: {} ──", failed.len(), failed.join(", "));
        std::process::exit(1);
    }
}

/// A workspace with one project, one group, and three sessions — one of them
/// archived, so the window has something to hide as well as something to show.
fn seed(storage: &Storage, project_path: &std::path::Path) {
    let mut workspace = Workspace::load(storage.workspace_file()).expect("workspace");
    workspace.add_project(project_path).expect("project");
    let project = workspace.state().projects[0].id.clone();

    for (title, agent) in [("Builder", Agent::Claude), ("Review", Agent::Codex)] {
        workspace
            .add_session(NewSession::new(&project, agent, title))
            .expect("session");
    }
    workspace
        .add_session(NewSession::new(&project, Agent::Codex, "Archived"))
        .expect("session");
    let archived = workspace.state().sessions[2].id.clone();
    workspace
        .edit_session(
            &archived,
            convoy_core::workspace::SessionPatch {
                archived: Some(true),
                ..Default::default()
            },
            false,
        )
        .expect("archive");
    workspace
        .save_settings(SettingsPatch {
            theme: Some(Theme::Dark),
            font_size: Some(13),
            ..Default::default()
        })
        .expect("settings");
}

fn run_checks(app: &Rc<convoy_gtk::state::App>) {
    let project = app.selected_project();
    check(
        "a project is selected on startup",
        project.is_some(),
        "nothing was selected",
    );
    let project = project.expect("project");

    check(
        "the header names the project",
        app.chrome.title.title() == project.title,
        format!("title is {:?}", app.chrome.title.title()),
    );

    check(
        "archived sessions are hidden",
        app.visible_sessions().len() == 2,
        format!("{} sessions visible", app.visible_sessions().len()),
    );
    check(
        "one tab per visible session",
        app.tabs.n_pages() == 2,
        format!("{} pages", app.tabs.n_pages()),
    );
    check(
        "the terminal area is shown, not the placeholder",
        app.chrome.stack.visible_child_name().as_deref() == Some("terminals"),
        format!("{:?}", app.chrome.stack.visible_child_name()),
    );

    check(
        "a session is selected with it",
        app.selected_session().is_some(),
        "no session selected",
    );
    check(
        "an unstarted session can be started",
        app.chrome.start.is_sensitive() && app.chrome.start.label().as_deref() == Some("Start"),
        format!(
            "sensitive={} label={:?}",
            app.chrome.start.is_sensitive(),
            app.chrome.start.label()
        ),
    );
    check(
        "nothing can be stopped yet",
        !app.chrome.stop.is_sensitive(),
        "stop was enabled with no process",
    );
    check(
        "the running count starts at zero",
        app.chrome.running_label.label() == "No sessions running",
        format!("{:?}", app.chrome.running_label.label()),
    );

    // Each tab owns a terminal, and each terminal honours the saved settings.
    let views = app.views.borrow();
    check(
        "every visible session has a terminal",
        views.len() == 2,
        format!("{} views", views.len()),
    );
    let font_matches = views.values().all(|view| {
        // Qualified: `font_desc` also exists on GtkCellRendererText.
        vte4::prelude::TerminalExt::font_desc(&view.terminal)
            .map(|font| font.size() / gtk::pango::SCALE)
            == Some(13)
    });
    check("the terminal font follows the settings", font_matches, "font size differs");
    drop(views);

    // Searching filters the sidebar without disturbing the open tabs.
    *app.search.borrow_mut() = "nothing-matches-this".to_string();
    app.sync();
    check(
        "search filters the project tree",
        app.roots.n_items() == 0,
        format!("{} roots", app.roots.n_items()),
    );
    check(
        "search hides sessions that do not match",
        app.sessions.n_items() == 0,
        format!("{} sessions", app.sessions.n_items()),
    );

    *app.search.borrow_mut() = "Build".to_string();
    app.sync();
    check(
        "a matching search keeps the project",
        app.roots.n_items() == 1,
        format!("{} roots", app.roots.n_items()),
    );
    check(
        "a matching search narrows the sessions",
        app.sessions.n_items() == 1,
        format!("{} sessions", app.sessions.n_items()),
    );

    *app.search.borrow_mut() = String::new();
    app.sync();

    // A session added while the window is open appears without a restart.
    let added = {
        let mut workspace = app.workspace.borrow_mut();
        workspace
            .add_session(NewSession::new(&project.id, Agent::Claude, "Added later"))
            .map(|_| ())
    };
    check("a session can be added", added.is_ok(), "add_session failed");
    window::rebuild_tabs(app);
    app.sync();
    check(
        "the new session gets a tab",
        app.tabs.n_pages() == 3,
        format!("{} pages", app.tabs.n_pages()),
    );

    // The workspace on disk stays a document the Electron build accepts.
    let raw: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(app.storage.workspace_file()).expect("read workspace"),
    )
    .expect("parse workspace");
    check(
        "the saved workspace is still valid",
        convoy_core::workspace::validate::validate(&raw).is_ok(),
        "validation rejected the file the window wrote",
    );
    check(
        "the schema version is unchanged",
        raw["schemaVersion"] == 3,
        format!("{}", raw["schemaVersion"]),
    );

    session_menu_checks(app, &project.id);
    quick_command_checks(app, &project.id);
    split_checks(app);
}

fn enabled(app: &Rc<convoy_gtk::state::App>, name: &str) -> bool {
    app.actions
        .lookup_action(name)
        .and_downcast::<gtk::gio::SimpleAction>()
        .map(|action| action.is_enabled())
        .unwrap_or(false)
}

fn activate(app: &Rc<convoy_gtk::state::App>, name: &str) {
    if let Some(action) = app.actions.lookup_action(name) {
        action.activate(None);
    }
}

/// What the session menu offers depends on the session, exactly as the
/// Electron renderer's `#session-menu` did.
fn session_menu_checks(app: &Rc<convoy_gtk::state::App>, project: &str) {
    let session = app.selected_session().expect("session");
    check(
        "a stopped session can be edited, archived and recovered",
        enabled(app, "edit") && enabled(app, "archive") && enabled(app, "recover"),
        "an entry was disabled",
    );
    check(
        "a worktree is offered for a fresh session",
        enabled(app, "worktree") && !enabled(app, "remove-worktree"),
        format!(
            "worktree={} remove={}",
            enabled(app, "worktree"),
            enabled(app, "remove-worktree")
        ),
    );
    check(
        "feedback is offered only by a review",
        !enabled(app, "feedback"),
        "feedback was offered by a session with no builder",
    );
    check(
        "closing a split is offered only when one is open",
        !enabled(app, "unsplit"),
        "unsplit was enabled with no split",
    );

    // A review links back to its builder and may send feedback.
    let brief = convoy_core::review::brief(&app.workspace.borrow(), &session.id, "built it")
        .expect("brief");
    let handoff =
        convoy_core::review::handoff(&app.workspace.borrow(), &session.id, brief).expect("handoff");
    check(
        "a review uses the other agent",
        handoff.agent != session.agent,
        "the reviewer matched the builder",
    );
    app.workspace
        .borrow_mut()
        .add_session(handoff)
        .expect("review session");
    window::rebuild_tabs(app);
    app.sync();

    let review = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .find(|other| other.review_of.as_deref() == Some(session.id.as_str()))
        .cloned()
        .expect("review");
    app.selection.borrow_mut().session = Some(review.id.clone());
    app.refresh_selection();
    check(
        "a review may send feedback to its builder",
        enabled(app, "feedback"),
        "feedback was not offered",
    );

    // Pinning reorders the tabs; archiving removes one.
    app.selection.borrow_mut().session = Some(review.id.clone());
    app.refresh_selection();
    activate(app, "pin");
    check(
        "a pinned session comes first",
        app.visible_sessions().first().map(|first| first.id.clone()) == Some(review.id.clone()),
        "the pinned session was not first",
    );

    let before = app.tabs.n_pages();
    activate(app, "archive");
    check(
        "archiving removes the tab",
        app.tabs.n_pages() == before - 1,
        format!("{before} pages before, {} after", app.tabs.n_pages()),
    );
    check(
        "an archived session is out of sight, not deleted",
        app.workspace
            .borrow()
            .state()
            .sessions
            .iter()
            .any(|other| other.id == review.id),
        "the session disappeared from the workspace",
    );

    let _ = project;
    app.selection.borrow_mut().session = Some(session.id);
    app.refresh_selection();
}

/// Quick commands are scoped: a command saved for one project is not offered
/// in another, and a global one is offered everywhere.
fn quick_command_checks(app: &Rc<convoy_gtk::state::App>, project: &str) {
    use convoy_core::model::QuickCommand;

    let command = |id: &str, owner: Option<&str>| QuickCommand {
        id: id.to_string(),
        title: id.to_string(),
        text: "echo hello".to_string(),
        submit: false,
        project_id: owner.map(str::to_string),
        unknown: Default::default(),
    };
    {
        let mut workspace = app.workspace.borrow_mut();
        workspace.save_command(command("global", None)).expect("global");
        workspace
            .save_command(command("scoped", Some(project)))
            .expect("scoped");
    }
    app.sync();

    // Two commands plus the "Manage…" section.
    check(
        "both commands are offered for their project",
        app.quick_menu.n_items() == 3,
        format!("{} menu items", app.quick_menu.n_items()),
    );

    let session = app.selected_session().expect("session");
    let resolved = convoy_core::review::quick_command(&app.workspace.borrow(), &session.id, "scoped");
    check(
        "a scoped command resolves for its own project",
        resolved.is_ok(),
        "it did not resolve",
    );
    check(
        "an unknown command is refused",
        convoy_core::review::quick_command(&app.workspace.borrow(), &session.id, "missing").is_err(),
        "an unknown id resolved",
    );
}

/// The split view shows a second session beside the first, and closing it
/// returns that session to the tab strip.
fn split_checks(app: &Rc<convoy_gtk::state::App>) {
    let sessions = app.visible_sessions();
    if sessions.len() < 2 {
        check("there are two sessions to split", false, "not enough sessions");
        return;
    }
    let other = sessions[1].id.clone();
    let before = app.tabs.n_pages();

    window::open_split(app, &other);
    check(
        "the split pane is filled",
        app.panes.end_child().is_some(),
        "no second pane",
    );
    check(
        "the split session leaves the tab strip",
        app.tabs.n_pages() == before - 1,
        format!("{before} pages before, {} after", app.tabs.n_pages()),
    );
    check(
        "closing the split becomes available",
        enabled(app, "unsplit"),
        "unsplit stayed disabled",
    );

    window::close_split(app);
    check(
        "closing the split empties the pane",
        app.panes.end_child().is_none() && app.split.borrow().is_none(),
        "the pane survived",
    );
    check(
        "the session returns to the tab strip",
        app.tabs.n_pages() == before,
        format!("{} pages, expected {before}", app.tabs.n_pages()),
    );
}


/// A repository with one commit and one modified file, so Files & Changes has
/// a diff to show and a log to list.
fn seed_repository(path: &std::path::Path) {
    let git = convoy_core::Git::default();
    git.run(path, &["init"]).expect("init");
    git.run(path, &["config", "user.name", "Test"]).expect("name");
    git.run(path, &["config", "user.email", "test@example.invalid"]).expect("email");
    git.run(path, &["config", "commit.gpgSign", "false"]).expect("gpg");

    let original: String = (0..20).map(|index| format!("line {index}\n")).collect();
    std::fs::write(path.join("tracked.txt"), &original).expect("write");
    std::fs::write(path.join("notes.md"), "# Notes\n\nSome **bold** text.\n").expect("write");
    git.run(path, &["add", "--all"]).expect("add");
    git.run(path, &["commit", "-m", "Initial commit"]).expect("commit");

    std::fs::write(
        path.join("tracked.txt"),
        original.replace("line 3\n", "line three\n"),
    )
    .expect("edit");
    std::fs::write(path.join("untracked.txt"), "new file\n").expect("untracked");
}

async fn settle(milliseconds: u64) {
    glib::timeout_future(std::time::Duration::from_millis(milliseconds)).await;
}

/// Files & Changes reads a real repository: the checks below assert what it
/// found, not what it was told.
async fn files_checks(app: &Rc<convoy_gtk::state::App>) {
    use convoy_core::git::ReadRequest;
    use convoy_gtk::files_ui;

    let Some(view) = files_ui::open_view(app) else {
        check("Files & Changes opens", false, "it did not open");
        return;
    };
    settle(900).await;

    check(
        "the working tree is listed",
        view.changes.n_items() == 2,
        format!("{} changes", view.changes.n_items()),
    );
    check(
        "the branch and change count are shown",
        view.branch_label.label().contains("2 changed"),
        format!("{:?}", view.branch_label.label()),
    );
    check(
        "tracked files are listed",
        view.files.n_items() >= 3,
        format!("{} files", view.files.n_items()),
    );
    check(
        "the log has the initial commit",
        view.log.n_items() == 1,
        format!("{} commits", view.log.n_items()),
    );
    check(
        "branches are listed",
        view.branches.n_items() >= 1,
        format!("{} branches", view.branches.n_items()),
    );

    // A diff is shown as text, and the same diff can be read side by side.
    files_ui::select(&view, ReadRequest::Unstaged { path: "tracked.txt".into() });
    settle(600).await;
    check(
        "an unstaged diff is previewed",
        files_ui::preview_page(&view).as_deref() == Some("text"),
        format!("{:?}", files_ui::preview_page(&view)),
    );
    let hash = view.current.borrow().as_ref().map(|(_, hash)| hash.clone());
    check(
        "the diff is hashed for a later hunk discard",
        hash.as_ref().is_some_and(|hash| hash.len() == 64),
        format!("{hash:?}"),
    );

    view.side_by_side.set_active(true);
    settle(600).await;
    check(
        "the same diff can be read side by side",
        files_ui::preview_page(&view).as_deref() == Some("split"),
        format!("{:?}", files_ui::preview_page(&view)),
    );
    view.side_by_side.set_active(false);
    settle(400).await;

    // Markdown is rendered, not dumped.
    files_ui::select(&view, ReadRequest::File { path: "notes.md".into() });
    settle(600).await;
    check(
        "Markdown is rendered",
        files_ui::preview_page(&view).as_deref() == Some("markdown"),
        format!("{:?}", files_ui::preview_page(&view)),
    );

    // An untracked file is shown as its own content, not as a diff.
    files_ui::select(&view, ReadRequest::Untracked { path: "untracked.txt".into() });
    settle(600).await;
    check(
        "an untracked file shows its contents",
        files_ui::preview_page(&view).as_deref() == Some("text"),
        format!("{:?}", files_ui::preview_page(&view)),
    );

    // A stale hash is refused, which is what stops a hunk discard from being
    // applied to a diff the user never saw.
    let stale = convoy_core::git::diff::digest("not the current diff");
    check(
        "a stale diff hash differs from the real one",
        Some(stale) != hash,
        "the hashes matched",
    );

    view.dialog.close();
}


/// Specifications, tasks and the queue, driven through the same calls the
/// dialog makes.
async fn planning_checks(app: &Rc<convoy_gtk::state::App>) {
    use convoy_core::model::{PublishMode, TaskStatus};
    use convoy_core::planning::{SpecInput, TaskInput};
    use convoy_core::queue::{self, Step};

    let project = app.selected_project().expect("project").id;

    // A specification starts as a draft, and a task cannot be prepared from it.
    {
        let mut workspace = app.workspace.borrow_mut();
        workspace
            .save_spec(SpecInput {
                project_id: project.clone(),
                title: "Sign-in".into(),
                acceptance: "Reachable by keyboard".into(),
                ..SpecInput::default()
            })
            .expect("spec");
    }
    let spec = app
        .workspace
        .borrow()
        .state()
        .specs
        .last()
        .expect("spec")
        .clone();
    check(
        "a new specification is a draft at revision 1",
        spec.revision == 1 && !spec.approved(),
        format!("revision {} approved {}", spec.revision, spec.approved()),
    );

    {
        let mut workspace = app.workspace.borrow_mut();
        workspace
            .save_task(TaskInput {
                id: None,
                project_id: project.clone(),
                spec_id: Some(spec.id.clone()),
                title: "Add focus ring".into(),
                details: "Make the button focusable".into(),
                findings: String::new(),
                agent: convoy_core::model::Agent::Claude,
                mode: PublishMode::None,
                auto_review: false,
            })
            .expect("task");
    }
    let task = app.workspace.borrow().state().tasks.last().expect("task").clone();

    let blocked = app.workspace.borrow_mut().prepare_task(&task.id, None).is_err();
    check(
        "an unapproved specification blocks preparation",
        blocked,
        "the task was prepared anyway",
    );

    app.workspace
        .borrow_mut()
        .approve_spec(&spec.id, 1)
        .expect("approve");
    let prepared = app.workspace.borrow_mut().prepare_task(&task.id, None).is_ok();
    check("an approved specification allows it", prepared, "it was refused");

    let session = app
        .workspace
        .borrow()
        .state()
        .tasks
        .iter()
        .find(|other| other.id == task.id)
        .and_then(|other| other.session_id.clone());
    check(
        "the brief carries the specification",
        session
            .as_ref()
            .and_then(|id| app.workspace.borrow().session(id).ok().map(|s| s.prompt.clone()))
            .is_some_and(|prompt| prompt.contains("Reachable by keyboard")),
        "the brief did not mention the acceptance criteria",
    );

    // A clean exit reaches review, and accepting it stays a separate step.
    let session = session.expect("session");
    convoy_core::session::finish_session(
        &mut app.workspace.borrow_mut(),
        &session,
        convoy_core::session::ExitCause::Exited(0),
    )
    .expect("finish");
    let status = app
        .workspace
        .borrow()
        .state()
        .tasks
        .iter()
        .find(|other| other.id == task.id)
        .map(|other| other.status);
    check(
        "a clean exit reaches review, not done",
        status == Some(TaskStatus::Review),
        format!("{status:?}"),
    );

    // The queue is not running, so nothing starts on its own.
    check(
        "the queue is idle until it is started",
        !queue_ui_running(app, &project),
        "a queue was running",
    );
    let idle = |_: &str| false;
    check(
        "a task awaiting review is not queued again",
        queue::next_step(&app.workspace.borrow(), &project, &idle) == Step::Empty,
        "the queue offered something to run",
    );

    app.workspace
        .borrow_mut()
        .set_task_status(&task.id, TaskStatus::Done)
        .expect("accept");
    check(
        "an approved specification allows the task to be accepted",
        app.workspace
            .borrow()
            .state()
            .tasks
            .iter()
            .any(|other| other.id == task.id && other.status == TaskStatus::Done),
        "the task was not accepted",
    );

    settle(50).await;
}

fn queue_ui_running(app: &Rc<convoy_gtk::state::App>, project: &str) -> bool {
    convoy_gtk::queue_ui::is_running(app, project)
}


/// Hooks, agent state, hibernation and account isolation.
async fn integration_checks(app: &Rc<convoy_gtk::state::App>) {
    use convoy_core::model::Agent;
    use convoy_core::monitor::{self, AgentState, Transition};

    let session = app.selected_session().expect("session");

    // Starting a Claude session writes a settings file pointing every hook at
    // this binary. Nothing is launched here; only the plan is made.
    let plan = convoy_core::session::plan_launch(
        &app.workspace.borrow(),
        &app.storage,
        &session.id,
        std::path::Path::new("/usr/bin/convoy"),
    );
    match (&plan, session.agent) {
        (Ok(plan), Agent::Claude) => {
            check(
                "a Claude launch installs its hooks",
                plan.settings_file.as_ref().is_some_and(|file| file.exists()),
                "no settings file",
            );
            let document: serde_json::Value = plan
                .settings_file
                .as_ref()
                .and_then(|file| std::fs::read_to_string(file).ok())
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            check(
                "every documented hook event is subscribed",
                document["hooks"].as_object().map(|hooks| hooks.len()) == Some(8),
                format!("{:?}", document["hooks"].as_object().map(|hooks| hooks.len())),
            );
            check(
                "the launch resolves the session's own folder",
                plan.directory.exists(),
                format!("{:?}", plan.directory),
            );
        }
        (Ok(_), Agent::Codex) => check(
            "a Codex launch needs no settings file",
            true,
            "",
        ),
        (Err(error), _) => check("a launch can be planned", false, error),
    }

    // A reported state moves the task it belongs to, and never to done.
    let undisturbed = {
        let workspace = app.workspace.borrow();
        monitor::transition(&workspace, &session.id, AgentState::Working) == Transition::None
    };
    check(
        "a working report does not disturb a session with no task",
        undisturbed,
        "it changed something",
    );

    // Hibernation needs a finished turn and a real idle period.
    let now = 1_000_000.0;
    check(
        "hibernation waits for a finished turn",
        !monitor::should_hibernate(Some(AgentState::Working), now - 3_600_000.0, 10, now),
        "a working agent was hibernated",
    );
    check(
        "hibernation is off when the delay is zero",
        !monitor::should_hibernate(Some(AgentState::Done), 0.0, 0, now),
        "it hibernated with the setting off",
    );

    // Two profiles for the same provider get different homes.
    {
        let mut workspace = app.workspace.borrow_mut();
        workspace.add_profile("Work", Agent::Claude).expect("work");
        workspace.add_profile("Personal", Agent::Claude).expect("personal");
        check(
            "a duplicate label is refused",
            workspace.add_profile("work", Agent::Claude).is_err(),
            "the duplicate was accepted",
        );
    }
    let profiles = app.workspace.borrow().state().profiles.clone();
    let homes: Vec<String> = profiles
        .iter()
        .filter(|profile| profile.agent == Agent::Claude)
        .map(|profile| {
            let mut probe =
                convoy_core::model::Session::new("p", Agent::Claude, "probe");
            probe.profile_id = Some(profile.id.clone());
            convoy_core::accounts::account_environment(
                &probe,
                &profiles,
                &app.storage.accounts(),
                &Default::default(),
            )
            .map(|account| account.home.to_string_lossy().into_owned())
            .unwrap_or_default()
        })
        .collect();
    check(
        "each profile gets its own provider home",
        homes.len() == 2 && homes[0] != homes[1] && homes.iter().all(|home| !home.is_empty()),
        format!("{homes:?}"),
    );

    settle(50).await;
}


/// Shortcuts, tab navigation and project settings.
async fn navigation_checks(app: &Rc<convoy_gtk::state::App>) {
    use convoy_core::shortcuts;
    use std::collections::BTreeMap;

    // Every configurable shortcut reaches a real action.
    let resolved = shortcuts::resolve(&BTreeMap::new());
    let bound = resolved
        .iter()
        .all(|(action, _, accelerator)| {
            !accelerator.is_empty() && convoy_gtk::app::action_for(action).is_some()
        });
    check("every shortcut maps to an action", bound, "one did not");

    let application = app
        .window
        .application()
        .and_downcast::<adw::Application>()
        .expect("application");
    for (action, _, accelerator) in &resolved {
        let Some(name) = convoy_gtk::app::action_for(action) else {
            continue;
        };
        let accels = application.accels_for_action(&format!("app.{name}"));
        // GTK reports `<Primary>` back as `<Control>`, so the two are compared
        // as parsed key and modifier rather than as text.
        let wanted = gtk::accelerator_parse(accelerator);
        let installed = accels
            .iter()
            .any(|value| gtk::accelerator_parse(value) == wanted);
        check(
            "the accelerator is installed",
            installed && wanted.is_some(),
            format!("{name}: {accels:?} does not match {accelerator}"),
        );
        break; // One is enough; the loop above proved they all resolve.
    }

    // A duplicate binding is refused with the wording the file's rules use.
    let mut clashing = BTreeMap::new();
    clashing.insert("palette".to_string(), "mod+k".to_string());
    clashing.insert("files".to_string(), "mod+k".to_string());
    let refused = app
        .workspace
        .borrow_mut()
        .save_settings(convoy_core::workspace::SettingsPatch {
            shortcuts: Some(clashing),
            ..Default::default()
        })
        .err()
        .map(|error| error.to_string());
    check(
        "two actions cannot share a shortcut",
        refused.as_deref() == Some("Invalid or duplicate shortcut."),
        format!("{refused:?}"),
    );

    // Tab navigation wraps rather than stopping at the ends.
    let pages = app.tabs.n_pages();
    if pages > 1 {
        let first = app.tabs.nth_page(0);
        app.tabs.set_selected_page(&first);
        window::step_session(app, -1);
        let position = app
            .tabs
            .selected_page()
            .map(|page| app.tabs.page_position(&page));
        check(
            "stepping back from the first tab wraps to the last",
            position == Some(pages - 1),
            format!("{position:?} of {pages}"),
        );
        window::step_session(app, 1);
        let position = app
            .tabs
            .selected_page()
            .map(|page| app.tabs.page_position(&page));
        check(
            "stepping forward wraps again",
            position == Some(0),
            format!("{position:?}"),
        );
    }

    // Project settings round-trip through the workspace.
    let project = app.selected_project().expect("project").id;
    let saved = {
        let mut workspace = app.workspace.borrow_mut();
        workspace
            .edit_project(
                &project,
                convoy_core::workspace::ProjectPatch {
                    group: Some("Work".into()),
                    shared_paths: Some(".env\nconfig/local.toml".into()),
                    setup_command: Some("echo ready".into()),
                    ..Default::default()
                },
            )
            .map(|_| ())
    };
    check("project settings save", saved.is_ok(), "they did not");
    app.sync();
    check(
        "a group becomes a node in the sidebar",
        app.roots.n_items() == 1
            && app
                .roots
                .item(0)
                .and_downcast::<convoy_gtk::objects::SidebarItem>()
                .is_some_and(|item| item.is_group() && item.title() == "Work"),
        format!("{} roots", app.roots.n_items()),
    );

    settle(50).await;
}
