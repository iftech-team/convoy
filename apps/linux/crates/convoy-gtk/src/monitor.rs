//! The two-second check of what each running agent is doing.
//!
//! Everything it decides comes from `convoy_core::monitor`; this module only
//! supplies the clock, the notifications and the stopping.

use convoy_core::monitor::{self, AgentState, Transition};
use std::rc::Rc;
use std::time::Duration;

use crate::state::App;
use crate::{notify, terminal};

const INTERVAL: Duration = Duration::from_secs(2);

pub fn start(app: &Rc<App>) {
    glib::timeout_add_local(INTERVAL, {
        let app = app.clone();
        move || {
            tick(&app);
            glib::ControlFlow::Continue
        }
    });
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as f64)
        .unwrap_or(0.0)
}

fn tick(app: &Rc<App>) {
    let now = now_ms();
    let running: Vec<String> = app
        .views
        .borrow()
        .iter()
        .filter(|(_, view)| view.running())
        .map(|(id, _)| id.clone())
        .collect();

    let mut changed = false;
    for id in &running {
        let Some(view) = app.views.borrow().get(id).cloned() else {
            continue;
        };
        let Some(status) =
            monitor::read_status(&app.storage.telemetry(), id, view.status_at.get(), now)
        else {
            continue;
        };
        let moved = view.agent_state.get() != Some(status.state);
        view.status_at.set(status.at);
        view.agent_state.set(Some(status.state));

        if moved && status.state.notable() {
            let kind = match status.state {
                AgentState::Done => convoy_core::model::ActivityKind::Done,
                _ => convoy_core::model::ActivityKind::Waiting,
            };
            if let Err(error) =
                app.workspace
                    .borrow_mut()
                    .record(kind, id, status.state.detail())
            {
                app.error(error);
            }
            let title = app
                .workspace
                .borrow()
                .session(id)
                .map(|session| session.title.clone())
                .unwrap_or_default();
            notify::send(app, id, status.state.headline(), &title);
        }

        let transition = monitor::transition(&app.workspace.borrow(), id, status.state);
        if transition != Transition::None {
            if let Err(error) = monitor::apply(&mut app.workspace.borrow_mut(), id, transition) {
                app.error(error);
            }
            // A finished turn is what moves a task on to review, and that is
            // what lets the queue continue.
            if transition == Transition::Review {
                crate::queue_ui::after_exit(
                    app,
                    id,
                    convoy_core::session::ExitCause::Exited(0),
                );
            }
        }
        changed = true;
    }

    // Hibernation: stop a Claude session that said it finished and then sat
    // idle. Resuming it stays an explicit action.
    let idle_minutes = app.workspace.borrow().settings().hibernate_minutes;
    if idle_minutes > 0 {
        for id in &running {
            let Some(view) = app.views.borrow().get(id).cloned() else {
                continue;
            };
            if view.stopping.get() || view.hibernating.get() {
                continue;
            }
            if monitor::should_hibernate(
                view.agent_state.get(),
                view.status_at.get(),
                idle_minutes,
                now,
            ) {
                view.hibernating.set(true);
                if let Err(error) = app.workspace.borrow_mut().record(
                    convoy_core::model::ActivityKind::Hibernated,
                    id,
                    "Stopped after completed-turn idle timeout",
                ) {
                    app.error(error);
                }
                terminal::stop(app, id);
                changed = true;
            }
        }
    }

    if changed {
        app.sync();
    }
}
