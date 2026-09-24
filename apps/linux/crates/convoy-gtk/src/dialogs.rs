//! Dialogs. libadwaita's `AlertDialog` replaces the `<dialog>` elements the
//! Electron renderer used; forms are built from `PreferencesGroup` rows so
//! they look and behave like the rest of the desktop.

use adw::prelude::*;
use convoy_core::model::Agent;
use convoy_core::workspace::NewSession;
use std::rc::Rc;

use crate::state::{background, App};
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

// ---------------------------------------------------------------------------
// Session menu dialogs.
// ---------------------------------------------------------------------------

use convoy_core::model::Session;
use convoy_core::workspace::SessionPatch;

fn text_area(text: &str, editable: bool) -> (gtk::TextView, gtk::ScrolledWindow) {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .editable(editable)
        .monospace(!editable)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    view.buffer().set_text(text);
    let frame = gtk::ScrolledWindow::builder()
        .child(&view)
        .height_request(260)
        .width_request(560)
        .build();
    frame.add_css_class("card");
    (view, frame)
}

fn buffer_text(view: &gtk::TextView) -> String {
    let buffer = view.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

/// Name, notes and provider identity. The identity is what makes an exact
/// resume possible, so it can only be changed while the session is stopped.
pub fn edit_session(app: &Rc<App>, session: &Session) {
    let running = app
        .views
        .borrow()
        .get(&session.id)
        .is_some_and(|view| view.running());

    let title = adw::EntryRow::builder().title("Session name").build();
    title.set_text(&session.title);

    let provider = adw::EntryRow::builder()
        .title("Provider session ID")
        .sensitive(!running)
        .build();
    provider.set_text(&session.provider_id);
    if running {
        provider.set_tooltip_text(Some("Stop the session before changing its provider ID."));
    }

    let (notes_view, notes_frame) = text_area(session.notes.as_deref().unwrap_or(""), true);

    let identity = adw::PreferencesGroup::new();
    identity.add(&title);
    identity.add(&provider);
    let notes_group = adw::PreferencesGroup::builder().title("Notes").build();
    notes_group.add(&notes_frame);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&identity);
    body.append(&notes_group);

    let dialog = adw::AlertDialog::builder()
        .heading("Edit session")
        .extra_child(&body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        let id = session.id.clone();
        let original = session.provider_id.clone();
        move |_, response| {
            if response != "save" {
                return;
            }
            let mut patch = SessionPatch {
                title: Some(title.text().to_string()),
                notes: Some(buffer_text(&notes_view)),
                ..Default::default()
            };
            if !running && provider.text() != original {
                patch.provider_id = Some(provider.text().to_string());
            }
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.edit_session(&id, patch, running).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    crate::window::rebuild_tabs(&app);
                    app.sync();
                }
                Err(error) => app.error(error),
            }
        }
    });
    dialog.present(Some(&app.window));
}

/// Hands the work to the other agent. The brief is editable first: it carries
/// terminal output, and the user decides what a reviewer gets to see.
pub fn start_review(app: &Rc<App>, session: &Session) {
    let output = crate::terminal::saved_output(app, &session.id);
    let brief = match convoy_core::review::brief(&app.workspace.borrow(), &session.id, &output) {
        Ok(brief) => brief,
        Err(error) => {
            app.error(error);
            return;
        }
    };
    let reviewer = crate::actions::reviewer_for(session);
    let (view, frame) = text_area(&brief, true);

    let dialog = adw::AlertDialog::builder()
        .heading("Start review")
        .body(format!(
            "A new {} session will review this work in the same folder. \
             It is told not to edit files.",
            match reviewer {
                Agent::Claude => "Claude Code",
                Agent::Codex => "Codex",
            }
        ))
        .extra_child(&frame)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create review");
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        let id = session.id.clone();
        move |_, response| {
            if response != "create" {
                return;
            }
            let input = {
                let workspace = app.workspace.borrow();
                convoy_core::review::handoff(&workspace, &id, buffer_text(&view))
            };
            let created = match input {
                Ok(input) => {
                    let mut workspace = app.workspace.borrow_mut();
                    workspace.add_session(input).map(|_| ())
                }
                Err(error) => Err(error),
            };
            match created {
                Ok(()) => {
                    crate::window::rebuild_tabs(&app);
                    app.sync();
                }
                Err(error) => app.error(error),
            }
        }
    });
    dialog.present(Some(&app.window));
}

/// Findings go into the builder's prompt, not into its conversation: no Enter
/// is sent, so nothing is submitted on the user's behalf.
pub fn send_feedback(app: &Rc<App>, session: &Session) {
    let builder = match convoy_core::review::builder_of(&app.workspace.borrow(), &session.id) {
        Ok(builder) => builder,
        Err(error) => {
            app.error(error);
            return;
        }
    };
    let (view, frame) = text_area("", true);

    let dialog = adw::AlertDialog::builder()
        .heading("Send feedback to builder")
        .body("The text is typed into the builder's terminal. Nothing is submitted for you.")
        .extra_child(&frame)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("send", "Insert");
    dialog.set_response_appearance("send", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        move |_, response| {
            if response != "send" {
                return;
            }
            if crate::actions::insert(&app, &builder, &buffer_text(&view), false) {
                app.toasts.add_toast(
                    adw::Toast::builder()
                        .title("Feedback inserted; press Enter in the builder to send it")
                        .build(),
                );
            }
        }
    });
    dialog.present(Some(&app.window));
}

/// The bounded excerpt kept on disk. Not a transcript: it is whatever the CLI
/// printed, trimmed to the last 48,000 characters.
pub fn saved_output(app: &Rc<App>, session: &Session) {
    let text = crate::terminal::saved_output(app, &session.id);
    let (_, frame) = text_area(&text, false);
    let dialog = adw::AlertDialog::builder()
        .heading("Saved output")
        .body(format!("{} · bounded plain text, not a transcript", session.title))
        .extra_child(&frame)
        .build();
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(&app.window));
}

/// Quota windows. Claude reports them through its hooks; Codex is asked
/// directly, with no model request involved. A missing quota is reported as
/// unavailable and never guessed as zero.
pub fn usage(app: &Rc<App>, session: &Session) {
    let dialog = adw::AlertDialog::builder()
        .heading("Usage limits")
        .body("Reading…")
        .build();
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(&app.window));

    match session.agent {
        Agent::Claude => {
            let stored = convoy_core::telemetry::read(
                &app.storage.telemetry(),
                &session.id,
                ".usage",
            );
            let now = convoy_core::provider::now_seconds();
            let windows: Vec<String> = stored
                .as_ref()
                .and_then(|value| value.get("windows"))
                .and_then(|value| value.as_array())
                .map(|windows| {
                    windows
                        .iter()
                        .filter(|window| {
                            window
                                .get("resetsAt")
                                .and_then(|value| value.as_f64())
                                .is_none_or(|resets| resets > now)
                        })
                        .map(describe_window)
                        .collect()
                })
                .unwrap_or_default();
            dialog.set_body(&render_usage(windows, "Claude reports usage through its status line. Enable it in Settings and relaunch the session."));
        }
        Agent::Codex => {
            let account = {
                let workspace = app.workspace.borrow();
                convoy_core::session::prepare_account(&workspace, &app.storage, session)
            };
            let directory =
                convoy_core::session::directory_for(&app.workspace.borrow(), &session.id);
            match (account, directory) {
                (Ok(account), Ok(directory)) => {
                    let dialog = dialog.clone();
                    background(
                        app,
                        move || {
                            convoy_core::provider::codex::codex_limits(
                                &account.env,
                                &directory,
                                convoy_core::provider::now_seconds(),
                            )
                        },
                        move |_, windows| {
                            let lines: Vec<String> = windows
                                .iter()
                                .map(|window| {
                                    format!("{}: {:.0}%", window.name, window.percent)
                                })
                                .collect();
                            dialog.set_body(&render_usage(
                                lines,
                                "Codex reported no active quota window.",
                            ));
                        },
                    );
                }
                (Err(error), _) | (_, Err(error)) => dialog.set_body(&error.to_string()),
            }
        }
    }
}

fn describe_window(window: &serde_json::Value) -> String {
    let name = window
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("window");
    let percent = window
        .get("percent")
        .and_then(|value| value.as_f64())
        .unwrap_or_default();
    format!("{name}: {percent:.0}%")
}

fn render_usage(windows: Vec<String>, empty: &str) -> String {
    if windows.is_empty() {
        empty.to_string()
    } else {
        windows.join("\n")
    }
}

/// An isolated branch for one session. The branch name is checked by Git
/// itself, so anything it would refuse is refused here too.
pub fn create_worktree(app: &Rc<App>, session: &Session) {
    let suggestion = format!(
        "convoy/{}",
        session
            .title
            .to_lowercase()
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() { character } else { '-' })
            .collect::<String>()
            .trim_matches('-')
    );
    let branch = adw::EntryRow::builder().title("New branch").build();
    branch.set_text(&suggestion);
    let group = adw::PreferencesGroup::new();
    group.add(&branch);

    let dialog = adw::AlertDialog::builder()
        .heading("Create worktree")
        .body("A separate checkout on a new branch. The project's own checkout is left where it is.")
        .extra_child(&group)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create");
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        let id = session.id.clone();
        move |_, response| {
            if response != "create" {
                return;
            }
            crate::window::create_worktree(&app, &id, &branch.text());
        }
    });
    dialog.present(Some(&app.window));
}

/// Removal is clean-only and keeps the branch. Every session that used the
/// folder is archived, because it no longer exists for them.
pub fn remove_worktree(app: &Rc<App>, session: &Session) {
    let plan = {
        let workspace = app.workspace.borrow();
        let busy = |id: &str| app.busy.borrow().contains(id) || is_running(app, id);
        convoy_core::worktree::plan_remove(&workspace, &session.id, &app.storage.worktrees(), &busy)
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            app.error(error);
            return;
        }
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Remove this worktree?")
        .body(format!(
            "Git removes {} only if it is clean. The branch is kept. \
             All {} linked sessions will be archived.",
            plan.directory.display(),
            plan.linked.len()
        ))
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("remove", "Remove worktree");
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        move |_, response| {
            if response != "remove" {
                return;
            }
            crate::window::remove_worktree(&app, plan.clone());
        }
    });
    dialog.present(Some(&app.window));
}

fn is_running(app: &Rc<App>, id: &str) -> bool {
    app.views.borrow().get(id).is_some_and(|view| view.running())
}

/// Two terminals side by side. Exactly two, as in the Electron build: nesting
/// more panes is a change to the model, not to the view.
pub fn open_split(app: &Rc<App>, session: &Session) {
    let others: Vec<Session> = app
        .visible_sessions()
        .into_iter()
        .filter(|other| other.id != session.id)
        .collect();
    if others.is_empty() {
        app.error("There is no other session to show beside this one.");
        return;
    }

    let names: Vec<&str> = others.iter().map(|other| other.title.as_str()).collect();
    let model = gtk::StringList::new(&names);
    let choice = adw::ComboRow::builder()
        .title("Second pane")
        .model(&model)
        .build();
    let group = adw::PreferencesGroup::new();
    group.add(&choice);

    let dialog = adw::AlertDialog::builder()
        .heading("Open split terminal")
        .extra_child(&group)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("open", "Open");
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let app = app.clone();
        move |_, response| {
            if response != "open" {
                return;
            }
            let Some(chosen) = others.get(choice.selected() as usize) else {
                return;
            };
            crate::window::open_split(&app, &chosen.id);
        }
    });
    dialog.present(Some(&app.window));
}

/// Quick commands are saved text, sent into a running agent on demand.
/// Submitting is opt-in per command: without it the text is only typed.
pub fn quick_commands(app: &Rc<App>) {
    let project = app.selected_project();
    let page = adw::PreferencesPage::new();

    let existing = adw::PreferencesGroup::builder()
        .title("Saved commands")
        .build();
    let commands = app.workspace.borrow().state().quick_commands.clone();
    if commands.is_empty() {
        existing.add(
            &adw::ActionRow::builder()
                .title("Nothing saved yet")
                .subtitle("Add one below.")
                .build(),
        );
    }
    for command in &commands {
        let row = adw::ActionRow::builder()
            .title(&command.title)
            .subtitle(format!(
                "{}{}",
                convoy_core::json::head(&command.text, 80),
                if command.submit { " · sends Enter" } else { "" }
            ))
            .build();
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Remove")
            .build();
        remove.add_css_class("flat");
        remove.connect_clicked({
            let app = app.clone();
            let id = command.id.clone();
            let row = row.clone();
            move |_| {
                let outcome = {
                    let mut workspace = app.workspace.borrow_mut();
                    workspace.remove_command(&id).map(|_| ())
                };
                match outcome {
                    Ok(()) => {
                        row.set_sensitive(false);
                        app.sync();
                    }
                    Err(error) => app.error(error),
                }
            }
        });
        row.add_suffix(&remove);
        existing.add(&row);
    }

    let title = adw::EntryRow::builder().title("Name").build();
    let (text_view, text_frame) = text_area("", true);
    let submit = adw::SwitchRow::builder()
        .title("Send Enter")
        .subtitle("Off by default: the text is typed, not submitted.")
        .build();
    let scoped = adw::SwitchRow::builder()
        .title("Only this project")
        .active(project.is_some())
        .sensitive(project.is_some())
        .build();
    let add = gtk::Button::with_label("Add command");
    add.add_css_class("suggested-action");

    let fresh = adw::PreferencesGroup::builder().title("New command").build();
    fresh.add(&title);
    fresh.add(&text_frame);
    fresh.add(&submit);
    fresh.add(&scoped);
    fresh.add(&add);

    page.add(&existing);
    page.add(&fresh);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    add.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        move |_| {
            let command = convoy_core::model::QuickCommand {
                id: String::new(),
                title: title.text().to_string(),
                text: buffer_text(&text_view),
                submit: submit.is_active(),
                project_id: if scoped.is_active() {
                    project.as_ref().map(|project| project.id.clone())
                } else {
                    None
                },
                unknown: Default::default(),
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.save_command(command).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    app.sync();
                    dialog.close();
                }
                Err(error) => app.error(error),
            }
        }
    });

    dialog.present(Some(&app.window));
}
