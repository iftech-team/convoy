//! The session menu.
//!
//! Thirteen entries, each a `gio::SimpleAction` in the window's `session`
//! group so the menu, keyboard shortcuts and any future command palette all
//! reach the same code. What each one may do is decided in `convoy-core`;
//! whether it is offered at all is decided by `App::refresh_selection`.

use adw::prelude::*;
use convoy_core::model::{Agent, Session};
use convoy_core::workspace::SessionPatch;
use std::rc::Rc;

use crate::state::{background, App};
use crate::{dialogs, terminal, window};

pub fn menu() -> gtk::gio::Menu {
    let menu = gtk::gio::Menu::new();

    let session = gtk::gio::Menu::new();
    session.append(Some("Edit session…"), Some("session.edit"));
    session.append(Some("Pin or unpin"), Some("session.pin"));
    session.append(Some("Archive session"), Some("session.archive"));
    session.append(Some("Fresh recovery session"), Some("session.recover"));
    menu.append_section(None, &session);

    let review = gtk::gio::Menu::new();
    review.append(Some("Start review…"), Some("session.review"));
    review.append(Some("Send feedback to builder…"), Some("session.feedback"));
    menu.append_section(None, &review);

    let inspect = gtk::gio::Menu::new();
    inspect.append(Some("Saved output…"), Some("session.history"));
    inspect.append(Some("Usage limits…"), Some("session.usage"));
    inspect.append(Some("Refresh Git status"), Some("session.git-status"));
    menu.append_section(None, &inspect);

    let worktree = gtk::gio::Menu::new();
    worktree.append(Some("Create worktree…"), Some("session.worktree"));
    worktree.append(Some("Remove worktree…"), Some("session.remove-worktree"));
    menu.append_section(None, &worktree);

    let split = gtk::gio::Menu::new();
    split.append(Some("Open split terminal…"), Some("session.split"));
    split.append(Some("Close split view"), Some("session.unsplit"));
    menu.append_section(None, &split);

    menu
}

/// A menu entry: the action's name and what it does with the selected session.
type Entry = (&'static str, fn(&Rc<App>, &Session));

pub fn register(app: &Rc<App>) {
    let entries: [Entry; 13] = [
        ("edit", |app, session| dialogs::edit_session(app, session)),
        ("pin", pin),
        ("archive", archive),
        ("recover", recover),
        ("review", |app, session| dialogs::start_review(app, session)),
        ("feedback", |app, session| {
            dialogs::send_feedback(app, session)
        }),
        ("history", |app, session| {
            dialogs::saved_output(app, session)
        }),
        ("usage", |app, session| dialogs::usage(app, session)),
        ("git-status", git_status),
        ("worktree", |app, session| {
            dialogs::create_worktree(app, session)
        }),
        ("remove-worktree", |app, session| {
            dialogs::remove_worktree(app, session)
        }),
        ("split", |app, session| dialogs::open_split(app, session)),
        ("unsplit", |app, _| window::close_split(app)),
    ];

    for (name, handler) in entries {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.connect_activate({
            let app = app.clone();
            move |_, _| {
                let Some(session) = app.selected_session() else {
                    return;
                };
                handler(&app, &session);
            }
        });
        app.actions.add_action(&action);
    }

    // Quick commands carry the command id as the action target, so one action
    // serves the whole menu however it changes.
    let quick = gtk::gio::SimpleAction::new("quick", Some(glib::VariantTy::STRING));
    quick.connect_activate({
        let app = app.clone();
        move |_, target| {
            let (Some(session), Some(command)) = (
                app.selected_session(),
                target.and_then(|value| value.str().map(str::to_string)),
            ) else {
                return;
            };
            let found = {
                let workspace = app.workspace.borrow();
                convoy_core::review::quick_command(&workspace, &session.id, &command)
            };
            match found {
                Ok((text, submit)) => {
                    insert(&app, &session.id, &text, submit);
                }
                Err(error) => app.error(error),
            }
        }
    });
    app.actions.add_action(&quick);

    let manage = gtk::gio::SimpleAction::new("manage-commands", None);
    manage.connect_activate({
        let app = app.clone();
        move |_, _| dialogs::quick_commands(&app)
    });
    app.actions.add_action(&manage);
}

fn pin(app: &Rc<App>, session: &Session) {
    let patch = SessionPatch {
        pinned: Some(!session.is_pinned()),
        ..Default::default()
    };
    apply(app, &session.id, patch);
    window::rebuild_tabs(app);
    app.sync();
}

fn archive(app: &Rc<App>, session: &Session) {
    let patch = SessionPatch {
        archived: Some(true),
        ..Default::default()
    };
    apply(app, &session.id, patch);
    window::rebuild_tabs(app);
    app.sync();
}

fn apply(app: &Rc<App>, id: &str, patch: SessionPatch) {
    let running = app
        .views
        .borrow()
        .get(id)
        .is_some_and(|view| view.running());
    let outcome = {
        let mut workspace = app.workspace.borrow_mut();
        workspace.edit_session(id, patch, running).map(|_| ())
    };
    if let Err(error) = outcome {
        app.error(error);
    }
}

/// A recovery session keeps the folder and the notes but starts a fresh
/// conversation: the old one is left intact rather than resumed into.
fn recover(app: &Rc<App>, session: &Session) {
    let outcome = {
        let mut workspace = app.workspace.borrow_mut();
        workspace.recover_session(&session.id).map(|_| ())
    };
    match outcome {
        Ok(()) => {
            window::rebuild_tabs(app);
            app.sync();
        }
        Err(error) => app.error(error),
    }
}

fn git_status(app: &Rc<App>, session: &Session) {
    let directory = match convoy_core::session::directory_for(&app.workspace.borrow(), &session.id)
    {
        Ok(directory) => directory,
        Err(error) => {
            app.error(error);
            return;
        }
    };
    background(
        app,
        move || convoy_core::Git::default().status(&directory),
        |app, status| {
            app.git_status.set_label(&format!(
                "{} · {} changed",
                status.branch, status.changed_files
            ));
        },
    );
}

/// The label shown next to a session, refreshed whenever the selection moves.
pub fn refresh_git_status(app: &Rc<App>) {
    let Some(session) = app.selected_session() else {
        app.git_status.set_label("");
        return;
    };
    git_status(app, &session);
}

/// The agent a review of `session` would use: the other one.
pub fn reviewer_for(session: &Session) -> Agent {
    match session.agent {
        Agent::Claude => Agent::Codex,
        Agent::Codex => Agent::Claude,
    }
}

/// Text destined for a session's terminal. The session must be running:
/// inserting into a stopped agent would silently do nothing.
pub fn insert(app: &Rc<App>, id: &str, text: &str, submit: bool) -> bool {
    let running = app
        .views
        .borrow()
        .get(id)
        .is_some_and(|view| view.running() && !view.stopping.get());
    if !running {
        app.error("Start or resume the target session first.");
        return false;
    }
    terminal::paste(app, id, text, submit);
    true
}
