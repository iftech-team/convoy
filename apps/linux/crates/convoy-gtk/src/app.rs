//! The application object: one instance, one window.

use adw::prelude::*;
use convoy_core::Workspace;

use crate::paths;

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(paths::APPLICATION_ID)
        .build();
    application.connect_activate(build);
    application.run()
}

fn build(application: &adw::Application) {
    let storage = paths::storage();
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Convoy")
        .default_width(1100)
        .default_height(720)
        .build();

    let summary = match Workspace::load(storage.workspace_file()) {
        Ok(workspace) => format!(
            "{} projects · {} sessions · schema {}",
            workspace.state().projects.len(),
            workspace.state().sessions.len(),
            workspace.state().schema_version
        ),
        Err(error) => format!("Workspace unavailable: {error}"),
    };

    let status = adw::StatusPage::builder()
        .icon_name("utilities-terminal-symbolic")
        .title("Convoy")
        .description(format!(
            "{summary}\n\n{}",
            storage.root().display()
        ))
        .build();

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&status));
    window.set_content(Some(&view));
    window.present();
}
