//! Dialogs. Confirmations use libadwaita's `AlertDialog`; forms are built from
//! `PreferencesGroup` rows so
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
    dialog.select_folder(Some(&app.window), gtk::gio::Cancellable::NONE, {
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
    });
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

    let model = adw::EntryRow::builder()
        .title("Model")
        .tooltip_text("Optional. Passed to the CLI as --model; leave empty for its default.")
        .build();

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
    group.add(&model);
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
        let model = model.clone();
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
            input.model = model.text().trim().to_string();

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

    let notifications = adw::SwitchRow::builder()
        .title("Notify when an agent finishes or needs input")
        .subtitle("Only while the window is not focused.")
        .active(settings.notifications)
        .build();

    let awake_modes = gtk::StringList::new(&["Never", "Always", "While sessions run"]);
    let keep_awake = adw::ComboRow::builder()
        .title("Keep the system awake")
        .subtitle("Prevents idle suspension, not closing the lid.")
        .model(&awake_modes)
        .build();
    keep_awake.set_selected(match settings.keep_awake {
        convoy_core::model::KeepAwake::Off => 0,
        convoy_core::model::KeepAwake::Always => 1,
        convoy_core::model::KeepAwake::Sessions => 2,
    });

    let hibernate = adw::SpinRow::with_range(0.0, 1440.0, 5.0);
    hibernate.set_title("Stop an idle Claude session after");
    hibernate.set_subtitle("Minutes after it reports a finished turn. 0 disables it.");
    hibernate.set_value(settings.hibernate_minutes as f64);

    let claude_usage = adw::SwitchRow::builder()
        .title("Claude usage status line")
        .subtitle("Replaces that launch's custom status line and takes effect on the next launch.")
        .active(settings.claude_usage)
        .build();

    let appearance = adw::PreferencesGroup::builder().title("Appearance").build();
    appearance.add(&theme);
    appearance.add(&font);
    appearance.add(&scrollback);
    let behaviour = adw::PreferencesGroup::builder().title("Agents").build();
    behaviour.add(&default_agent);
    behaviour.add(&claude_usage);
    behaviour.add(&hibernate);
    let session = adw::PreferencesGroup::builder().title("Session").build();
    session.add(&notifications);
    session.add(&keep_awake);

    let keys = adw::PreferencesGroup::builder()
        .title("Keyboard")
        .description("Shortcuts are saved in the workspace file.")
        .build();
    let edit_keys = gtk::Button::with_label("Edit shortcuts…");
    edit_keys.set_valign(gtk::Align::Center);
    edit_keys.connect_clicked({
        let app = app.clone();
        move |_| shortcuts(&app)
    });
    let keys_row = adw::ActionRow::builder().title("Shortcuts").build();
    keys_row.add_suffix(&edit_keys);
    keys.add(&keys_row);

    let accounts = adw::PreferencesGroup::builder()
        .title("Accounts")
        .description("Each profile gets its own provider home, so sign-ins stay separate.")
        .build();
    let manage_accounts = gtk::Button::with_label("Manage accounts…");
    manage_accounts.set_valign(gtk::Align::Center);
    manage_accounts.connect_clicked({
        let app = app.clone();
        move |_| accounts_dialog(&app)
    });
    let accounts_row = adw::ActionRow::builder()
        .title(format!(
            "{} saved",
            app.workspace.borrow().state().profiles.len()
        ))
        .build();
    accounts_row.add_suffix(&manage_accounts);
    accounts.add(&accounts_row);

    let page = adw::PreferencesPage::new();
    page.add(&appearance);
    page.add(&behaviour);
    page.add(&session);
    page.add(&keys);
    page.add(&accounts);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    // Settings apply as they are changed, like every other GNOME preference.
    let save = {
        let app = app.clone();
        let theme = theme.clone();
        let font = font.clone();
        let scrollback = scrollback.clone();
        let default_agent = default_agent.clone();
        let notifications = notifications.clone();
        let keep_awake = keep_awake.clone();
        let hibernate = hibernate.clone();
        let claude_usage = claude_usage.clone();
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
                notifications: Some(notifications.is_active()),
                keep_awake: Some(match keep_awake.selected() {
                    1 => convoy_core::model::KeepAwake::Always,
                    2 => convoy_core::model::KeepAwake::Sessions,
                    _ => convoy_core::model::KeepAwake::Off,
                }),
                hibernate_minutes: Some(hibernate.value() as i64),
                claude_usage: Some(claude_usage.is_active()),
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
            crate::power::refresh(&app);
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
    notifications.connect_active_notify({
        let save = save.clone();
        move |_| save()
    });
    keep_awake.connect_selected_notify({
        let save = save.clone();
        move |_| save()
    });
    hibernate.connect_value_notify({
        let save = save.clone();
        move |_| save()
    });
    claude_usage.connect_active_notify({
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
        .body(format!(
            "{} · bounded plain text, not a transcript",
            session.title
        ))
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
            let stored =
                convoy_core::telemetry::read(&app.storage.telemetry(), &session.id, ".usage");
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
                                .map(|window| format!("{}: {:.0}%", window.name, window.percent))
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
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
            .trim_matches('-')
    );
    let branch = adw::EntryRow::builder().title("New branch").build();
    branch.set_text(&suggestion);
    let group = adw::PreferencesGroup::new();
    group.add(&branch);

    let dialog = adw::AlertDialog::builder()
        .heading("Create worktree")
        .body(
            "A separate checkout on a new branch. The project's own checkout is left where it is.",
        )
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
    app.views
        .borrow()
        .get(id)
        .is_some_and(|view| view.running())
}

/// Two terminals side by side. Exactly two: nesting
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
                if command.submit {
                    " · sends Enter"
                } else {
                    ""
                }
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

    let fresh = adw::PreferencesGroup::builder()
        .title("New command")
        .build();
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

/// Account profiles.
///
/// Each profile is a separate provider home, so two subscriptions never share
/// a sign-in. Removing a profile removes only Convoy's record of it: the
/// credential files stay where they are, and a profile still bound to saved
/// sessions cannot be removed at all.
pub fn accounts_dialog(app: &Rc<App>) {
    let page = adw::PreferencesPage::new();

    let saved = adw::PreferencesGroup::builder()
        .title("Profiles")
        .description("Sign in through the provider's own interface inside the terminal.")
        .build();
    let profiles = app.workspace.borrow().state().profiles.clone();
    if profiles.is_empty() {
        saved.add(
            &adw::ActionRow::builder()
                .title("No profiles yet")
                .subtitle("Without one, a session uses the provider's default home.")
                .build(),
        );
    }
    for profile in &profiles {
        let bound = app
            .workspace
            .borrow()
            .state()
            .sessions
            .iter()
            .filter(|session| session.profile_id.as_deref() == Some(profile.id.as_str()))
            .count();
        let row = adw::ActionRow::builder()
            .title(&profile.label)
            .subtitle(format!(
                "{} · {}",
                match profile.agent {
                    Agent::Claude => "Claude Code",
                    Agent::Codex => "Codex",
                },
                match bound {
                    0 => "not in use".to_string(),
                    1 => "1 session".to_string(),
                    count => format!("{count} sessions"),
                }
            ))
            .build();
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(if bound == 0 {
                "Remove this profile record"
            } else {
                "Bound to saved sessions"
            })
            .sensitive(bound == 0)
            .build();
        remove.add_css_class("flat");
        remove.connect_clicked({
            let app = app.clone();
            let id = profile.id.clone();
            let row = row.clone();
            move |_| {
                let id = id.clone();
                let outcome = {
                    let mut workspace = app.workspace.borrow_mut();
                    workspace
                        .update(move |state| {
                            state.profiles.retain(|profile| profile.id != id);
                            Ok(())
                        })
                        .map(|_| ())
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
        saved.add(&row);
    }

    let label = adw::EntryRow::builder().title("Label").build();
    let agents = gtk::StringList::new(&["Claude Code", "Codex"]);
    let agent = adw::ComboRow::builder()
        .title("Provider")
        .model(&agents)
        .build();
    let add = gtk::Button::with_label("Add profile");
    add.add_css_class("suggested-action");
    let fresh = adw::PreferencesGroup::builder()
        .title("New profile")
        .build();
    fresh.add(&label);
    fresh.add(&agent);
    fresh.add(&add);

    page.add(&saved);
    page.add(&fresh);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);
    add.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        move |_| {
            let chosen = if agent.selected() == 0 {
                Agent::Claude
            } else {
                Agent::Codex
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.add_profile(&label.text(), chosen).map(|_| ())
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

/// Conversations the provider already has for this folder.
///
/// Importing one records it as a session with that exact identity; it is not
/// launched, and nothing is copied out of the provider's own storage.
pub fn import_history(app: &Rc<App>) {
    let Some(project) = app.selected_project() else {
        app.error("Select a project first.");
        return;
    };

    let agents = gtk::StringList::new(&["Claude Code", "Codex"]);
    let agent = adw::ComboRow::builder()
        .title("Provider")
        .model(&agents)
        .build();
    let profiles = app.workspace.borrow().state().profiles.clone();
    let mut names: Vec<String> = vec!["Default home".to_string()];
    names.extend(profiles.iter().map(|profile| profile.label.clone()));
    let profile_model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let profile = adw::ComboRow::builder()
        .title("Account")
        .model(&profile_model)
        .build();

    let chooser = adw::PreferencesGroup::new();
    chooser.add(&agent);
    chooser.add(&profile);

    let found = adw::PreferencesGroup::builder()
        .title("Conversations")
        .description("Press Scan to look for conversations in this folder.")
        .build();

    let page = adw::PreferencesPage::new();
    page.add(&chooser);
    page.add(&found);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    let scan = gtk::Button::with_label("Scan");
    scan.add_css_class("suggested-action");
    scan.connect_clicked({
        let app = app.clone();
        let found = found.clone();
        let agent = agent.clone();
        let profile = profile.clone();
        let profiles = profiles.clone();
        let project = project.clone();
        move |_| {
            let chosen = if agent.selected() == 0 {
                Agent::Claude
            } else {
                Agent::Codex
            };
            let bound = profile
                .selected()
                .checked_sub(1)
                .and_then(|index| profiles.get(index as usize))
                .filter(|profile| profile.agent == chosen)
                .map(|profile| profile.id.clone());

            let mut probe = Session::new(project.id.clone(), chosen, "scan");
            probe.profile_id = bound.clone();
            let account = {
                let workspace = app.workspace.borrow();
                convoy_core::accounts::account_environment(
                    &probe,
                    &workspace.state().profiles,
                    &app.storage.accounts(),
                    &convoy_core::provider::launch::current_environment(),
                )
            };
            let account = match account {
                Ok(account) => account,
                Err(error) => return app.error(error),
            };
            let path = project.path.clone();
            let home = account.home.clone();
            background(
                &app,
                move || {
                    Ok(convoy_core::provider::transcripts::scan(
                        chosen, &home, &path,
                    ))
                },
                {
                    let app = app.clone();
                    let found = found.clone();
                    let project = project.clone();
                    let home = account.home.clone();
                    let bound = bound.clone();
                    move |_, transcripts: Vec<convoy_core::provider::transcripts::Transcript>| {
                        show_transcripts(
                            &app,
                            &found,
                            &project.id,
                            chosen,
                            &home,
                            bound.clone(),
                            &transcripts,
                        );
                    }
                },
            );
        }
    });
    chooser.set_header_suffix(Some(&scan));
    dialog.present(Some(&app.window));
}

fn show_transcripts(
    app: &Rc<App>,
    group: &adw::PreferencesGroup,
    project_id: &str,
    agent: Agent,
    home: &std::path::Path,
    profile_id: Option<String>,
    transcripts: &[convoy_core::provider::transcripts::Transcript],
) {
    // `PreferencesGroup` has no clear, so rows are removed one by one.
    while let Some(child) = group.first_child().and_downcast::<gtk::Widget>() {
        if child.downcast_ref::<adw::ActionRow>().is_none() {
            break;
        }
        group.remove(&child);
    }
    if transcripts.is_empty() {
        group.add(
            &adw::ActionRow::builder()
                .title("Nothing found for this folder")
                .subtitle("Conversations are matched by the folder the CLI ran in.")
                .build(),
        );
        return;
    }
    for transcript in transcripts {
        let row = adw::ActionRow::builder()
            .title(&transcript.title)
            .subtitle(&transcript.provider_id)
            .build();
        let import = gtk::Button::with_label("Import");
        import.set_valign(gtk::Align::Center);
        import.connect_clicked({
            let app = app.clone();
            let project_id = project_id.to_string();
            let provider_id = transcript.provider_id.clone();
            let title = transcript.title.clone();
            let home = home.to_path_buf();
            let profile_id = profile_id.clone();
            let row = row.clone();
            move |_| {
                let outcome = import_one(
                    &app,
                    &project_id,
                    agent,
                    &provider_id,
                    &title,
                    &home,
                    profile_id.clone(),
                );
                match outcome {
                    Ok(()) => {
                        row.set_sensitive(false);
                        crate::window::rebuild_tabs(&app);
                        app.sync();
                    }
                    Err(error) => app.error(error),
                }
            }
        });
        row.add_suffix(&import);
        group.add(&row);
    }
}

fn import_one(
    app: &Rc<App>,
    project_id: &str,
    agent: Agent,
    provider_id: &str,
    title: &str,
    home: &std::path::Path,
    profile_id: Option<String>,
) -> convoy_core::Result<()> {
    let already = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .any(|session| {
            session.provider_id == provider_id
                && session.agent == agent
                && session.agent_home.as_deref() == Some(home)
        });
    if already {
        return Ok(());
    }

    let mut input = NewSession::new(
        project_id,
        agent,
        if title.trim().is_empty() {
            "Imported session"
        } else {
            title
        },
    );
    input.profile_id = profile_id;

    let mut workspace = app.workspace.borrow_mut();
    workspace.add_session(input)?;
    let id = workspace
        .state()
        .sessions
        .last()
        .map(|session| session.id.clone())
        .unwrap_or_default();
    let provider_id = provider_id.to_string();
    let home = home.to_path_buf();
    workspace
        .update(move |state| {
            if let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) {
                // The conversation already exists, so the session is recorded
                // as started: resuming it opens that exact conversation.
                session.provider_id = provider_id;
                session.started = true;
                session.agent_home = Some(home);
            }
            Ok(())
        })
        .map(|_| ())
}

/// What the agents have been doing. Bounded to the last 200 events and stored
/// with the workspace, so it survives a restart.
pub fn activity(app: &Rc<App>) {
    let group = adw::PreferencesGroup::builder()
        .title("Recent activity")
        .description("Newest first. Kept to the last 200 events.")
        .build();

    let workspace = app.workspace.borrow();
    if workspace.state().activity.is_empty() {
        group.add(
            &adw::ActionRow::builder()
                .title("Nothing yet")
                .subtitle("Start a session to see it here.")
                .build(),
        );
    }
    for event in workspace.state().activity.iter().take(200) {
        let when = glib::DateTime::from_iso8601(&event.at, None)
            .ok()
            .and_then(|value| value.format("%d %b %H:%M").ok())
            .map(|value| value.to_string())
            .unwrap_or_else(|| event.at.clone());
        group.add(
            &adw::ActionRow::builder()
                .title(format!("{} · {}", event.title, kind_label(event.kind)))
                .subtitle(format!("{when} · {}", event.detail))
                .build(),
        );
    }
    drop(workspace);

    let page = adw::PreferencesPage::new();
    page.add(&group);
    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);
    dialog.present(Some(&app.window));
}

fn kind_label(kind: convoy_core::model::ActivityKind) -> &'static str {
    use convoy_core::model::ActivityKind;
    match kind {
        ActivityKind::Started => "started",
        ActivityKind::Resumed => "resumed",
        ActivityKind::Exited => "exited",
        ActivityKind::Done => "finished a turn",
        ActivityKind::Waiting => "needs attention",
        ActivityKind::Hibernated => "hibernated",
        ActivityKind::Worktree => "worktree",
    }
}

/// The shortcut editor.
///
/// A shortcut is stored in the workspace notation (`mod+shift+p`) so every
/// build reads the same file, and the same rules apply: Primary is required, the modifier order is
/// fixed, and no two actions may share a binding.
pub fn shortcuts(app: &Rc<App>) {
    let saved = app.workspace.borrow().settings().shortcuts.clone();
    let group = adw::PreferencesGroup::builder()
        .title("Shortcuts")
        .description("Press a combination with Ctrl. Clear a field to restore its default.")
        .build();

    let rows: Vec<(String, adw::EntryRow)> = convoy_core::shortcuts::resolve(&saved)
        .into_iter()
        .map(|(action, stored, _)| {
            let row = adw::EntryRow::builder()
                .title(convoy_core::shortcuts::description(action))
                .build();
            row.set_text(&stored);
            group.add(&row);
            (action.to_string(), row)
        })
        .collect();

    let page = adw::PreferencesPage::new();
    page.add(&group);
    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    let save = gtk::Button::with_label("Save shortcuts");
    save.add_css_class("suggested-action");
    save.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        let rows = rows.clone();
        move |_| {
            let mut shortcuts = std::collections::BTreeMap::new();
            for (action, row) in &rows {
                let value = row.text().trim().to_lowercase();
                if value.is_empty() {
                    continue;
                }
                if convoy_core::shortcuts::to_accelerator(&value).is_none() {
                    app.error(format!(
                        "{} is not a usable shortcut. Use the form mod+shift+p.",
                        row.text()
                    ));
                    return;
                }
                shortcuts.insert(action.clone(), value);
            }
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace
                    .save_settings(convoy_core::workspace::SettingsPatch {
                        shortcuts: Some(shortcuts),
                        ..Default::default()
                    })
                    .map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    if let Some(application) =
                        app.window.application().and_downcast::<adw::Application>()
                    {
                        crate::app::apply_shortcuts(&application, &app);
                    }
                    dialog.close();
                }
                // `Invalid or duplicate shortcut.` arrives here unchanged.
                Err(error) => app.error(error),
            }
        }
    });
    group.set_header_suffix(Some(&save));
    dialog.present(Some(&app.window));
}

/// Per-project settings: how it is shown, and what a new worktree gets.
pub fn project_settings(app: &Rc<App>, project_id: &str) {
    let Some(project) = app
        .workspace
        .borrow()
        .state()
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .cloned()
    else {
        return;
    };

    let title = adw::EntryRow::builder().title("Name").build();
    title.set_text(&project.title);
    let group_row = adw::EntryRow::builder().title("Group").build();
    group_row.set_text(project.group.as_deref().unwrap_or(""));
    let icon = adw::EntryRow::builder()
        .title("Icon")
        .tooltip_text("A single emoji, shown beside the name.")
        .build();
    icon.set_text(project.icon.as_deref().unwrap_or(""));

    let identity = adw::PreferencesGroup::builder().title("Appearance").build();
    identity.add(&title);
    identity.add(&group_row);
    identity.add(&icon);

    let (shared_group, shared) = {
        let (view, frame) = text_area(project.shared_paths.as_deref().unwrap_or(""), true);
        let wrapper = adw::PreferencesGroup::builder()
            .title("Shared files")
            .description(
                "One repository-relative path per line, copied into each new worktree. \
                 Existing files are never replaced.",
            )
            .build();
        wrapper.add(&frame);
        (wrapper, view)
    };

    let setup = adw::EntryRow::builder().title("Setup command").build();
    setup.set_text(project.setup_command.as_deref().unwrap_or(""));
    let setup_group = adw::PreferencesGroup::builder()
        .title("Worktree setup")
        .description("Shown and confirmed before it runs. It runs with your permissions.")
        .build();
    setup_group.add(&setup);

    let (review_group, review) = {
        let (view, frame) = text_area(project.review_template.as_deref().unwrap_or(""), true);
        let wrapper = adw::PreferencesGroup::builder()
            .title("Review instructions")
            .description("Prefixed to every review brief for this project.")
            .build();
        wrapper.add(&frame);
        (wrapper, view)
    };

    let page = adw::PreferencesPage::new();
    page.add(&identity);
    page.add(&shared_group);
    page.add(&setup_group);
    page.add(&review_group);

    let dialog = adw::PreferencesDialog::new();
    dialog.add(&page);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        let id = project.id.clone();
        move |_| {
            let patch = convoy_core::workspace::ProjectPatch {
                title: Some(title.text().to_string()),
                group: Some(group_row.text().to_string()),
                icon: Some(icon.text().to_string()),
                setup_command: Some(setup.text().to_string()),
                shared_paths: Some(buffer_text(&shared)),
                review_template: Some(buffer_text(&review)),
                ..Default::default()
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.edit_project(&id, patch).map(|_| ())
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
    identity.set_header_suffix(Some(&save));
    dialog.present(Some(&app.window));
}

/// Removing a project forgets its sessions. The folder, its worktrees and the
/// provider's own conversations are all left alone.
pub fn remove_project(app: &Rc<App>, project_id: &str) {
    let running = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .any(|session| {
            session.project_id == project_id
                && app
                    .views
                    .borrow()
                    .get(&session.id)
                    .is_some_and(|view| view.running())
        });
    if running {
        app.error("Stop project sessions first.");
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading("Remove project and its saved sessions?")
        .body("Files, worktrees and provider conversations remain on disk.")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("remove", "Remove");
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let app = app.clone();
        let project_id = project_id.to_string();
        move |_, response| {
            if response != "remove" {
                return;
            }
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.remove_project(&project_id).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    if app.selection.borrow().project.as_deref() == Some(project_id.as_str()) {
                        app.selection.borrow_mut().project = None;
                        app.selection.borrow_mut().session = None;
                    }
                    crate::window::rebuild_tabs(&app);
                    app.sync();
                }
                Err(error) => app.error(error),
            }
        }
    });
    dialog.present(Some(&app.window));
}

/// Points a project at a folder that moved. Sessions keep their identities.
pub fn reconnect_project(app: &Rc<App>, project_id: &str) {
    let running = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .any(|session| {
            session.project_id == project_id
                && app
                    .views
                    .borrow()
                    .get(&session.id)
                    .is_some_and(|view| view.running())
        });
    if running {
        app.error("Stop project sessions before reconnecting.");
        return;
    }

    let chooser = gtk::FileDialog::builder()
        .title("Reconnect project folder")
        .build();
    chooser.select_folder(Some(&app.window), gtk::gio::Cancellable::NONE, {
        let app = app.clone();
        let project_id = project_id.to_string();
        move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file
                .path()
                .and_then(|path| std::fs::canonicalize(path).ok())
            else {
                return;
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace
                    .edit_project(
                        &project_id,
                        convoy_core::workspace::ProjectPatch {
                            path: Some(path.to_string_lossy().into_owned()),
                            ..Default::default()
                        },
                    )
                    .map(|_| ())
            };
            match outcome {
                Ok(()) => app.sync(),
                Err(error) => app.error(error),
            }
        }
    });
}
