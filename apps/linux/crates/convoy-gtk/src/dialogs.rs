//! Dialogs. libadwaita's `AlertDialog` replaces the `<dialog>` elements the
//! Electron renderer used; forms are built from `PreferencesGroup` rows so
//! they look and behave like the rest of the desktop.

use adw::prelude::*;
use convoy_core::model::Agent;
use convoy_core::workspace::NewSession;
use std::rc::Rc;

use crate::state::App;
use crate::window;

/// Asks for the folder to add. Discovery runs afterwards, so choosing a
/// directory of repositories registers each one rather than the parent.
pub fn open_folder(app: &Rc<App>) {
    let dialog = gtk::FileDialog::builder().title("Open folder").build();
    dialog.select_folder(
        Some(&app.window),
        gtk::gio::Cancellable::NONE,
        {
            let app = app.clone();
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                let found = convoy_core::files::discover(&path);
                let mut added = 0usize;
                for directory in found {
                    let mut workspace = app.workspace.borrow_mut();
                    match workspace.add_project(&directory) {
                        Ok(_) => added += 1,
                        Err(error) => {
                            drop(workspace);
                            app.error(error);
                        }
                    }
                }
                if added > 1 {
                    app.toasts.add_toast(
                        adw::Toast::builder()
                            .title(format!("Added {added} projects"))
                            .build(),
                    );
                }
                app.sync();
            }
        },
    );
}

/// A new session for the selected project. The first message is sent only on
/// the first launch; resuming never replays it.
pub fn new_session(app: &Rc<App>) {
    let Some(project) = app.selected_project() else {
        return;
    };

    let title = adw::EntryRow::builder().title("Session name").build();
    title.set_text(&project.title);

    let agents = gtk::StringList::new(&["Claude Code", "Codex"]);
    let agent = adw::ComboRow::builder()
        .title("Agent")
        .model(&agents)
        .build();
    agent.set_selected(match app.workspace.borrow().settings().default_agent {
        Agent::Claude => 0,
        Agent::Codex => 1,
    });

    let prompt = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let prompt_frame = gtk::ScrolledWindow::builder()
        .child(&prompt)
        .height_request(120)
        .build();
    prompt_frame.add_css_class("card");

    let group = adw::PreferencesGroup::new();
    group.add(&title);
    group.add(&agent);
    let prompt_group = adw::PreferencesGroup::builder()
        .title("First message")
        .description("Optional. Sent once, on the first launch only.")
        .build();
    prompt_group.add(&prompt_frame);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&group);
    body.append(&prompt_group);

    let dialog = adw::AlertDialog::builder()
        .heading("New session")
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create");
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("create"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        let title = title.clone();
        let agent = agent.clone();
        let prompt = prompt.clone();
        move |_, response| {
            if response != "create" {
                return;
            }
            let buffer = prompt.buffer();
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let mut input = NewSession::new(
                project.id.clone(),
                if agent.selected() == 0 {
                    Agent::Claude
                } else {
                    Agent::Codex
                },
                title.text().to_string(),
            );
            input.prompt = text;

            let created = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.add_session(input).map(|state| {
                    state
                        .sessions
                        .last()
                        .map(|session| session.id.clone())
                        .unwrap_or_default()
                })
            };
            match created {
                Ok(id) => {
                    window::rebuild_tabs(&app);
                    app.selection.borrow_mut().session = Some(id.clone());
                    if let Some(page) = app
                        .views
                        .borrow()
                        .get(&id)
                        .and_then(|view| view.page.borrow().clone())
                    {
                        app.tabs.set_selected_page(&page);
                    }
                    app.sync();
                }
                Err(error) => app.error(error),
            }
        }
    });
    dialog.present(Some(&app.window));
}

/// Appearance and agent defaults. The rest of the preference pages arrive with
/// the milestones that need them.
pub fn settings(app: &Rc<App>) {
    let settings = app.workspace.borrow().settings().clone();

    let themes = gtk::StringList::new(&["Follow system", "Dark", "Light"]);
    let theme = adw::ComboRow::builder()
        .title("Theme")
        .model(&themes)
        .build();
    theme.set_selected(match settings.theme {
        convoy_core::model::Theme::System => 0,
        convoy_core::model::Theme::Dark => 1,
        convoy_core::model::Theme::Light => 2,
    });

    let font = adw::SpinRow::with_range(10.0, 24.0, 1.0);
    font.set_title("Font size");
    font.set_value(settings.font_size as f64);

    let scrollback = adw::SpinRow::with_range(1000.0, 50_000.0, 1000.0);
    scrollback.set_title("Scrollback");
    scrollback.set_subtitle("Lines kept per terminal");
    scrollback.set_value(settings.scrollback as f64);

    let agents = gtk::StringList::new(&["Claude Code", "Codex"]);
    let default_agent = adw::ComboRow::builder()
        .title("Default agent")
        .model(&agents)
        .build();
    default_agent.set_selected(match settings.default_agent {
        Agent::Claude => 0,
        Agent::Codex => 1,
    });

    let appearance = adw::PreferencesGroup::builder().title("Appearance").build();
    appearance.add(&theme);
    appearance.add(&font);
    appearance.add(&scrollback);
    let behaviour = adw::PreferencesGroup::builder().title("Agents").build();
    behaviour.add(&default_agent);

    let page = adw::PreferencesPage::new();
    page.add(&appearance);
    page.add(&behaviour);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    // Settings apply as they are changed, like every other GNOME preference.
    let save = {
        let app = app.clone();
        let theme = theme.clone();
        let font = font.clone();
        let scrollback = scrollback.clone();
        let default_agent = default_agent.clone();
        move || {
            let patch = convoy_core::workspace::SettingsPatch {
                theme: Some(match theme.selected() {
                    1 => convoy_core::model::Theme::Dark,
                    2 => convoy_core::model::Theme::Light,
                    _ => convoy_core::model::Theme::System,
                }),
                font_size: Some(font.value() as i64),
                scrollback: Some(scrollback.value() as i64),
                default_agent: Some(if default_agent.selected() == 0 {
                    Agent::Claude
                } else {
                    Agent::Codex
                }),
                ..Default::default()
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.save_settings(patch).map(|_| ())
            };
            if let Err(error) = outcome {
                app.error(error);
                return;
            }
            crate::theme::apply(&app);
            for view in app.views.borrow().values() {
                crate::terminal::apply_settings(&app, &view.terminal);
            }
        }
    };

    theme.connect_selected_notify({
        let save = save.clone();
        move |_| save()
    });
    font.connect_value_notify({
        let save = save.clone();
        move |_| save()
    });
    scrollback.connect_value_notify({
        let save = save.clone();
        move |_| save()
    });
    default_agent.connect_selected_notify({
        let save = save.clone();
        move |_| save()
    });

    dialog.present(Some(&app.window));
}

pub fn about(app: &Rc<App>) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Convoy")
        .application_icon(crate::paths::APPLICATION_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Run and review coding agents across projects.")
        .build();
    dialog.present(Some(&app.window));
}
