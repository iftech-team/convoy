#![allow(dead_code)]

//! Shared fixtures for the ported test suites.

use convoy_core::workspace::model::{Agent, State};
use convoy_core::workspace::{NewSession, Workspace};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct Fixture {
    pub directory: TempDir,
    pub file: PathBuf,
}

pub fn fixture() -> Fixture {
    let directory = tempfile::Builder::new()
        .prefix("convoy-test-")
        .tempdir()
        .expect("temporary directory");
    let file = directory.path().join("workspace.json");
    Fixture { directory, file }
}

impl Fixture {
    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    pub fn workspace(&self) -> Workspace {
        Workspace::load(&self.file).expect("workspace")
    }

    /// A workspace with the fixture directory registered as a project.
    pub fn with_project(&self) -> (Workspace, String) {
        let mut workspace = self.workspace();
        workspace.add_project(self.path()).expect("add project");
        let id = workspace.state().projects[0].id.clone();
        (workspace, id)
    }
}

pub fn new_session(project_id: &str, agent: Agent, title: &str) -> NewSession {
    NewSession::new(project_id, agent, title)
}

/// The comparison `assert.deepEqual(new Workspace(file).state, workspace.state)`
/// performs: equality of the serialised documents.
pub fn same_state(left: &State, right: &State) -> bool {
    serde_json::to_value(left).unwrap() == serde_json::to_value(right).unwrap()
}

/// Permission-based failure tests cannot work for a user that bypasses them.
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}
