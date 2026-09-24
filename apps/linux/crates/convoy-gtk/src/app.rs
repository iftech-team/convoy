//! The application object: one instance, one window, and the actions the
//! window's menus and accelerators refer to.

use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::state::App;
use crate::{dialogs, paths, theme, window};

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(paths::APPLICATION_ID)
        .build();

    // A second launch raises the existing window rather than opening another:
    // two instances would write the same workspace file in turn and the last
    // one would win.
    let app: Rc<RefCell<Option<Rc<App>>>> = Rc::new(RefCell::new(None));

    application.connect_activate({
        let app = app.clone();
        move |application| {
            if let Some(existing) = app.borrow().as_ref() {
                existing.window.present();
                return;
            }
            let Some(built) = window::build(application, paths::storage()) else {
                return;
            };
            theme::apply(&built);
            arm_exit_timer(application);
            *app.borrow_mut() = Some(built);
        }
    });

    application.run()
}

/// An action the menus and accelerators refer to: its name, its shortcuts and
/// what it does.
type Entry = (&'static str, &'static [&'static str], fn(&Rc<App>));

/// Called from `window::build`, so a window always arrives with its actions
/// and accelerators attached however it was created.
pub fn register_actions(application: &adw::Application, app: &Rc<App>) {
    let entries: [Entry; 12] = [
        ("open-folder", &["<Primary>o"], dialogs::open_folder),
        ("new-session", &[], dialogs::new_session),
        ("files", &[], crate::files_ui::open),
        ("specs", &["<Primary>t"], crate::planning_ui::open),
        ("palette", &[], crate::palette::open),
        ("next-session", &[], |app| {
            crate::window::step_session(app, 1)
        }),
        ("previous-session", &[], |app| {
            crate::window::step_session(app, -1)
        }),
        ("focus-search", &[], crate::window::focus_search),
        ("import-history", &[], dialogs::import_history),
        ("activity", &[], dialogs::activity),
        ("settings", &[], dialogs::settings),
        ("about", &[], dialogs::about),
    ];

    for (name, accels, handler) in entries {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.connect_activate({
            let app = app.clone();
            move |_, _| handler(&app)
        });
        application.add_action(&action);
        if !accels.is_empty() {
            application.set_accels_for_action(&format!("app.{name}"), accels);
        }
    }

    let stop = gtk::gio::SimpleAction::new("stop-session", None);
    stop.connect_activate({
        let app = app.clone();
        move |_, _| stop_selected(&app)
    });
    application.add_action(&stop);

    register_project_actions(application, app);
    apply_shortcuts(application, app);

    // Activated by clicking a notification, with the session id as its target.
    let focus =
        gtk::gio::SimpleAction::new(crate::notify::FOCUS_ACTION, Some(glib::VariantTy::STRING));
    focus.connect_activate({
        let app = app.clone();
        move |_, target| {
            if let Some(id) = target.and_then(|value| value.str()) {
                crate::notify::focus(&app, id);
            }
        }
    });
    application.add_action(&focus);

    let quit = gtk::gio::SimpleAction::new("quit", None);
    quit.connect_activate({
        let app = app.clone();
        move |_, _| app.window.close()
    });
    application.add_action(&quit);
    application.set_accels_for_action("app.quit", &["<Primary>q"]);
}

/// Lets a headless check prove the app starts, presents its window and shuts
/// down cleanly. Killing it would prove only the first of those.
fn arm_exit_timer(application: &adw::Application) {
    let Some(milliseconds) = std::env::var("CONVOY_EXIT_AFTER_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return;
    };
    glib::timeout_add_local_once(std::time::Duration::from_millis(milliseconds), {
        let application = application.clone();
        move || application.quit()
    });
}

/// Actions the sidebar's context menu offers, each taking the project id as
/// its target so one action serves every row.
fn register_project_actions(application: &adw::Application, app: &Rc<App>) {
    type ProjectEntry = (&'static str, fn(&Rc<App>, &str));
    let entries: [ProjectEntry; 6] = [
        ("project-new-session", |app, id| {
            select_project(app, id);
            dialogs::new_session(app);
        }),
        ("project-settings", |app, id| {
            dialogs::project_settings(app, id)
        }),
        ("project-reconnect", |app, id| {
            dialogs::reconnect_project(app, id)
        }),
        ("project-remove", |app, id| dialogs::remove_project(app, id)),
        ("project-show-files", |app, id| show_in_files(app, id)),
        ("project-copy-path", |app, id| copy_path(app, id)),
    ];
    for (name, handler) in entries {
        let action = gtk::gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        action.connect_activate({
            let app = app.clone();
            move |_, target| {
                if let Some(id) = target.and_then(|value| value.str()) {
                    handler(&app, id);
                }
            }
        });
        application.add_action(&action);
    }
}

fn select_project(app: &Rc<App>, project_id: &str) {
    if app.selection.borrow().project.as_deref() == Some(project_id) {
        return;
    }
    app.selection.borrow_mut().project = Some(project_id.to_string());
    app.selection.borrow_mut().session = None;
    crate::window::rebuild_tabs(app);
    app.sync();
}

/// Opens the project folder in the desktop's own file manager.
fn show_in_files(app: &Rc<App>, project_id: &str) {
    let Ok(project) = app.workspace.borrow().project(project_id).cloned() else {
        return;
    };
    let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(&project.path)));
    launcher.launch(Some(&app.window), gtk::gio::Cancellable::NONE, {
        let app = app.clone();
        move |result| {
            if let Err(error) = result {
                app.error(error);
            }
        }
    });
}

fn copy_path(app: &Rc<App>, project_id: &str) {
    let Ok(project) = app.workspace.borrow().project(project_id).cloned() else {
        return;
    };
    let path = project.path.to_string_lossy().into_owned();
    app.window.clipboard().set_text(&path);
    app.toasts.add_toast(
        adw::Toast::builder()
            .title(format!("Copied {path}"))
            .build(),
    );
}

/// Binds the shortcuts the workspace holds, falling back to the defaults.
/// The stored notation is the Electron one, so both builds read the same file.
pub fn apply_shortcuts(application: &adw::Application, app: &Rc<App>) {
    let saved = app.workspace.borrow().settings().shortcuts.clone();
    for (action, _, accelerator) in convoy_core::shortcuts::resolve(&saved) {
        let Some(name) = action_for(action) else {
            continue;
        };
        if accelerator.is_empty() {
            continue;
        }
        application.set_accels_for_action(&format!("app.{name}"), &[&accelerator]);
    }
}

/// The action each configurable shortcut triggers.
pub fn action_for(shortcut: &str) -> Option<&'static str> {
    match shortcut {
        "palette" => Some("palette"),
        "newSession" => Some("new-session"),
        "files" => Some("files"),
        "next" => Some("next-session"),
        "previous" => Some("previous-session"),
        "settings" => Some("settings"),
        "search" => Some("focus-search"),
        _ => None,
    }
}

fn stop_selected(app: &Rc<App>) {
    if let Some(id) = app.selection.borrow().session.clone() {
        crate::terminal::stop(app, &id);
    }
}
