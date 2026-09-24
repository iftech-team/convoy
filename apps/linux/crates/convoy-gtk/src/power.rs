//! Keeping the machine awake while agents work.
//!
//! This prevents idle suspension only. Closing the lid or choosing Suspend
//! still sleeps the machine, and the running agents go with it.

use adw::prelude::*;
use std::rc::Rc;

use crate::state::App;
use convoy_core::model::KeepAwake;

pub fn refresh(app: &Rc<App>) {
    let mode = app.workspace.borrow().settings().keep_awake;
    let running = app
        .views
        .borrow()
        .values()
        .any(|view| view.running());
    let wanted = match mode {
        KeepAwake::Off => false,
        KeepAwake::Always => true,
        KeepAwake::Sessions => running,
    };

    let current = app.wake_lock.get();
    if wanted && current == 0 {
        if let Some(application) = app.window.application() {
            let cookie = application.inhibit(
                Some(&app.window),
                gtk::ApplicationInhibitFlags::SUSPEND | gtk::ApplicationInhibitFlags::IDLE,
                Some("An agent session is running"),
            );
            app.wake_lock.set(cookie);
        }
    } else if !wanted && current != 0 {
        if let Some(application) = app.window.application() {
            application.uninhibit(current);
        }
        app.wake_lock.set(0);
    }
}
