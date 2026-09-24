//! Convoy's GTK front end.
//!
//! Exposed as a library so the window can be built and inspected by a test
//! rather than only by a person. `main.rs` is a thin wrapper around [`run`].

pub mod app;
pub mod dialogs;
pub mod objects;
pub mod paths;
pub mod state;
pub mod terminal;
pub mod theme;
pub mod window;

pub use app::run;
