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
            register_actions(application, &built);
            arm_exit_timer(application);
            *app.borrow_mut() = Some(built);
        }
    });

    application.run()
}

/// An action the menus and accelerators refer to: its name, its shortcuts and
/// what it does.
type Entry = (&'static str, &'static [&'static str], fn(&Rc<App>));

fn register_actions(application: &adw::Application, app: &Rc<App>) {
    let entries: [Entry; 5] = [
        ("open-folder", &["<Primary>o"], dialogs::open_folder),
        ("new-session", &["<Primary>n"], dialogs::new_session),
        ("settings", &["<Primary>comma"], dialogs::settings),
        ("about", &[], dialogs::about),
        ("stop-session", &[], stop_selected),
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

fn stop_selected(app: &Rc<App>) {
    if let Some(id) = app.selection.borrow().session.clone() {
        crate::terminal::stop(app, &id);
    }
}
