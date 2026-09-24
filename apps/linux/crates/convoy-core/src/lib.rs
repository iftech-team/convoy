//! Convoy core: every rule the Linux client obeys, with no toolkit dependency.
//!
//! The crate is a port of the Electron preview's main-process modules. It is
//! deliberately synchronous — the GTK front end drives long-running work with
//! `gio::spawn_blocking` rather than dragging a second async runtime into the
//! GLib main loop. External commands go through [`process::ProcessRunner`] so
//! tests can observe them without touching the machine.

pub mod accounts;
pub mod ansi;
pub mod error;
pub mod files;
pub mod git;
pub mod hash;
pub mod history;
pub mod json;
pub mod patterns;
pub mod planning;
pub mod process;
pub mod provider;
pub mod repository;
pub mod session;
pub mod storage;
pub mod telemetry;
pub mod time;
pub mod worktree;
pub mod workspace;

pub use error::{ConvoyError, Result};
pub use git::Git;
pub use process::{Output, ProcessRunner, ProcessSpec, StdRunner};
pub use storage::Storage;
pub use workspace::model;
pub use workspace::Workspace;
