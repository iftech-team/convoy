//! Additive migration to schema 3.
//!
//! Older previews are upgraded by filling in collections that did not exist
//! yet; nothing is renamed or dropped, and unknown fields survive through the
//! `unknown` maps on every record. A task left in `building` means the app was
//! killed while its session ran, so it is marked failed rather than resumed
//! silently.

use super::model::{State, TaskStatus};
use super::RECOVERED_TASK_ERROR;

pub fn migrate(state: &mut State) {
    state.schema_version = 3;
    for task in &mut state.tasks {
        if task.status == TaskStatus::Building {
            task.status = TaskStatus::Failed;
            task.last_error = Some(RECOVERED_TASK_ERROR.to_string());
        }
    }
}
