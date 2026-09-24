//! Running an agent in a VTE terminal.
//!
//! The rules live in `convoy_core::session`; this module owns the pseudo-
//! terminal, which VTE will not hand over. The split matters most at start-up:
//! `Pty::spawn_async` is asynchronous where `node-pty`'s spawn was not, so the
//! process can exist before the workspace knows about it. If recording the
//! start fails, the group is killed immediately — an agent running against a
//! workspace that does not list it would be invisible and unstoppable.

use adw::prelude::*;
use convoy_core::history::History;
use convoy_core::model::{Agent, Session};
use convoy_core::patterns::UUID;
use convoy_core::session::{finish_session, mark_started, plan_launch, ExitCause};
use gtk::gdk;
use std::cell::Cell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};
use vte4::prelude::*;
use vte4::{Format, Pty, PtyFlags, Terminal};

use crate::state::{App, SessionView};

/// How long output may sit unsaved. Matches the Electron debounce.
const SAVE_INTERVAL: Duration = Duration::from_secs(2);

/// Builds the terminal and its tab. The widget is created once per session and
/// kept: stopping a session leaves its output on screen.
pub fn ensure_view(app: &Rc<App>, session: &Session) -> Rc<SessionView> {
    if let Some(view) = app.views.borrow().get(&session.id) {
        return view.clone();
    }

    let terminal = Terminal::new();
    apply_settings(app, &terminal);
    terminal.set_hexpand(true);
    terminal.set_vexpand(true);

    let view = Rc::new(SessionView {
        terminal: terminal.clone(),
        page: std::cell::RefCell::new(None),
        pid: Cell::new(0),
        stopping: Cell::new(false),
        saved_at: Cell::new(Instant::now()),
        save_pending: Cell::new(false),
        agent_state: Cell::new(None),
        status_at: Cell::new(0.0),
        hibernating: Cell::new(false),
    });
    app.views
        .borrow_mut()
        .insert(session.id.clone(), view.clone());

    // One handler for the life of the widget: which child it refers to is
    // decided by the pid recorded at start.
    terminal.connect_child_exited({
        let app = app.clone();
        let id = session.id.clone();
        move |_, status| exited(&app, &id, status)
    });

    terminal.connect_contents_changed({
        let app = app.clone();
        let id = session.id.clone();
        move |_| contents_changed(&app, &id)
    });

    // Restore what the previous run left behind, so a session reopened after a
    // restart is not a blank rectangle.
    let history = History::new(app.storage.history());
    if let Ok(previous) = history.read(&session.id) {
        if !previous.is_empty() {
            terminal.feed(previous.replace('\n', "\r\n").as_bytes());
            terminal.feed(b"\r\n");
        }
    }
    view
}

pub fn apply_settings(app: &Rc<App>, terminal: &Terminal) {
    let settings = app.workspace.borrow().settings().clone();
    terminal.set_scrollback_lines(settings.scrollback as _);
    terminal.set_font_desc(Some(&gtk::pango::FontDescription::from_string(&format!(
        "monospace {}",
        settings.font_size
    ))));
    terminal.set_audible_bell(false);
    terminal.set_mouse_autohide(true);
    terminal.set_bold_is_bright(true);
    terminal.set_allow_hyperlink(true);
    terminal.set_scroll_on_output(false);
    terminal.set_cursor_blink_mode(vte4::CursorBlinkMode::Off);

    let dark = adw::StyleManager::default().is_dark();
    let (foreground, background) = if dark {
        ("#e4e7ee", "#111318")
    } else {
        ("#202637", "#fafbfe")
    };
    if let Ok(colour) = gdk::RGBA::parse(foreground) {
        terminal.set_color_foreground(&colour);
    }
    if let Ok(colour) = gdk::RGBA::parse(background) {
        terminal.set_color_background(&colour);
    }
}

/// Starts or resumes a session.
pub fn start(app: &Rc<App>, id: &str) {
    let Some(session) = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .find(|session| session.id == id)
        .cloned()
    else {
        app.error("Session not found.");
        return;
    };
    let view = ensure_view(app, &session);
    if view.running() {
        return;
    }

    let plan = match plan_launch(
        &app.workspace.borrow(),
        &app.storage,
        id,
        &app.executable,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            app.error(error);
            return;
        }
    };

    let pty = match Pty::new_sync(PtyFlags::DEFAULT, gtk::gio::Cancellable::NONE) {
        Ok(pty) => pty,
        Err(error) => {
            app.error(error);
            return;
        }
    };
    view.terminal.set_pty(Some(&pty));
    view.stopping.set(false);

    let argv: Vec<&str> = std::iter::once(plan.spec.file.as_str())
        .chain(plan.spec.args.iter().map(String::as_str))
        .collect();
    let envv: Vec<String> = plan
        .env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    let envv: Vec<&str> = envv.iter().map(String::as_str).collect();

    pty.spawn_async(
        Some(&plan.directory.to_string_lossy()),
        &argv,
        &envv,
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        gtk::gio::Cancellable::NONE,
        {
            let app = app.clone();
            let view = view.clone();
            let plan = plan.clone();
            move |result| {
                let pid = match result {
                    Ok(pid) => pid,
                    Err(error) => {
                        app.error(error);
                        view.terminal.set_pty(None);
                        return;
                    }
                };
                view.pid.set(pid.0);

                // VTE drops the pty the instant the child is gone, and
                // `watch_child` asserts without one. A command that fails at
                // once — a missing CLI exits 127 — still needs to report its
                // code, so the GLib watch stands in. Exactly one per pid.
                if view.terminal.pty().is_some() {
                    view.terminal.watch_child(pid);
                } else {
                    glib::child_watch_add_local(pid, {
                        let app = app.clone();
                        let id = plan.session_id.clone();
                        move |_, status| exited(&app, &id, status)
                    });
                }

                crate::power::refresh(&app);
                let outcome = mark_started(&mut app.workspace.borrow_mut(), &plan);
                if let Err(error) = outcome {
                    // The process is already running but the workspace does
                    // not know it. Stop it rather than leave it orphaned.
                    kill_group(pid.0, libc::SIGHUP);
                    view.stopping.set(true);
                    app.error(error);
                }
                app.sync();
            }
        },
    );
}

/// Stops a session: the group first, so the agent's own children go too, then
/// SIGKILL if it has not gone after a grace period.
pub fn stop(app: &Rc<App>, id: &str) {
    let Some(view) = app.views.borrow().get(id).cloned() else {
        return;
    };
    let pid = view.pid.get();
    if pid == 0 || view.stopping.get() {
        return;
    }
    view.stopping.set(true);
    kill_group(pid, libc::SIGHUP);
    crate::power::refresh(app);

    glib::timeout_add_local_once(Duration::from_millis(1500), {
        let view = view.clone();
        move || {
            if view.pid.get() == pid {
                kill_group(pid, libc::SIGKILL);
            }
        }
    });
}

fn kill_group(pid: i32, signal: i32) {
    if pid <= 0 {
        return;
    }
    unsafe {
        libc::killpg(pid, signal);
    }
}

/// Sends text to the agent, bracketed and without an Enter unless asked.
#[allow(dead_code)]
pub fn paste(app: &Rc<App>, id: &str, text: &str, submit: bool) {
    let Some(view) = app.views.borrow().get(id).cloned() else {
        return;
    };
    match convoy_core::history::paste(text, submit) {
        Ok(payload) => view.terminal.feed_child(payload.as_bytes()),
        Err(error) => app.error(error),
    }
}

/// The whole buffer as plain text.
///
/// `text_format` and not `text_range_format`: agents draw their interface with
/// absolute cursor positioning, and a row range returns none of it. Measured
/// on Codex, a range read gave zero characters for a full screen of output.
pub fn buffer_text(terminal: &Terminal) -> String {
    terminal
        .text_format(Format::Text)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn contents_changed(app: &Rc<App>, id: &str) {
    let Some(view) = app.views.borrow().get(id).cloned() else {
        return;
    };
    detect_codex_session(app, id, &view);

    let elapsed = view.saved_at.get().elapsed();
    if elapsed >= SAVE_INTERVAL {
        save_output(app, id, &view);
        return;
    }
    if view.save_pending.replace(true) {
        return;
    }
    glib::timeout_add_local_once(SAVE_INTERVAL - elapsed, {
        let app = app.clone();
        let id = id.to_string();
        let view = view.clone();
        move || {
            view.save_pending.set(false);
            save_output(&app, &id, &view);
        }
    });
}

fn save_output(app: &Rc<App>, id: &str, view: &Rc<SessionView>) {
    view.saved_at.set(Instant::now());
    let history = History::new(app.storage.history());
    if let Err(error) = history.save(id, &buffer_text(&view.terminal)) {
        app.error(format!("Could not save terminal output: {error}"));
    }
}

/// Codex prints the command to resume a conversation; capturing it is the only
/// way to reopen that exact conversation later. Terminal text is used for this
/// one fact and never to guess whether the agent has finished.
fn detect_codex_session(app: &Rc<App>, id: &str, view: &Rc<SessionView>) {
    let current = {
        let workspace = app.workspace.borrow();
        let Some(session) = workspace.state().sessions.iter().find(|s| s.id == id) else {
            return;
        };
        if session.agent != Agent::Codex {
            return;
        }
        session.provider_id.clone()
    };

    let text = buffer_text(&view.terminal);
    let Some(found) = text
        .split("codex resume ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .find(|candidate| UUID.is_match(candidate))
    else {
        return;
    };
    if found == current {
        return;
    }
    let found = found.to_string();
    let id = id.to_string();
    let outcome = {
        let mut workspace = app.workspace.borrow_mut();
        workspace
            .update(move |state| {
                if let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) {
                    session.provider_id = found;
                }
                Ok(())
            })
            .map(|_| ())
    };
    if let Err(error) = outcome {
        app.error(error);
    }
}

fn exited(app: &Rc<App>, id: &str, status: i32) {
    let Some(view) = app.views.borrow().get(id).cloned() else {
        return;
    };
    if view.pid.get() == 0 {
        return;
    }
    view.pid.set(0);
    view.terminal.set_pty(None);
    save_output(app, id, &view);

    // VTE reports the raw wait status, exactly as waitpid does.
    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    };
    let cause = if view.hibernating.get() {
        ExitCause::Hibernated
    } else if view.stopping.get() {
        ExitCause::Stopped
    } else {
        ExitCause::Exited(code)
    };
    view.stopping.set(false);
    view.hibernating.set(false);
    view.agent_state.set(None);
    view.status_at.set(0.0);

    if let Err(error) = finish_session(&mut app.workspace.borrow_mut(), id, cause) {
        app.error(error);
    }
    app.sync();
    crate::power::refresh(app);
    crate::queue_ui::after_exit(app, id, cause);
}

/// Writes straight to the pty master. Kept as the escape hatch for input that
/// must not go through VTE's own encoding.
#[allow(dead_code)]
pub fn write_raw(view: &SessionView, bytes: &[u8]) -> isize {
    let Some(pty) = view.terminal.pty() else {
        return -1;
    };
    unsafe {
        libc::write(
            pty.fd().as_raw_fd(),
            bytes.as_ptr() as *const libc::c_void,
            bytes.len(),
        )
    }
}

/// What the agent has printed: the live buffer when the terminal is open,
/// otherwise the bounded excerpt kept on disk. This is the text that feeds a
/// review brief, and it is data — never instructions.
pub fn saved_output(app: &Rc<App>, id: &str) -> String {
    let live = app
        .views
        .borrow()
        .get(id)
        .map(|view| buffer_text(&view.terminal))
        .unwrap_or_default();
    if !live.trim().is_empty() {
        return live;
    }
    History::new(app.storage.history())
        .read(id)
        .unwrap_or_default()
}
