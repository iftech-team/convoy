//! Status sync: the thread that tells trackers what changed, and the
//! commands the project settings and the task form use.
//!
//! The rules live in `convoy_core::integrations::sync`; what changed is noted
//! on each task by the core as it is saved. This only sends the requests,
//! off the main thread, and records how they went.

use convoy_core::integrations::sync::{self, SyncState, TrackerStatus};
use convoy_core::model::TaskStatus;
use convoy_core::ConvoyError;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::commands::Workspace;
use crate::tasks::{connection, send, Trackers};

/// How often pending changes are looked for; a change waits at most this.
const INTERVAL: Duration = Duration::from_secs(3);

pub fn start_worker<R: Runtime>(app: AppHandle<R>) {
    std::thread::Builder::new()
        .name("convoy-tracker-sync".into())
        .spawn(move || loop {
            std::thread::sleep(INTERVAL);
            pass(&app);
        })
        .ok();
}

/// One pass over the tasks with something to tell their tracker.
fn pass<R: Runtime>(app: &AppHandle<R>) {
    let (Some(workspace), Some(trackers)) =
        (app.try_state::<Workspace>(), app.try_state::<Trackers>())
    else {
        return;
    };
    let pending = workspace
        .with(|workspace| {
            Ok(workspace
                .state()
                .tasks
                .iter()
                .filter(|task| {
                    task.sync
                        .as_ref()
                        .is_some_and(|sync| sync.pending.is_some())
                })
                .cloned()
                .collect::<Vec<_>>())
        })
        .unwrap_or_default();
    for task in pending {
        let Some(sent) = task.sync.as_ref().and_then(|sync| sync.pending.clone()) else {
            continue;
        };
        let link = trackers
            .with_links(&workspace, |links| Ok(links.get(&task.project_id).cloned()))
            .ok()
            .flatten();
        // Not linked, or linked elsewhere, or sync off: nothing to tell, the
        // note goes. A later change is noted again.
        let result = link
            .filter(|link| {
                task.source.is_some() && link.sync.as_ref().is_some_and(|sync| sync.enabled)
            })
            .and_then(|link| {
                let tracker = connection(&trackers, &workspace, &link.connection_id).ok()?;
                let kind_matches = task
                    .source
                    .as_ref()
                    .is_some_and(|source| source.tracker == tracker.kind);
                kind_matches.then(|| {
                    sync::push(&tracker, &link, &task, &mut |request| {
                        send(&tracker, request).map_err(ConvoyError::message)
                    })
                })?
            });
        let recorded = workspace.act(|workspace| {
            workspace.update(|state| {
                if let Some(saved) = state.tasks.iter_mut().find(|saved| saved.id == task.id) {
                    saved
                        .sync
                        .get_or_insert_with(SyncState::default)
                        .settle(&sent, result.clone());
                }
                Ok(())
            })?;
            Ok(())
        });
        if recorded.is_ok() {
            let _ = app.emit("task:sync", &task.id);
        }
    }
}

#[derive(Serialize)]
pub struct StatusChoices {
    statuses: Vec<TrackerStatus>,
    /// What each Convoy status would map to, for a sync not set up yet.
    suggested: BTreeMap<TaskStatus, TrackerStatus>,
}

/// The statuses a project's linked tracker offers, and a suggested mapping.
#[tauri::command]
pub async fn tracker_statuses(
    project_id: String,
    trackers: State<'_, Trackers>,
    workspace: State<'_, Workspace>,
) -> Result<StatusChoices, String> {
    let Some(link) =
        trackers.with_links(&workspace, |links| Ok(links.get(&project_id).cloned()))?
    else {
        return Err("Link the project to an integration first.".into());
    };
    let tracker = connection(&trackers, &workspace, &link.connection_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let statuses = sync::list_statuses(&tracker, &link, &mut |request| {
            send(&tracker, request).map_err(ConvoyError::message)
        })
        .map_err(|error| error.to_string())?;
        let suggested = sync::suggest_map(&statuses);
        Ok(StatusChoices {
            statuses,
            suggested,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Sends what the last push failed to deliver again.
#[tauri::command]
pub fn task_sync_retry(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| {
        workspace.update(|state| {
            let retried = state
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .and_then(|task| task.sync.as_mut())
                .is_some_and(SyncState::retry);
            convoy_core::ensure!(retried, "Nothing to send again.");
            Ok(())
        })?;
        Ok(())
    })
}
