//! Driving the task queue.
//!
//! The rules are in `convoy_core::queue`; this module supplies the two things
//! it cannot know — which sessions have a live process, and how to start one.
//! A queue lives in memory only, so closing the app stops it and a restart
//! never resumes it on its own.

use adw::prelude::*;
use convoy_core::queue::{self, Completion, Step};
use convoy_core::session::ExitCause;
use std::rc::Rc;

use crate::state::{background, App};
use crate::{terminal, window};

pub fn is_running(app: &Rc<App>, project_id: &str) -> bool {
    app.queues.borrow().contains(project_id)
}

/// Shows exactly what the queue would do, then starts it if the user agrees.
/// Running agents costs money and can publish, so this is never implicit.
pub fn confirm_and_start(app: &Rc<App>, project_id: &str) {
    let summary = queue::summary(&app.workspace.borrow(), project_id);
    if summary.is_empty() {
        app.error("No queued tasks.");
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading(format!("Run {} queued tasks?", summary.len()))
        .body(format!(
            "{}\n\nRuns the installed agents. Publishing happens only for tasks \
             configured for it. A failure pauses the queue.",
            summary.join("\n")
        ))
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("run", "Run queue");
    dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let app = app.clone();
        let project_id = project_id.to_string();
        move |_, response| {
            if response != "run" {
                return;
            }
            app.queues.borrow_mut().insert(project_id.clone());
            advance(&app, &project_id);
        }
    });
    dialog.present(Some(&app.window));
}

pub fn stop(app: &Rc<App>, project_id: &str) {
    app.queues.borrow_mut().remove(project_id);
    app.sync();
}

/// Starts the next queued task, if the queue is still running and nothing is
/// building.
pub fn advance(app: &Rc<App>, project_id: &str) {
    if !is_running(app, project_id) {
        return;
    }
    let running = |id: &str| app.views.borrow().get(id).is_some_and(|view| view.running());
    let step = queue::next_step(&app.workspace.borrow(), project_id, &running);
    match step {
        Step::Waiting => {}
        Step::Empty => stop(app, project_id),
        Step::Run { task_id, .. } => prepare_and_run(app, project_id, &task_id),
    }
}

fn prepare_and_run(app: &Rc<App>, project_id: &str, task_id: &str) {
    let prepared = {
        let mut workspace = app.workspace.borrow_mut();
        workspace.prepare_task(task_id, None).map(|_| ())
    };
    if let Err(error) = prepared {
        app.error(error);
        stop(app, project_id);
        return;
    }

    let session_id = app
        .workspace
        .borrow()
        .state()
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .and_then(|task| task.session_id.clone());
    let Some(session_id) = session_id else {
        stop(app, project_id);
        return;
    };

    window::rebuild_tabs(app);
    app.sync();

    // A pull-request task publishes, so it never shares the project checkout.
    match queue::needs_worktree(&app.workspace.borrow(), task_id) {
        Some(branch) => make_worktree_then_start(app, project_id, &session_id, &branch),
        None => terminal::start(app, &session_id),
    }
}

fn make_worktree_then_start(app: &Rc<App>, project_id: &str, session_id: &str, branch: &str) {
    let plan = {
        let workspace = app.workspace.borrow();
        let busy = |id: &str| {
            app.busy.borrow().contains(id)
                || app.views.borrow().get(id).is_some_and(|view| view.running())
        };
        convoy_core::worktree::plan_create(&workspace, session_id, branch, &busy)
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            app.error(error);
            stop(app, project_id);
            return;
        }
    };

    let root = app.storage.worktrees();
    let project_path = plan.project_path.clone();
    let branch = plan.branch.clone();
    background(
        app,
        move || convoy_core::Git::default().create_worktree(&project_path, &root, &branch),
        {
            let plan = plan.clone();
            let project_id = project_id.to_string();
            let session_id = session_id.to_string();
            move |app, directory: std::path::PathBuf| {
                let recorded = convoy_core::worktree::record_create(
                    &mut app.workspace.borrow_mut(),
                    &plan.session_id,
                    &directory,
                    &plan.branch,
                );
                if let Err(error) = recorded {
                    app.error(error);
                    stop(app, &project_id);
                    return;
                }
                app.sync();
                if is_running(app, &project_id) {
                    terminal::start(app, &session_id);
                }
            }
        },
    );
}

/// What a finished session means for the queue. Called after the exit has
/// already been recorded, so the task's status is settled.
pub fn after_exit(app: &Rc<App>, session_id: &str, cause: ExitCause) {
    let project_id = app
        .workspace
        .borrow()
        .session(session_id)
        .ok()
        .map(|session| session.project_id.clone());
    let Some(project_id) = project_id else {
        return;
    };

    if !convoy_core::session::completed_cleanly(cause) {
        // A stop or a failure pauses the queue. Hibernation is neither.
        if cause != ExitCause::Hibernated {
            stop(app, &project_id);
        }
        return;
    }

    let output = terminal::saved_output(app, session_id);
    let completion = queue::on_clean_exit(&app.workspace.borrow(), session_id, &output);
    match completion {
        Err(error) => {
            app.error(error);
            stop(app, &project_id);
        }
        Ok(Completion::Nothing) => {}
        Ok(Completion::Superseded) => {
            if let Err(error) = queue::mark_superseded(&mut app.workspace.borrow_mut(), session_id)
            {
                app.error(error);
            }
            stop(app, &project_id);
            app.sync();
        }
        Ok(Completion::Continue) => advance(app, &project_id),
        Ok(Completion::Review(handoff)) => {
            let created = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.add_session(*handoff).map(|state| {
                    state
                        .sessions
                        .last()
                        .map(|session| session.id.clone())
                        .unwrap_or_default()
                })
            };
            match created {
                Ok(review) => {
                    window::rebuild_tabs(app);
                    app.sync();
                    terminal::start(app, &review);
                    advance(app, &project_id);
                }
                Err(error) => {
                    app.error(error);
                    stop(app, &project_id);
                }
            }
        }
    }
}
