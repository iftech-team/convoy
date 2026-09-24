//! Desktop notifications.
//!
//! Sent only when the setting is on and the window is not focused: an agent
//! asking for attention while its terminal is already on screen is not news.
//! Clicking a notification raises the window and selects that session.

use adw::prelude::*;
use std::rc::Rc;

use crate::state::App;

/// The action a notification activates. Registered on the application so the
/// click works even if the window was closed and reopened.
pub const FOCUS_ACTION: &str = "focus-session";

pub fn should_notify(app: &Rc<App>) -> bool {
    app.workspace.borrow().settings().notifications && !app.window.is_active()
}

pub fn send(app: &Rc<App>, session_id: &str, headline: &str, body: &str) {
    if !should_notify(app) {
        return;
    }
    let Some(application) = app.window.application() else {
        return;
    };
    let notification = gtk::gio::Notification::new(headline);
    notification.set_body(Some(body));
    notification.set_default_action_and_target_value(
        &format!("app.{FOCUS_ACTION}"),
        Some(&session_id.to_variant()),
    );
    // One notification per session: a chatty agent replaces its own rather
    // than stacking up.
    application.send_notification(Some(session_id), &notification);
}

/// Raises the window and selects the session a notification referred to.
pub fn focus(app: &Rc<App>, session_id: &str) {
    app.window.present();
    let page = app
        .views
        .borrow()
        .get(session_id)
        .and_then(|view| view.page.borrow().clone());
    match page {
        Some(page) => app.tabs.set_selected_page(&page),
        None => {
            // The session belongs to another project; switch to it first.
            let project = app
                .workspace
                .borrow()
                .session(session_id)
                .ok()
                .map(|session| session.project_id.clone());
            if let Some(project) = project {
                app.selection.borrow_mut().project = Some(project);
                app.selection.borrow_mut().session = Some(session_id.to_string());
                crate::window::rebuild_tabs(app);
                app.sync();
            }
        }
    }
}
