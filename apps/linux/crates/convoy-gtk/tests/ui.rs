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
            run_checks(&app);
            application.quit();
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
}
