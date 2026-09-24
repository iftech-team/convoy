//! Application state and the two refresh paths.
//!
//! The Electron renderer rebuilt its whole DOM on every change. Widgets carry
//! state that a rebuild destroys — focus, scroll position, selection, and in a
//! terminal the pty itself — so this port updates instead:
//!
//! * [`App::sync`] aligns the list models with the workspace, touching only
//!   rows whose contents actually differ.
//! * [`App::refresh_selection`] updates the scalar parts of the chrome: the
//!   title, the path, and which buttons are usable.

use adw::prelude::*;
use convoy_core::model::{Agent, Project, Session};
use convoy_core::{Storage, Workspace};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use crate::objects::{SessionObject, SidebarItem};

/// One session's terminal. The widget outlives the process: a stopped session
/// keeps its output on screen until the tab is closed.
pub struct SessionView {
    /// Kept alive across project switches so a running agent survives
    /// navigating away and back. The scrolled window around it is disposable:
    /// `AdwTabView` does not unparent a closed page's child straight away, so
    /// reusing that container would hit an assertion on the next append.
    pub terminal: vte4::Terminal,
    pub page: RefCell<Option<adw::TabPage>>,
    /// Process group leader, or 0 when nothing is running.
    pub pid: Cell<i32>,
    /// Set when the user asked to stop, so the exit is reported as such
    /// rather than as a failure.
    pub stopping: Cell<bool>,
    pub saved_at: Cell<Instant>,
    pub save_pending: Cell<bool>,
}

impl SessionView {
    pub fn running(&self) -> bool {
        self.pid.get() != 0
    }
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Selection {
    pub project: Option<String>,
    pub session: Option<String>,
}

/// Widgets whose contents depend on the selection.
pub struct Chrome {
    pub title: adw::WindowTitle,
    pub start: gtk::Button,
    pub stop: gtk::Button,
    pub new_session: gtk::Button,
    pub running_label: gtk::Label,
    pub stack: gtk::Stack,
    pub empty: adw::StatusPage,
}

pub struct App {
    pub storage: Storage,
    pub executable: PathBuf,
    pub workspace: RefCell<Workspace>,
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    pub roots: gtk::gio::ListStore,
    pub sessions: gtk::gio::ListStore,
    pub tabs: adw::TabView,
    pub views: RefCell<HashMap<String, Rc<SessionView>>>,
    pub selection: RefCell<Selection>,
    pub chrome: Chrome,
    /// Guards the tab-selection handler while the tab set is being rebuilt.
    pub rebuilding: Cell<bool>,
    pub search: RefCell<String>,
    /// Actions of the session menu, enabled and disabled by
    /// [`App::refresh_selection`].
    pub actions: gtk::gio::SimpleActionGroup,
    /// Sessions in the middle of a worktree operation. A worktree moving under
    /// a running agent would be worse than making the user wait.
    pub busy: RefCell<std::collections::HashSet<String>>,
    pub git_status: gtk::Label,
    /// The session shown in the second pane, when the view is split.
    pub split: RefCell<Option<String>>,
    pub panes: gtk::Paned,
    /// Rebuilt whenever the quick commands change, so the menu always matches
    /// what is saved and what the selected project is allowed to see.
    pub quick_menu: gtk::gio::Menu,
}

/// Runs blocking work off the main loop and returns to it with the result.
/// Git and provider calls are short but not instant, and the UI must not stop
/// while `git fetch` talks to a remote.
pub fn background<T, W, D>(app: &Rc<App>, work: W, done: D)
where
    T: Send + 'static,
    W: FnOnce() -> convoy_core::Result<T> + Send + 'static,
    D: Fn(&Rc<App>, T) + 'static,
{
    let (sender, receiver) = async_channel::bounded(1);
    gtk::gio::spawn_blocking(move || {
        let _ = sender.send_blocking(work());
    });
    glib::spawn_future_local({
        let app = app.clone();
        async move {
            match receiver.recv().await {
                Ok(Ok(value)) => done(&app, value),
                Ok(Err(error)) => app.error(error),
                Err(_) => {}
            }
        }
    });
}

impl App {
    /// Shows a failure without stealing focus. Everything that can fail —
    /// a missing folder, a rejected mutation, an agent that will not start —
    /// arrives here.
    pub fn error(&self, message: impl std::fmt::Display) {
        let toast = adw::Toast::builder()
            .title(message.to_string())
            .timeout(6)
            .build();
        self.toasts.add_toast(toast);
    }

    pub fn selected_project(&self) -> Option<Project> {
        let id = self.selection.borrow().project.clone()?;
        self.workspace
            .borrow()
            .state()
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
    }

    pub fn selected_session(&self) -> Option<Session> {
        let id = self.selection.borrow().session.clone()?;
        self.workspace
            .borrow()
            .state()
            .sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
    }

    /// Sessions of the selected project, in the order the sidebar shows them:
    /// pinned first, archived hidden.
    pub fn visible_sessions(&self) -> Vec<Session> {
        let Some(project) = self.selection.borrow().project.clone() else {
            return Vec::new();
        };
        let needle = self.search.borrow().to_lowercase();
        let workspace = self.workspace.borrow();
        let mut sessions: Vec<Session> = workspace
            .state()
            .sessions
            .iter()
            .filter(|session| session.project_id == project && !session.is_archived())
            .filter(|session| {
                needle.is_empty() || session.title.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        sessions.sort_by_key(|session| !session.is_pinned());
        sessions
    }

    /// Rebuilds the sidebar tree and the session model from the workspace.
    pub fn sync(&self) {
        self.sync_projects();
        self.sync_sessions();
        self.sync_quick_commands();
        self.refresh_selection();
    }

    /// Quick commands scoped to the selected project, plus the global ones.
    fn sync_quick_commands(&self) {
        self.quick_menu.remove_all();
        let project = self.selection.borrow().project.clone();
        let workspace = self.workspace.borrow();
        let mut any = false;
        for command in &workspace.state().quick_commands {
            let scoped = command
                .project_id
                .as_deref()
                .is_none_or(|owner| Some(owner) == project.as_deref());
            if !scoped {
                continue;
            }
            any = true;
            let item = gtk::gio::MenuItem::new(
                Some(&if command.submit {
                    format!("{} ⏎", command.title)
                } else {
                    command.title.clone()
                }),
                None,
            );
            item.set_action_and_target_value(
                Some("session.quick"),
                Some(&command.id.to_variant()),
            );
            self.quick_menu.append_item(&item);
        }
        if !any {
            self.quick_menu.append(
                Some("No quick commands yet"),
                Some("session.manage-commands"),
            );
        }
        let manage = gtk::gio::Menu::new();
        manage.append(Some("Manage quick commands…"), Some("session.manage-commands"));
        self.quick_menu.append_section(None, &manage);
    }

    fn sync_projects(&self) {
        let workspace = self.workspace.borrow();
        let needle = self.search.borrow().to_lowercase();
        let running: HashMap<String, u32> = {
            let views = self.views.borrow();
            let mut counts: HashMap<String, u32> = HashMap::new();
            for session in &workspace.state().sessions {
                if views.get(&session.id).is_some_and(|view| view.running()) {
                    *counts.entry(session.project_id.clone()).or_default() += 1;
                }
            }
            counts
        };

        // Groups come from `project.group`, which discovery fills in with the
        // name of the folder the projects were found under.
        let mut groups: Vec<String> = Vec::new();
        let mut rows: Vec<(Option<String>, SidebarItem)> = Vec::new();
        for project in &workspace.state().projects {
            // A project stays visible when one of its sessions matches, so
            // searching for a session name still shows the way to it.
            let matches = needle.is_empty()
                || project.title.to_lowercase().contains(&needle)
                || project.path.to_string_lossy().to_lowercase().contains(&needle)
                || workspace.state().sessions.iter().any(|session| {
                    session.project_id == project.id
                        && !session.is_archived()
                        && session.title.to_lowercase().contains(&needle)
                });
            if !matches {
                continue;
            }
            let item = SidebarItem::project(
                &project.id,
                &project.title,
                &project.path.to_string_lossy(),
            );
            item.set_running(running.get(&project.id).copied().unwrap_or(0));
            let group = project.group.clone().filter(|name| !name.is_empty());
            if let Some(name) = &group {
                if !groups.contains(name) {
                    groups.push(name.clone());
                }
            }
            rows.push((group, item));
        }

        self.roots.remove_all();
        let mut headings: HashMap<String, SidebarItem> = HashMap::new();
        for name in &groups {
            let heading = SidebarItem::heading(name);
            headings.insert(name.clone(), heading.clone());
            self.roots.append(&heading);
        }
        for (group, item) in rows {
            match group.and_then(|name| headings.get(&name).cloned()) {
                Some(heading) => heading.push(&item),
                None => self.roots.append(&item),
            }
        }
    }

    fn sync_sessions(&self) {
        let sessions = self.visible_sessions();
        let views = self.views.borrow();

        // Align in place: a removed row would otherwise take the selection
        // with it, and a rebuilt row loses the tab it is bound to.
        let mut index = 0u32;
        for session in &sessions {
            let running = views.get(&session.id).is_some_and(|view| view.running());
            let existing: Option<SessionObject> = self
                .sessions
                .item(index)
                .and_then(|item| item.downcast().ok());
            let object = match existing {
                Some(object) if object.id() == session.id => object,
                _ => {
                    let object = SessionObject::new(&session.id);
                    self.sessions.insert(index, &object);
                    object
                }
            };
            object.set_title(session.title.clone());
            object.set_agent(session.agent.as_str());
            object.set_subtitle(describe(session, running));
            object.set_running(running);
            object.set_pinned(session.is_pinned());
            object.set_archived(session.is_archived());
            index += 1;
        }
        while self.sessions.n_items() > index {
            self.sessions.remove(index);
        }
    }

    /// The second half of the Electron `render()`: everything scalar.
    pub fn refresh_selection(&self) {
        let project = self.selected_project();
        let session = self.selected_session();
        let running_total = self
            .views
            .borrow()
            .values()
            .filter(|view| view.running())
            .count();

        match &project {
            Some(project) => {
                self.chrome.title.set_title(&project.title);
                self.chrome
                    .title
                    .set_subtitle(&project.path.to_string_lossy());
            }
            None => {
                self.chrome.title.set_title("Convoy");
                self.chrome.title.set_subtitle("");
            }
        }
        self.chrome.new_session.set_sensitive(project.is_some());

        let view = session
            .as_ref()
            .and_then(|session| self.views.borrow().get(&session.id).cloned());
        let running = view.as_ref().is_some_and(|view| view.running());
        let startable = session.as_ref().is_some_and(|session| {
            !running && !session.is_archived() && !session.worktree_removed()
        });
        self.chrome.start.set_sensitive(startable);
        self.chrome.start.set_label(
            if session.as_ref().is_some_and(|session| session.started) {
                "Resume"
            } else {
                "Start"
            },
        );
        self.chrome.stop.set_sensitive(running);
        self.refresh_actions(session.as_ref(), running);

        self.chrome.running_label.set_label(&match running_total {
            0 => "No sessions running".to_string(),
            1 => "1 session running".to_string(),
            count => format!("{count} sessions running"),
        });

        let page = if self.tabs.n_pages() == 0 {
            "empty"
        } else {
            "terminals"
        };
        self.chrome.stack.set_visible_child_name(page);
        self.chrome.empty.set_title(match project {
            Some(_) => "No sessions yet",
            None => "No project selected",
        });
        self.chrome.empty.set_description(Some(match self.selection.borrow().project {
            Some(_) => "Start a session to run Claude Code or Codex in this folder.",
            None => "Open a folder to begin.",
        }));
    }
}

impl App {
    /// Which menu entries make sense for the selected session. The conditions
    /// are the ones the Electron renderer applied to its `#session-menu`.
    fn refresh_actions(&self, session: Option<&Session>, running: bool) {
        let busy = session.is_some_and(|session| self.busy.borrow().contains(&session.id));
        let idle = session.is_some() && !running && !busy;
        let has_others = session.is_some_and(|session| {
            self.visible_sessions()
                .iter()
                .any(|other| other.id != session.id)
        });
        let enable = |name: &str, value: bool| {
            if let Some(action) = self.actions.lookup_action(name) {
                if let Ok(action) = action.downcast::<gtk::gio::SimpleAction>() {
                    action.set_enabled(value);
                }
            }
        };
        enable("recover", idle);
        enable("edit", session.is_some());
        enable("pin", session.is_some());
        enable("archive", idle);
        enable("review", session.is_some());
        enable(
            "feedback",
            session.is_some_and(|session| session.review_of.is_some()),
        );
        enable("usage", session.is_some());
        enable("history", session.is_some());
        enable(
            "worktree",
            idle
                && session.is_some_and(|session| {
                    !session.started
                        && session.working_directory.is_none()
                        && session.review_of.is_none()
                        && !session.is_archived()
                }),
        );
        enable(
            "remove-worktree",
            idle && session.is_some_and(|session| session.owns_worktree()),
        );
        enable("git-status", session.is_some());
        enable("split", has_others);
        enable("unsplit", self.split.borrow().is_some());
    }
}

fn describe(session: &Session, running: bool) -> String {
    let agent = match session.agent {
        Agent::Claude => "Claude Code",
        Agent::Codex => "Codex",
    };
    let state = if running {
        "running"
    } else if session.started {
        "stopped"
    } else {
        "never started"
    };
    format!("{agent} · {state}")
}
