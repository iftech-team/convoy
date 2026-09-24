//! Files & Changes.
//!
//! A dialog with four views over one folder: working-tree changes, the files
//! themselves, the commit log and the branches. The Electron build put Git
//! behind a single dialog with a plain text diff; here the diff is highlighted,
//! numbered and can be read side by side, because GtkSourceView does that work.
//!
//! Every destructive action asks first, and every one of them is narrow:
//! discard restores one path, hunk discard applies one reversed patch and
//! refuses if the diff has moved on, reset takes an object name and never a
//! flag.

use adw::prelude::*;
use convoy_core::git::diff::{digest, hunks, split};
use convoy_core::git::mutate::Action;
use convoy_core::git::{ReadRequest, Snapshot};
use convoy_core::Git;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::objects::{ChangeObject, CommitObject};
use crate::preview::{language_for, Preview};
use crate::state::{background, background_result, App};

pub struct FilesView {
    pub app: Rc<App>,
    pub dialog: adw::Dialog,
    pub directory: PathBuf,
    pub session: Option<String>,
    pub changes: gtk::gio::ListStore,
    pub files: gtk::StringList,
    pub log: gtk::gio::ListStore,
    pub branches: gtk::StringList,
    pub preview: Preview,
    pub side_by_side: gtk::ToggleButton,
    pub message: gtk::TextView,
    pub branch_label: gtk::Label,
    /// Bumped on every new request so a slow read cannot overwrite a newer
    /// selection when it finally arrives.
    pub generation: Cell<u64>,
    pub snapshot: RefCell<Option<Snapshot>>,
    /// The diff currently shown, and its hash. Discarding a hunk is refused
    /// unless the diff is still exactly this one.
    pub current: RefCell<Option<(ReadRequest, String)>>,
    /// One Git write at a time per folder.
    pub busy: Cell<bool>,
}

pub fn open(app: &Rc<App>) {
    let _ = open_view(app);
}

/// As [`open`], but hands back the view so a test can look at it.
pub fn open_view(app: &Rc<App>) -> Option<Rc<FilesView>> {
    let (directory, session) = match app.selected_session() {
        Some(session) => match convoy_core::session::directory_for(&app.workspace.borrow(), &session.id) {
            Ok(directory) => (directory, Some(session.id)),
            Err(error) => {
                app.error(error);
                return None;
            }
        },
        None => match app.selected_project() {
            Some(project) => (project.path, None),
            None => {
                app.error("Select a project first.");
                return None;
            }
        },
    };

    let preview = Preview::new();
    let changes = gtk::gio::ListStore::new::<ChangeObject>();
    let files = gtk::StringList::new(&[]);
    let log = gtk::gio::ListStore::new::<CommitObject>();
    let branches = gtk::StringList::new(&[]);

    let message = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .build();

    let dialog = adw::Dialog::builder()
        .title("Files & Changes")
        .content_width(1100)
        .content_height(720)
        .build();

    let view = Rc::new(FilesView {
        app: app.clone(),
        dialog: dialog.clone(),
        directory,
        session,
        changes,
        files,
        log,
        branches,
        preview,
        side_by_side: gtk::ToggleButton::builder()
            .icon_name("view-dual-symbolic")
            .tooltip_text("Side by side")
            .build(),
        message,
        branch_label: gtk::Label::new(None),
        generation: Cell::new(0),
        snapshot: RefCell::new(None),
        current: RefCell::new(None),
        busy: Cell::new(false),
    });

    dialog.set_child(Some(&layout(&view)));
    dialog.present(Some(&app.window));
    refresh(&view);
    Some(view)
}

/// Re-reads the folder. Exposed so a test can wait for the result.
pub fn reload(view: &Rc<FilesView>) {
    refresh(view);
}

/// Shows one selection, as clicking it would.
pub fn select(view: &Rc<FilesView>, request: ReadRequest) {
    read(view, request);
}

/// Which page of the preview is showing: "text", "split", "markdown",
/// "image" or "notice".
pub fn preview_page(view: &Rc<FilesView>) -> Option<String> {
    view.preview
        .root
        .visible_child_name()
        .map(|name| name.to_string())
}

fn layout(view: &Rc<FilesView>) -> gtk::Widget {
    // One preview, one splitter. Only the list on the left changes with the
    // view, because a widget has exactly one parent.
    let stack = adw::ViewStack::new();
    stack.add_titled_with_icon(
        &changes_list(view),
        Some("changes"),
        "Changes",
        "document-edit-symbolic",
    );
    stack.add_titled_with_icon(&files_list(view), Some("files"), "Files", "folder-symbolic");
    stack.add_titled_with_icon(
        &log_list(view),
        Some("log"),
        "Log",
        "document-open-recent-symbolic",
    );
    stack.add_titled_with_icon(
        &branches_page(view),
        Some("branches"),
        "Branches",
        "media-playlist-shuffle-symbolic",
    );
    let body = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&stack)
        .end_child(&view.preview.root)
        .resize_start_child(false)
        .resize_end_child(true)
        .shrink_start_child(false)
        .position(340)
        .build();

    let header = adw::HeaderBar::builder()
        .title_widget(&adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build())
        .build();

    let refresh_button = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Refresh")
        .build();
    refresh_button.connect_clicked({
        let view = view.clone();
        move |_| refresh(&view)
    });
    header.pack_start(&refresh_button);
    header.pack_start(&view.branch_label);
    view.branch_label.add_css_class("dim-label");

    let remote = gtk::gio::Menu::new();
    remote.append(Some("Fetch"), Some("files.fetch"));
    remote.append(Some("Pull (fast-forward only)"), Some("files.pull"));
    remote.append(Some("Push"), Some("files.push"));
    remote.append(Some("Create pull request"), Some("files.pr"));
    header.pack_end(
        &gtk::MenuButton::builder()
            .icon_name("network-transmit-receive-symbolic")
            .tooltip_text("Remote")
            .menu_model(&remote)
            .build(),
    );
    header.pack_end(&view.side_by_side);

    view.side_by_side.connect_toggled({
        let view = view.clone();
        move |_| reshow(&view)
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    toolbar.add_bottom_bar(&commit_bar(view));
    register_actions(view);
    toolbar.upcast()
}

fn changes_list(view: &Rc<FilesView>) -> gtk::Widget {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let status = gtk::Label::builder().width_chars(2).build();
        status.add_css_class("monospace");
        status.add_css_class("dim-label");
        let name = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .build();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&status);
        row.append(&name);
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(row), Some(change)) = (
            item.child().and_downcast::<gtk::Box>(),
            item.item().and_downcast::<ChangeObject>(),
        ) else {
            return;
        };
        let Some(status) = row.first_child().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(name) = status.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        status.set_label(&change.status());
        name.set_label(&change.path());
        name.set_tooltip_text(Some(&change.path()));
        if change.conflict() {
            name.add_css_class("error");
        } else {
            name.remove_css_class("error");
        }
    });

    let selection = gtk::SingleSelection::builder()
        .model(&view.changes)
        .autoselect(false)
        .can_unselect(true)
        .build();
    selection.connect_selected_item_notify({
        let view = view.clone();
        move |selection| {
            let Some(change) = selection.selected_item().and_downcast::<ChangeObject>() else {
                return;
            };
            let request = if change.untracked() {
                ReadRequest::Untracked { path: change.path() }
            } else if change.staged() {
                ReadRequest::Staged { path: change.path() }
            } else {
                ReadRequest::Unstaged { path: change.path() }
            };
            read(&view, request);
        }
    });

    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .build();
    list.add_css_class("navigation-sidebar");

    let bar = gtk::ActionBar::new();
    for (label, action, css) in [
        ("Stage", "files.stage", None),
        ("Unstage", "files.unstage", None),
        ("Stage all", "files.stage-all", None),
        ("Discard", "files.discard", Some("destructive-action")),
        ("Trash", "files.trash", Some("destructive-action")),
        ("Discard hunk", "files.discard-hunk", Some("destructive-action")),
    ] {
        let button = gtk::Button::with_label(label);
        button.set_action_name(Some(action));
        if let Some(css) = css {
            button.add_css_class(css);
        }
        bar.pack_start(&button);
    }

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
    box_.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
    box_.append(&bar);
    box_.upcast()
}

fn files_list(view: &Rc<FilesView>) -> gtk::Widget {
    let selection = gtk::SingleSelection::builder()
        .model(&view.files)
        .autoselect(false)
        .can_unselect(true)
        .build();
    selection.connect_selected_item_notify({
        let view = view.clone();
        move |selection| {
            let Some(item) = selection.selected_item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            read(&view, ReadRequest::File { path: item.string().to_string() });
        }
    });
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&string_factory())
        .build();
    list.add_css_class("navigation-sidebar");
    gtk::ScrolledWindow::builder()
        .child(&list)
        .width_request(320)
        .build()
        .upcast()
}

fn log_list(view: &Rc<FilesView>) -> gtk::Widget {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let subject = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let detail = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        detail.add_css_class("dim-label");
        detail.add_css_class("caption");
        let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row.append(&subject);
        row.append(&detail);
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(row), Some(commit)) = (
            item.child().and_downcast::<gtk::Box>(),
            item.item().and_downcast::<CommitObject>(),
        ) else {
            return;
        };
        let Some(subject) = row.first_child().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(detail) = subject.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        subject.set_label(&commit.subject());
        detail.set_label(&format!(
            "{} · {} · {}",
            commit.short(),
            commit.author(),
            commit.date().split('T').next().unwrap_or_default()
        ));
    });

    let selection = gtk::SingleSelection::builder()
        .model(&view.log)
        .autoselect(false)
        .can_unselect(true)
        .build();
    selection.connect_selected_item_notify({
        let view = view.clone();
        move |selection| {
            let Some(commit) = selection.selected_item().and_downcast::<CommitObject>() else {
                return;
            };
            read(&view, ReadRequest::Commit { commit: commit.id() });
        }
    });

    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .build();
    list.add_css_class("navigation-sidebar");

    let bar = gtk::ActionBar::new();
    for (label, action) in [
        ("Revert", "files.revert"),
        ("Reset soft", "files.reset-soft"),
        ("Reset mixed", "files.reset-mixed"),
    ] {
        let button = gtk::Button::with_label(label);
        button.set_action_name(Some(action));
        button.add_css_class("destructive-action");
        bar.pack_start(&button);
    }

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
    box_.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
    box_.append(&bar);
    box_.upcast()
}

fn branches_page(view: &Rc<FilesView>) -> gtk::Widget {
    let selection = gtk::SingleSelection::builder()
        .model(&view.branches)
        .autoselect(false)
        .can_unselect(true)
        .build();
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&string_factory())
        .build();
    list.add_css_class("navigation-sidebar");

    let bar = gtk::ActionBar::new();
    let switch = gtk::Button::with_label("Switch to selected");
    switch.connect_clicked({
        let view = view.clone();
        let selection = selection.clone();
        move |_| {
            let Some(item) = selection.selected_item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            confirm_and_mutate(
                &view,
                Action::Switch { branch: item.string().to_string() },
                None,
            );
        }
    });
    let create = gtk::Button::with_label("New branch…");
    create.connect_clicked({
        let view = view.clone();
        move |_| new_branch(&view)
    });
    bar.pack_start(&switch);
    bar.pack_start(&create);

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
    box_.append(&gtk::ScrolledWindow::builder().child(&list).vexpand(true).build());
    box_.append(&bar);
    box_.upcast()
}

fn string_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        item.set_child(Some(
            &gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Start)
                .build(),
        ));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(label), Some(value)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) else {
            return;
        };
        label.set_label(&value.string());
        label.set_tooltip_text(Some(&value.string()));
    });
    factory
}

fn commit_bar(view: &Rc<FilesView>) -> gtk::Widget {
    let frame = gtk::ScrolledWindow::builder()
        .child(&view.message)
        .height_request(64)
        .hexpand(true)
        .build();
    frame.add_css_class("card");

    let commit = gtk::Button::with_label("Commit");
    commit.add_css_class("suggested-action");
    commit.set_action_name(Some("files.commit"));
    let amend = gtk::Button::with_label("Amend");
    amend.set_action_name(Some("files.amend"));
    let generate = gtk::Button::with_label("Generate with Claude");
    generate.set_action_name(Some("files.generate"));

    let buttons = gtk::Box::new(gtk::Orientation::Vertical, 6);
    buttons.append(&commit);
    buttons.append(&amend);
    buttons.append(&generate);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_top(8);
    row.set_margin_bottom(8);
    row.set_margin_start(12);
    row.set_margin_end(12);
    row.append(&frame);
    row.append(&buttons);
    row.upcast()
}

// ---------------------------------------------------------------------------
// Behaviour.
// ---------------------------------------------------------------------------

fn message_text(view: &Rc<FilesView>) -> String {
    let buffer = view.message.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

fn selected_change(view: &Rc<FilesView>) -> Option<(String, Option<String>)> {
    let (request, _) = view.current.borrow().clone()?;
    let path = match request {
        ReadRequest::Untracked { path }
        | ReadRequest::Staged { path }
        | ReadRequest::Unstaged { path }
        | ReadRequest::File { path } => path,
        ReadRequest::Commit { .. } => return None,
    };
    let original = view
        .snapshot
        .borrow()
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .changes
                .iter()
                .find(|change| change.path == path)
                .and_then(|change| change.original.clone())
        });
    Some((path, original))
}

fn selected_commit(view: &Rc<FilesView>) -> Option<String> {
    match view.current.borrow().clone() {
        Some((ReadRequest::Commit { commit }, _)) => Some(commit),
        _ => None,
    }
}

/// Reloads everything the folder shows. Cheap enough to run after every write,
/// which is what keeps the panel honest about the repository's real state.
fn refresh(view: &Rc<FilesView>) {
    let directory = view.directory.clone();
    background(
        &view.app,
        move || Git::default().snapshot(&directory),
        {
            let view = view.clone();
            move |_, snapshot: Snapshot| {
                view.changes.remove_all();
                for change in &snapshot.changes {
                    view.changes.append(&ChangeObject::from(change));
                }
                while view.files.n_items() > 0 {
                    view.files.remove(0);
                }
                for name in &snapshot.files {
                    view.files.append(name);
                }
                view.log.remove_all();
                for commit in &snapshot.log {
                    view.log.append(&CommitObject::from(commit));
                }
                while view.branches.n_items() > 0 {
                    view.branches.remove(0);
                }
                for branch in &snapshot.branches {
                    view.branches.append(branch);
                }
                view.branch_label.set_label(&format!(
                    "{} · {} changed",
                    snapshot.branch,
                    snapshot.changes.len()
                ));
                *view.snapshot.borrow_mut() = Some(snapshot);

                // The shown diff may have moved on; read it again so its hash
                // matches what a hunk discard would be applied to.
                let current = view.current.borrow().clone();
                if let Some((request, _)) = current {
                    read(&view, request);
                }
            }
        },
    );
}

/// Reads one selection. A generation counter makes a slow read harmless: if
/// the selection has moved on by the time it returns, the result is dropped.
fn read(view: &Rc<FilesView>, request: ReadRequest) {
    let generation = view.generation.get() + 1;
    view.generation.set(generation);

    let directory = view.directory.clone();
    let wanted = request.clone();

    // An image is bytes, not text, and takes a different path entirely.
    if let ReadRequest::File { path } = &request {
        let name = path.clone();
        background(
            &view.app,
            move || convoy_core::files::file_content(&directory, &name),
            {
                let view = view.clone();
                let request = request.clone();
                move |_, content: convoy_core::files::Content| {
                    if view.generation.get() != generation {
                        return;
                    }
                    *view.current.borrow_mut() = Some((request.clone(), String::new()));
                    match content {
                        convoy_core::files::Content::Image { bytes, .. } => {
                            view.preview.show_image(&bytes)
                        }
                        convoy_core::files::Content::Text { text, markdown: true } => {
                            view.preview.show_markdown(&text)
                        }
                        convoy_core::files::Content::Text { text, .. } => {
                            let language = match &request {
                                ReadRequest::File { path } => language_for(path),
                                _ => None,
                            };
                            view.preview.show_text(&text, language.as_deref());
                        }
                    }
                }
            },
        );
        return;
    }

    background(
        &view.app,
        move || Git::default().read(&directory, &wanted),
        {
            let view = view.clone();
            move |_, text: String| {
                if view.generation.get() != generation {
                    return;
                }
                *view.current.borrow_mut() = Some((request.clone(), digest(&text)));
                show(&view, &request, &text);
            }
        },
    );
}

fn show(view: &Rc<FilesView>, request: &ReadRequest, text: &str) {
    if text.trim().is_empty() {
        view.preview
            .show_notice("Nothing to show", "This selection has no differences.");
        return;
    }
    let is_diff = !matches!(request, ReadRequest::Untracked { .. });
    if is_diff && view.side_by_side.is_active() {
        view.preview.show_split(&split(text));
        return;
    }
    let language = match request {
        ReadRequest::Untracked { path } => language_for(path),
        _ => Some("diff".to_string()),
    };
    view.preview.show_text(text, language.as_deref());
}

/// Re-renders the current selection after the side-by-side toggle moves.
fn reshow(view: &Rc<FilesView>) {
    let current = view.current.borrow().clone();
    if let Some((request, _)) = current {
        read(view, request);
    }
}

/// Runs a Git write, asking first when it cannot be undone. One write at a
/// time: two overlapping operations on the same index would leave a mess no
/// message could explain.
fn confirm_and_mutate(view: &Rc<FilesView>, action: Action, confirmation: Option<(&str, String)>) {
    if view.busy.get() {
        view.app.error("Wait for the current Git operation.");
        return;
    }
    let Some((heading, detail)) = confirmation else {
        run(view, action);
        return;
    };

    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(detail)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("continue", "Continue");
    dialog.set_response_appearance("continue", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let view = view.clone();
        move |_, response| {
            if response == "continue" {
                run(&view, action.clone());
            }
        }
    });
    dialog.present(Some(&view.dialog));
}

fn run(view: &Rc<FilesView>, action: Action) {
    view.busy.set(true);
    let directory = view.directory.clone();
    background_result(
        &view.app,
        move || Git::default().mutate(&directory, &action),
        {
            let view = view.clone();
            move |app, outcome: convoy_core::Result<String>| {
                // The lock is released whichever way it went: a refused write
                // must not leave the panel unusable.
                view.busy.set(false);
                match outcome {
                    Ok(output) => {
                        let text = output.trim().to_string();
                        if !text.is_empty() {
                            app.toasts.add_toast(
                                adw::Toast::builder()
                                    .title(convoy_core::json::head(&text, 120))
                                    .build(),
                            );
                        }
                    }
                    Err(error) => app.error(error),
                }
                refresh(&view);
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Actions.
// ---------------------------------------------------------------------------

/// A toolbar action: its name and what it does with the current selection.
type Entry = (&'static str, fn(&Rc<FilesView>));

fn register_actions(view: &Rc<FilesView>) {
    let group = gtk::gio::SimpleActionGroup::new();
    let simple: [Entry; 15] = [
        ("stage", |view| with_path(view, |view, path, original| {
            confirm_and_mutate(view, Action::Stage { path, original }, None)
        })),
        ("unstage", |view| with_path(view, |view, path, original| {
            confirm_and_mutate(view, Action::Unstage { path, original }, None)
        })),
        ("stage-all", |view| confirm_and_mutate(view, Action::StageAll, None)),
        ("discard", |view| with_path(view, |view, path, _| {
            let detail = path.clone();
            confirm_and_mutate(
                view,
                Action::Discard { path },
                Some(("Discard changes to this file?", detail)),
            )
        })),
        ("trash", trash),
        ("discard-hunk", discard_hunk),
        ("commit", |view| commit(view, false)),
        ("amend", |view| commit(view, true)),
        ("generate", generate),
        ("fetch", |view| confirm_and_mutate(view, Action::Fetch, None)),
        ("pull", |view| confirm_and_mutate(view, Action::Pull, None)),
        ("push", |view| confirm_and_mutate(view, Action::Push, None)),
        ("pr", create_pr),
        ("revert", |view| with_commit(view, |view, commit| {
            let detail = commit.clone();
            confirm_and_mutate(
                view,
                Action::Revert { commit },
                Some(("Revert this commit?", detail)),
            )
        })),
        ("reset-soft", |view| with_commit(view, |view, commit| {
            let detail = commit.clone();
            confirm_and_mutate(
                view,
                Action::ResetSoft { commit },
                Some(("Reset to this commit, keeping the index?", detail)),
            )
        })),
    ];
    for (name, handler) in simple {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.connect_activate({
            let view = view.clone();
            move |_, _| handler(&view)
        });
        group.add_action(&action);
    }

    let reset_mixed = gtk::gio::SimpleAction::new("reset-mixed", None);
    reset_mixed.connect_activate({
        let view = view.clone();
        move |_, _| {
            with_commit(&view, |view, commit| {
                let detail = commit.clone();
                confirm_and_mutate(
                    view,
                    Action::ResetMixed { commit },
                    Some(("Reset to this commit and clear the index?", detail)),
                )
            })
        }
    });
    group.add_action(&reset_mixed);

    view.dialog.insert_action_group("files", Some(&group));
}

fn with_path(view: &Rc<FilesView>, handler: impl Fn(&Rc<FilesView>, String, Option<String>)) {
    match selected_change(view) {
        Some((path, original)) => handler(view, path, original),
        None => view.app.error("Select a file first."),
    }
}

fn with_commit(view: &Rc<FilesView>, handler: impl Fn(&Rc<FilesView>, String)) {
    match selected_commit(view) {
        Some(commit) => handler(view, commit),
        None => view.app.error("Select a commit first."),
    }
}

/// Only untracked files go to Trash: anything Git knows about can be restored
/// from Git instead.
fn trash(view: &Rc<FilesView>) {
    let Some((path, _)) = selected_change(view) else {
        return view.app.error("Select a file first.");
    };
    let untracked = view
        .snapshot
        .borrow()
        .as_ref()
        .is_some_and(|snapshot| {
            snapshot
                .changes
                .iter()
                .any(|change| change.path == path && change.untracked)
        });
    if !untracked {
        view.app.error("Only untracked files can be moved to Trash.");
        return;
    }
    let target = match convoy_core::files::paths::trash_target(&view.directory, &path) {
        Ok(target) => target,
        Err(error) => return view.app.error(error),
    };

    let dialog = adw::AlertDialog::builder()
        .heading("Move untracked file to Trash?")
        .body(path)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("trash", "Move to Trash");
    dialog.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let view = view.clone();
        move |_, response| {
            if response != "trash" {
                return;
            }
            let file = gtk::gio::File::for_path(&target);
            match file.trash(gtk::gio::Cancellable::NONE) {
                Ok(()) => refresh(&view),
                Err(error) => view.app.error(error),
            }
        }
    });
    dialog.present(Some(&view.dialog));
}

/// Reverting one hunk. The diff must still be the one on screen, hash and all,
/// or the patch would be applied to something the user never saw.
fn discard_hunk(view: &Rc<FilesView>) {
    let Some((request, hash)) = view.current.borrow().clone() else {
        return view.app.error("Select an unstaged change first.");
    };
    let ReadRequest::Unstaged { path } = request else {
        return view
            .app
            .error("Hunks can only be discarded from unstaged changes.");
    };

    let directory = view.directory.clone();
    let wanted = ReadRequest::Unstaged { path: path.clone() };
    background(
        &view.app,
        move || Git::default().read(&directory, &wanted),
        {
            let view = view.clone();
            move |app, text: String| {
                if digest(&text) != hash {
                    app.error("The diff changed. Refresh before discarding a hunk.");
                    refresh(&view);
                    return;
                }
                let parsed = hunks(&text);
                if parsed.patches.is_empty() {
                    app.error("This hunk cannot be discarded separately.");
                    return;
                }
                choose_hunk(&view, &path, hash.clone(), parsed.patches);
            }
        },
    );
}

fn choose_hunk(view: &Rc<FilesView>, path: &str, hash: String, patches: Vec<String>) {
    let labels: Vec<String> = patches
        .iter()
        .enumerate()
        .map(|(index, patch)| {
            format!(
                "{}. {}",
                index + 1,
                patch.lines().next().unwrap_or_default()
            )
        })
        .collect();
    let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    let choice = adw::ComboRow::builder().title("Hunk").model(&model).build();
    let group = adw::PreferencesGroup::new();
    group.add(&choice);

    let dialog = adw::AlertDialog::builder()
        .heading("Discard which hunk?")
        .body(path.to_string())
        .extra_child(&group)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("discard", "Discard hunk");
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_close_response("cancel");

    let path = path.to_string();
    dialog.connect_response(None, {
        let view = view.clone();
        move |_, response| {
            if response != "discard" {
                return;
            }
            confirm_and_mutate(
                &view,
                Action::DiscardHunk {
                    path: path.clone(),
                    hunk: choice.selected() as usize,
                    hash: hash.clone(),
                },
                None,
            );
        }
    });
    dialog.present(Some(&view.dialog));
}

fn commit(view: &Rc<FilesView>, amend: bool) {
    let message = message_text(view);
    let action = Action::Commit {
        message: message.clone(),
        amend,
    };
    let confirmation = amend.then(|| {
        (
            "Amend the latest commit?",
            "The previous commit is replaced. Do not amend something already pushed."
                .to_string(),
        )
    });
    confirm_and_mutate(view, action, confirmation);
    view.message.buffer().set_text("");
}

/// Asks Claude for a message. This does make a provider request, so it is
/// always the user pressing the button, and a message already typed is never
/// overwritten without asking.
fn generate(view: &Rc<FilesView>) {
    let existing = message_text(view);
    if !existing.trim().is_empty() {
        let dialog = adw::AlertDialog::builder()
            .heading("Replace the message you have written?")
            .body("Generating asks Claude for a new message for the staged diff.")
            .build();
        dialog.add_response("cancel", "Keep mine");
        dialog.add_response("replace", "Replace");
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_close_response("cancel");
        dialog.connect_response(None, {
            let view = view.clone();
            move |_, response| {
                if response == "replace" {
                    request_message(&view);
                }
            }
        });
        dialog.present(Some(&view.dialog));
        return;
    }
    request_message(view);
}

fn request_message(view: &Rc<FilesView>) {
    let account = {
        let workspace = view.app.workspace.borrow();
        match &view.session {
            Some(id) => workspace
                .session(id)
                .and_then(|session| {
                    convoy_core::session::prepare_account(&workspace, &view.app.storage, session)
                }),
            None => {
                let session = convoy_core::model::Session::new(
                    String::new(),
                    convoy_core::model::Agent::Claude,
                    "commit message",
                );
                convoy_core::session::prepare_account(&workspace, &view.app.storage, &session)
            }
        }
    };
    let account = match account {
        Ok(account) => account,
        Err(error) => return view.app.error(error),
    };

    let directory = view.directory.clone();
    view.app.toasts.add_toast(
        adw::Toast::builder()
            .title("Asking Claude for a commit message…")
            .build(),
    );
    background(
        &view.app,
        move || {
            convoy_core::repository::generate_message(
                &Git::default(),
                &convoy_core::StdRunner,
                &directory,
                &account.env,
            )
        },
        {
            let view = view.clone();
            move |_, message: String| view.message.buffer().set_text(&message)
        },
    );
}

fn create_pr(view: &Rc<FilesView>) {
    let directory = view.directory.clone();
    background(
        &view.app,
        move || convoy_core::repository::create_pr(&convoy_core::StdRunner, &directory),
        |app, url: String| {
            app.toasts
                .add_toast(adw::Toast::builder().title(url).timeout(10).build());
        },
    );
}

fn new_branch(view: &Rc<FilesView>) {
    let name = adw::EntryRow::builder().title("Branch name").build();
    let group = adw::PreferencesGroup::new();
    group.add(&name);
    let dialog = adw::AlertDialog::builder()
        .heading("New branch")
        .extra_child(&group)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("create", "Create and switch");
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let view = view.clone();
        move |_, response| {
            if response == "create" {
                confirm_and_mutate(
                    &view,
                    Action::Branch {
                        branch: name.text().to_string(),
                    },
                    None,
                );
            }
        }
    });
    dialog.present(Some(&view.dialog));
}
