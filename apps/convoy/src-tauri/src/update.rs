//! Updates from the latest GitHub release.
//!
//! A newer version is downloaded in the background and checked against the
//! public key in tauri.conf.json, then waits. Nothing is installed until the
//! person says so: restarting stops every running agent, and that is never a
//! decision to make for them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Soon after launch, then every six hours: a release is cut a few times a
/// week at most.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Default)]
pub struct Updates {
    ready: Mutex<Option<Ready>>,
    checking: AtomicBool,
}

/// A verified download, held until it is installed.
struct Ready {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Found {
    pub version: String,
    pub notes: String,
}

impl Ready {
    fn found(&self) -> Found {
        Found {
            version: self.update.version.clone(),
            notes: self.update.body.clone().unwrap_or_default(),
        }
    }
}

impl Updates {
    fn found(&self) -> Option<Found> {
        self.ready.lock().ok()?.as_ref().map(Ready::found)
    }
}

/// Checks in the background. A development build never does: it would find
/// every release newer than itself and fetch it.
pub fn start_worker<R: Runtime>(app: AppHandle<R>) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::Builder::new()
        .name("convoy-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                // Offline or between a release and its update manifest: the
                // next pass tries again, and nobody needs telling.
                if let Err(error) = tauri::async_runtime::block_on(fetch(&app)) {
                    eprintln!("convoy: update check failed: {error}");
                }
                std::thread::sleep(INTERVAL);
            }
        })
        .ok();
}

/// Looks for a newer release and downloads it. Says what is ready, if
/// anything; a check already under way answers with what is ready now.
async fn fetch<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Found>, String> {
    let updates = app.state::<Updates>();
    if updates.checking.swap(true, Ordering::SeqCst) {
        return Ok(updates.found());
    }
    let result = download(app, &updates).await;
    updates.checking.store(false, Ordering::SeqCst);
    result
}

async fn download<R: Runtime>(
    app: &AppHandle<R>,
    updates: &Updates,
) -> Result<Option<Found>, String> {
    let checked = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    let Some(update) = checked else {
        return Ok(updates.found());
    };
    if updates
        .found()
        .is_some_and(|found| found.version == update.version)
    {
        return Ok(updates.found());
    }
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    let ready = Ready { update, bytes };
    let found = ready.found();
    *updates.ready.lock().map_err(|_| "Updates unavailable.")? = Some(ready);
    let _ = app.emit("update:ready", &found);
    Ok(Some(found))
}

/// What is downloaded and waiting, for a window opened after it arrived.
#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> Option<Found> {
    updates.found()
}

/// Settings' "Check for updates". Unlike the background check, a failure is
/// the answer.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Found>, String> {
    if cfg!(debug_assertions) {
        return Err("A development build does not update itself.".into());
    }
    fetch(&app).await
}

/// Installs what is ready and restarts into it. The window has already asked
/// about running agents; they are stopped here so an exit code is recorded
/// for each, as quitting does.
#[tauri::command]
pub async fn update_install(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    let ready = updates
        .ready
        .lock()
        .map_err(|_| "Updates unavailable.")?
        .take()
        .ok_or("No update is ready.")?;
    let handle = app.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        // Windows hands over to the installer and exits inside `install`, so
        // the agents are stopped first there. Elsewhere a failed install
        // leaves them running.
        if cfg!(windows) {
            crate::stop_agents(&handle);
        }
        match ready.update.install(&ready.bytes) {
            Ok(()) => {
                crate::stop_agents(&handle);
                Ok(())
            }
            Err(error) => Err((error.to_string(), Box::new(ready))),
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err((error, ready)) = installed {
        // Kept, so the next try needs no second download.
        if let Ok(mut slot) = updates.ready.lock() {
            *slot = Some(*ready);
        }
        return Err(format!("The update could not be installed: {error}"));
    }
    app.request_restart();
    Ok(())
}
