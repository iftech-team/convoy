//! Convoy for Windows and Linux.
//!
//! The rules live in `convoy-core`, unchanged from the GTK build: it was
//! written with no toolkit dependency precisely so the front end could be
//! replaced without touching them. This crate is the thin layer between that
//! core and a web view — commands in, events out.

mod commands;
mod monitor;
mod power;
mod pty;

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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Arc::new(pty::Terminals::new()))
        .manage(commands::Workspace::new())
        .manage(power::KeepAwake::default())
        .manage(monitor::Reports::default())
        .invoke_handler(tauri::generate_handler![
            commands::workspace_read,
            commands::settings_read,
            commands::settings_save,
            commands::project::project_open,
            commands::project::project_edit,
            commands::project::project_detail,
            commands::project::project_reconnect,
            commands::project::project_remove,
            commands::project::path_reveal,
            commands::session::sessions_for,
            commands::session::session_detail,
            commands::session::session_create,
            commands::session::session_edit,
            commands::session::session_archive,
            commands::session::session_pin,
            commands::session::session_recover,
            commands::session::session_start,
            commands::session::session_stop,
            commands::session::session_hibernate,
            commands::session::session_output,
            commands::session::terminal_write,
            commands::session::terminal_resize,
            commands::session::terminal_paste,
            commands::session::review_brief,
            commands::session::review_create,
            commands::session::review_builder,
            commands::session::git_status,
            commands::session::worktree_create,
            commands::session::worktree_setup,
            commands::session::worktree_plan_remove,
            commands::session::worktree_remove,
            commands::session::quick_commands_read,
            commands::session::quick_command_save,
            commands::session::quick_command_remove,
            commands::session::quick_command_send,
            commands::files::files_snapshot,
            commands::files::files_read,
            commands::files::files_hunks,
            commands::files::files_mutate,
            commands::files::files_trash,
            commands::files::commit_generate,
            commands::files::pr_create,
            commands::planning::planning_read,
            commands::planning::spec_save,
            commands::planning::spec_approve,
            commands::planning::spec_markdown,
            commands::planning::task_save,
            commands::planning::task_status,
            commands::planning::task_prepare,
            commands::planning::queue_summary,
            commands::planning::queue_after_exit,
            commands::integrations::profiles_read,
            commands::integrations::profile_add,
            commands::integrations::profile_remove,
            commands::integrations::transcripts_scan,
            commands::integrations::transcripts_import,
            commands::integrations::activity_read,
            commands::integrations::usage_read,
            monitor::monitor_tick,
            power::keep_awake,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Convoy");
}
