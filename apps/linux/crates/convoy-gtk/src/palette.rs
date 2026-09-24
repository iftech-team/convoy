//! The command palette.
//!
//! One list of everything reachable: the actions the menus offer, every
//! project, and every session of the selected project. Typing filters; Enter
//! runs. It exists so the keyboard is never the slow way round.

use adw::prelude::*;
use std::rc::Rc;

use crate::state::App;
use crate::window;

/// What activating a palette entry does.
type Run = Box<dyn Fn(&Rc<App>)>;

struct Command {
    title: String,
    subtitle: String,
    run: Run,
}

pub fn open(app: &Rc<App>) {
    let commands = Rc::new(build(app));

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Type a command, project or session")
        .hexpand(true)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .build();
    list.add_css_class("navigation-sidebar");

    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .height_request(420)
        .width_request(560)
        .build();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    body.append(&search);
    body.append(&scroller);

    let dialog = adw::Dialog::builder()
        .title("Commands")
        .child(&body)
        .content_width(600)
        .build();
    body.set_margin_top(12);
    body.set_margin_bottom(12);
    body.set_margin_start(12);
    body.set_margin_end(12);

    let fill = {
        let commands = commands.clone();
        let list = list.clone();
        move |needle: &str| {
            while let Some(row) = list.first_child() {
                list.remove(&row);
            }
            let needle = needle.to_lowercase();
            for (index, command) in commands.iter().enumerate() {
                let matches = needle.is_empty()
                    || command.title.to_lowercase().contains(&needle)
                    || command.subtitle.to_lowercase().contains(&needle);
                if !matches {
                    continue;
                }
                let row = adw::ActionRow::builder()
                    .title(&command.title)
                    .subtitle(&command.subtitle)
                    .activatable(true)
                    .build();
                // The index travels with the row so activation finds it again
                // after filtering has renumbered everything.
                unsafe { row.set_data("command-index", index) };
                list.append(&row);
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
        }
    };
    fill("");

    search.connect_search_changed({
        let fill = fill.clone();
        move |entry| fill(&entry.text())
    });
    search.connect_activate({
        let list = list.clone();
        move |_| {
            if let Some(row) = list.selected_row() {
                row.activate();
            }
        }
    });

    list.connect_row_activated({
        let app = app.clone();
        let commands = commands.clone();
        let dialog = dialog.clone();
        move |_, row| {
            let index: Option<usize> = unsafe { row.data("command-index").map(|value| *value.as_ptr()) };
            let Some(index) = index else { return };
            dialog.close();
            if let Some(command) = commands.get(index) {
                (command.run)(&app);
            }
        }
    });

    dialog.present(Some(&app.window));
    search.grab_focus();
}

fn build(app: &Rc<App>) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();

    for (action, title) in [
        ("app.new-session", "New session"),
        ("app.open-folder", "Open folder"),
        ("app.files", "Files & Changes"),
        ("app.specs", "Specs & tasks"),
        ("app.import-history", "Import provider history"),
        ("app.activity", "Activity"),
        ("app.settings", "Settings"),
    ] {
        let name = action.to_string();
        commands.push(Command {
            title: title.to_string(),
            subtitle: "Action".to_string(),
            run: Box::new(move |app| {
                if let Some(application) = app.window.application() {
                    application.activate_action(name.trim_start_matches("app."), None);
                }
            }),
        });
    }

    for (action, title) in [
        ("session.review", "Start review"),
        ("session.worktree", "Create worktree"),
        ("session.history", "Saved output"),
        ("session.usage", "Usage limits"),
        ("session.split", "Open split terminal"),
    ] {
        let name = action.trim_start_matches("session.").to_string();
        commands.push(Command {
            title: title.to_string(),
            subtitle: "Session".to_string(),
            run: Box::new(move |app| {
                if let Some(action) = app.actions.lookup_action(&name) {
                    action.activate(None);
                }
            }),
        });
    }

    let workspace = app.workspace.borrow();
    for project in &workspace.state().projects {
        let id = project.id.clone();
        commands.push(Command {
            title: project.title.clone(),
            subtitle: format!("Project · {}", project.path.display()),
            run: Box::new(move |app| {
                app.selection.borrow_mut().project = Some(id.clone());
                app.selection.borrow_mut().session = None;
                window::rebuild_tabs(app);
                app.sync();
            }),
        });
    }

    for session in app.visible_sessions() {
        let id = session.id.clone();
        commands.push(Command {
            title: session.title.clone(),
            subtitle: format!("Session · {}", session.agent.as_str()),
            run: Box::new(move |app| {
                let page = app
                    .views
                    .borrow()
                    .get(&id)
                    .and_then(|view| view.page.borrow().clone());
                if let Some(page) = page {
                    app.tabs.set_selected_page(&page);
                }
            }),
        });
    }
    drop(workspace);

    commands
}
