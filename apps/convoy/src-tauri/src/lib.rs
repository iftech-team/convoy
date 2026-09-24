//! Convoy for Windows and Linux.
//!
//! The rules live in `convoy-core`, unchanged from the GTK build: it was
//! written with no toolkit dependency precisely so the front end could be
//! replaced without touching them. This crate is the thin layer between that
//! core and a web view — commands in, events out.

mod commands;
mod pty;
mod tasks;

use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Claude runs the hook once per event with no display. It must be handled
    // before anything else starts.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|value| value == "--hook") {
        if let Some(output) = arguments.get(1) {
            if let Some(line) = convoy_core::telemetry::hook::run(
                std::path::Path::new(output),
                &mut std::io::stdin().lock(),
            ) {
                print!("{line}");
            }
        }
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(pty::Terminals::new()))
        .manage(commands::Workspace::new())
        .manage(tasks::Trackers::default())
        .invoke_handler(tauri::generate_handler![
            commands::workspace_read,
            commands::project_add,
            commands::review_create,
            commands::review_feedback,
            commands::sessions_for,
            commands::settings_read,
            commands::settings_save,
            commands::session_create,
            commands::session_archive,
            commands::session_pin,
            commands::session_start,
            commands::session_stop,
            commands::terminal_write,
            commands::terminal_resize,
            tasks::integrations_list,
            tasks::integration_save,
            tasks::integration_remove,
            tasks::issues_search,
            tasks::issues_parse_keys,
            tasks::issues_import,
            tasks::tasks_for,
            tasks::task_prepare,
            tasks::task_agent,
            tasks::task_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Convoy");
}
