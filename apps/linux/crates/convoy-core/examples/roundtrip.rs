//! Loads a `workspace.json`, reports what it contains, and saves it again.
//! Used to prove a document written by another build survives a round trip
//! through this one — including fields this build does not know.

use convoy_core::workspace::{SessionPatch, Workspace};

fn main() {
    let file = std::env::args().nth(1).expect("path");
    let mut workspace = Workspace::load(&file).expect("the port must accept the file");
    let state = workspace.state();
    println!(
        "read: schema {} | projects {} | sessions {} | specs {} | tasks {} | unknown top-level keys {:?}",
        state.schema_version,
        state.projects.len(),
        state.sessions.len(),
        state.specs.len(),
        state.tasks.len(),
        state.unknown.keys().collect::<Vec<_>>()
    );
    let id = state.sessions[0].id.clone();
    workspace
        .edit_session(
            &id,
            SessionPatch {
                notes: Some("edited by the port".into()),
                ..SessionPatch::default()
            },
            false,
        )
        .expect("write");
    println!("wrote it back");
}
