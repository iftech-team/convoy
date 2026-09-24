//! The main window.
//!
//! Layout follows `docs/plan-refactor/04-ui-spec.md`: a navigation split view
//! with the project tree on the left and the selected project's sessions as
//! tabs on the right. Groups are real expandable nodes rather than the text
//! prefix the Electron sidebar used.

use adw::prelude::*;
use gtk::gdk;
use convoy_core::{Storage, Workspace};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::objects::SidebarItem;
use crate::state::{background, App, Chrome, Selection};
use crate::{dialogs, terminal};

pub fn build(application: &adw::Application, storage: Storage) -> Option<Rc<App>> {
    let workspace = match Workspace::load(storage.workspace_file()) {
        Ok(workspace) => workspace,
        Err(error) => {
            // A damaged or future workspace is never overwritten; the user is
            // told which file to look at and the app stops there.
            let dialog = adw::AlertDialog::builder()
                .heading("Unable to open Convoy")
                .body(format!("{error}\n\n{}", storage.workspace_file().display()))
                .build();
            dialog.add_response("quit", "Quit");
            let window = adw::ApplicationWindow::builder()
                .application(application)
                .content(&adw::StatusPage::new())
                .build();
            dialog.connect_response(None, {
                let window = window.clone();
                move |_, _| window.close()
            });
            window.present();
            dialog.present(Some(&window));
            return None;
        }
    };

    let toasts = adw::ToastOverlay::new();
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Convoy")
        .default_width(1200)
        .default_height(800)
        .width_request(760)
        .height_request(500)
        .content(&toasts)
        .build();

    let roots = gtk::gio::ListStore::new::<SidebarItem>();
    let sessions = gtk::gio::ListStore::new::<crate::objects::SessionObject>();
    let tabs = adw::TabView::new();

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search projects and sessions")
        .hexpand(true)
        .build();
    let running_label = gtk::Label::builder()
        .label("No sessions running")
        .xalign(0.0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    running_label.add_css_class("dim-label");
    running_label.add_css_class("caption");

    let selection = build_sidebar(&roots);
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&sidebar_factory())
        .build();
    list.add_css_class("navigation-sidebar");
    attach_context_menu(&list, &selection);

    let menu = gtk::gio::Menu::new();
    menu.append(Some("Open folder…"), Some("app.open-folder"));
    menu.append(Some("Import provider history…"), Some("app.import-history"));
    menu.append(Some("Activity…"), Some("app.activity"));
    menu.append(Some("Settings"), Some("app.settings"));
    menu.append(Some("About Convoy"), Some("app.about"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();

    let sidebar_header = adw::HeaderBar::builder().title_widget(&search).build();
    sidebar_header.pack_start(&menu_button);

    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .build(),
    ));
    sidebar_view.add_bottom_bar(&running_label);

    let sidebar_page = adw::NavigationPage::builder()
        .title("Convoy")
        .child(&sidebar_view)
        .build();

    let title = adw::WindowTitle::new("Convoy", "");
    let new_session = gtk::Button::builder()
        .icon_name("tab-new-symbolic")
        .tooltip_text("New session")
        .sensitive(false)
        .build();
    let files_button = gtk::Button::builder()
        .icon_name("folder-symbolic")
        .tooltip_text("Files & Changes")
        .action_name("app.files")
        .build();
    let content_header = adw::HeaderBar::builder().title_widget(&title).build();
    content_header.pack_end(&new_session);
    content_header.pack_end(&files_button);
    content_header.pack_end(
        &gtk::Button::builder()
            .icon_name("view-list-bullet-symbolic")
            .tooltip_text("Specs & tasks")
            .action_name("app.specs")
            .build(),
    );

    let empty = adw::StatusPage::builder()
        .icon_name("utilities-terminal-symbolic")
        .title("No project selected")
        .description("Open a folder to begin.")
        .build();
    // Exactly two panes, as in the Electron build. The second is empty until
    // a split is opened, and `gtk::Paned` can be nested later without changing
    // the model.
    let panes = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&tabs)
        .resize_start_child(true)
        .shrink_start_child(false)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&panes, Some("terminals"));
    stack.set_vexpand(true);

    let start = gtk::Button::with_label("Start");
    start.add_css_class("suggested-action");
    start.set_sensitive(false);
    let stop = gtk::Button::with_label("Stop");
    stop.add_css_class("destructive-action");
    stop.set_sensitive(false);
    let quick_menu = gtk::gio::Menu::new();
    let quick_button = gtk::MenuButton::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text("Quick commands")
        .menu_model(&quick_menu)
        .build();
    let session_menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("Session actions")
        .menu_model(&crate::actions::menu())
        .build();
    let git_status = gtk::Label::builder().xalign(1.0).hexpand(true).build();
    git_status.add_css_class("dim-label");
    git_status.add_css_class("caption");

    let actions = gtk::ActionBar::new();
    actions.pack_start(&start);
    actions.pack_start(&stop);
    actions.pack_start(&quick_button);
    actions.pack_start(&session_menu);
    actions.pack_end(&git_status);

    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.add_top_bar(&adw::TabBar::builder().view(&tabs).autohide(false).build());
    content_view.set_content(Some(&stack));
    content_view.add_bottom_bar(&actions);

    let content_page = adw::NavigationPage::builder()
        .title("Sessions")
        .child(&content_view)
        .build();

    let split = adw::NavigationSplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .min_sidebar_width(240.0)
        .build();
    toasts.set_child(Some(&split));

    let app = Rc::new(App {
        storage,
        executable: std::env::current_exe().unwrap_or_else(|_| "convoy".into()),
        workspace: RefCell::new(workspace),
        window: window.clone(),
        toasts,
        roots,
        sessions,
        tabs: tabs.clone(),
        views: RefCell::new(HashMap::new()),
        selection: RefCell::new(Selection::default()),
        chrome: Chrome {
            title,
            start: start.clone(),
            stop: stop.clone(),
            new_session: new_session.clone(),
            running_label,
            stack,
            empty,
        },
        rebuilding: Cell::new(false),
        search: RefCell::new(String::new()),
        actions: gtk::gio::SimpleActionGroup::new(),
        queues: RefCell::new(std::collections::HashSet::new()),
        wake_lock: Cell::new(0),
        busy: RefCell::new(std::collections::HashSet::new()),
        git_status,
        search_entry: search.clone(),
        split: RefCell::new(None),
        panes,
        quick_menu,
    });
    window.insert_action_group("session", Some(&app.actions));
    crate::actions::register(&app);
    crate::app::register_actions(application, &app);

    wire(&app, &selection, &search, &start, &stop, &new_session);
    crate::monitor::start(&app);
    app.sync();
    select_first_project(&app, &selection);
    window.present();
    Some(app)
}

/// Opening onto an empty window when there is work to show helps nobody, so
/// the first project is selected for you.
fn select_first_project(app: &Rc<App>, selection: &gtk::SingleSelection) {
    for index in 0..selection.n_items() {
        let Some(item) = selection
            .item(index)
            .and_downcast::<gtk::TreeListRow>()
            .and_then(|row| row.item())
            .and_downcast::<SidebarItem>()
        else {
            continue;
        };
        if item.is_group() {
            continue;
        }
        selection.set_selected(index);
        app.selection.borrow_mut().project = Some(item.id());
        rebuild_tabs(app);
        app.sync();
        return;
    }
}

/// Right-clicking a project offers what the Electron sidebar had no room for.
fn attach_context_menu(list: &gtk::ListView, selection: &gtk::SingleSelection) {
    let popover = gtk::PopoverMenu::from_model(None::<&gtk::gio::Menu>);
    popover.set_parent(list);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);

    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    gesture.connect_pressed({
        let list = list.clone();
        let selection = selection.clone();
        let popover = popover.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let Some(item) = row_at(&list, &selection, y) else {
                return;
            };
            if item.is_group() {
                return;
            }
            popover.set_menu_model(Some(&project_menu(&item.id())));
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        }
    });
    list.add_controller(gesture);
}

/// Which row is under the pointer. `ListView` does not expose a hit test, so
/// the row is found by walking the children it has realised.
fn row_at(
    list: &gtk::ListView,
    selection: &gtk::SingleSelection,
    y: f64,
) -> Option<SidebarItem> {
    let mut child = list.first_child();
    let mut index = 0u32;
    while let Some(widget) = child {
        // `compute_bounds` against the list is the supported replacement for
        // the deprecated `allocation`.
        let bounds = widget.compute_bounds(list);
        let (top, height) = bounds
            .map(|bounds| (bounds.y() as f64, bounds.height() as f64))
            .unwrap_or((0.0, 0.0));
        if height > 0.0 && y >= top && y < top + height {
            selection.set_selected(index);
            return selection
                .item(index)
                .and_downcast::<gtk::TreeListRow>()
                .and_then(|row| row.item())
                .and_downcast::<SidebarItem>();
        }
        child = widget.next_sibling();
        index += 1;
    }
    None
}

fn project_menu(project_id: &str) -> gtk::gio::Menu {
    let menu = gtk::gio::Menu::new();
    let target = project_id.to_variant();
    for (label, action) in [
        ("New session…", "app.project-new-session"),
        ("Project settings…", "app.project-settings"),
    ] {
        let item = gtk::gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some(action), Some(&target));
        menu.append_item(&item);
    }
    let folder = gtk::gio::Menu::new();
    for (label, action) in [
        ("Show in Files", "app.project-show-files"),
        ("Copy path", "app.project-copy-path"),
        ("Reconnect folder…", "app.project-reconnect"),
    ] {
        let item = gtk::gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some(action), Some(&target));
        folder.append_item(&item);
    }
    menu.append_section(None, &folder);

    let danger = gtk::gio::Menu::new();
    let item = gtk::gio::MenuItem::new(Some("Remove project…"), None);
    item.set_action_and_target_value(Some("app.project-remove"), Some(&target));
    danger.append_item(&item);
    menu.append_section(None, &danger);
    menu
}

/// Groups become real tree nodes: a group's children are its projects.
fn build_sidebar(roots: &gtk::gio::ListStore) -> gtk::SingleSelection {
    let tree = gtk::TreeListModel::new(roots.clone(), false, true, |item| {
        let item = item.downcast_ref::<SidebarItem>()?;
        item.children().map(|children| children.upcast())
    });
    gtk::SingleSelection::builder()
        .model(&tree)
        .autoselect(false)
        .can_unselect(true)
        .build()
}

fn sidebar_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let title = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let badge = gtk::Label::new(None);
        badge.add_css_class("dim-label");
        badge.add_css_class("caption");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&title);
        row.append(&badge);

        let expander = gtk::TreeExpander::new();
        expander.set_child(Some(&row));
        item.set_child(Some(&expander));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
            return;
        };
        let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        expander.set_list_row(Some(&row));
        let Some(value) = row.item().and_downcast::<SidebarItem>() else {
            return;
        };
        let Some(content) = expander.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(title) = content.first_child().and_downcast::<gtk::Label>() else {
            return;
        };
        let Some(badge) = title.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        title.set_label(&value.title());
        title.set_tooltip_text(Some(&value.subtitle()));
        if value.is_group() {
            title.add_css_class("heading");
        } else {
            title.remove_css_class("heading");
        }
        badge.set_label(&match value.running() {
            0 => String::new(),
            count => format!("{count} ●"),
        });
    });
    factory
}

fn wire(
    app: &Rc<App>,
    selection: &gtk::SingleSelection,
    search: &gtk::SearchEntry,
    start: &gtk::Button,
    stop: &gtk::Button,
    new_session: &gtk::Button,
) {
    selection.connect_selected_item_notify({
        let app = app.clone();
        move |selection| {
            let chosen = selection
                .selected_item()
                .and_downcast::<gtk::TreeListRow>()
                .and_then(|row| row.item())
                .and_downcast::<SidebarItem>();
            let Some(chosen) = chosen else { return };
            if chosen.is_group() {
                return;
            }
            let id = chosen.id();
            if app.selection.borrow().project.as_deref() == Some(id.as_str()) {
                return;
            }
            app.selection.borrow_mut().project = Some(id);
            app.selection.borrow_mut().session = None;
            rebuild_tabs(&app);
            app.sync();
        }
    });

    search.connect_search_changed({
        let app = app.clone();
        move |entry| {
            *app.search.borrow_mut() = entry.text().to_string();
            app.sync();
        }
    });

    start.connect_clicked({
        let app = app.clone();
        move |_| {
            let Some(id) = app.selection.borrow().session.clone() else {
                return;
            };
            terminal::start(&app, &id);
        }
    });

    stop.connect_clicked({
        let app = app.clone();
        move |_| {
            let Some(id) = app.selection.borrow().session.clone() else {
                return;
            };
            terminal::stop(&app, &id);
        }
    });

    new_session.connect_clicked({
        let app = app.clone();
        move |_| dialogs::new_session(&app)
    });

    app.tabs.connect_selected_page_notify({
        let app = app.clone();
        move |tabs| {
            if app.rebuilding.get() {
                return;
            }
            let Some(page) = tabs.selected_page() else {
                app.selection.borrow_mut().session = None;
                app.refresh_selection();
                return;
            };
            let id = app
                .views
                .borrow()
                .iter()
                .find(|(_, view)| view.page.borrow().as_ref() == Some(&page))
                .map(|(id, _)| id.clone());
            app.selection.borrow_mut().session = id;
            app.refresh_selection();
            crate::actions::refresh_git_status(&app);
        }
    });

    // A running session is never closed by accident: the tab refuses until it
    // has been stopped. During a rebuild the page is only being detached, so
    // the view and its process are left alone.
    app.tabs.connect_close_page({
        let app = app.clone();
        move |tabs, page| {
            let entry = app
                .views
                .borrow()
                .iter()
                .find(|(_, view)| view.page.borrow().as_ref() == Some(page))
                .map(|(id, view)| (id.clone(), view.clone()));
            if app.rebuilding.get() {
                if let Some((_, view)) = entry {
                    view.page.replace(None);
                }
                tabs.close_page_finish(page, true);
                return glib::Propagation::Stop;
            }
            match entry {
                Some((_, view)) if view.running() => {
                    app.error("Stop the session before closing its tab.");
                    tabs.close_page_finish(page, false);
                }
                Some((id, view)) => {
                    view.page.replace(None);
                    app.views.borrow_mut().remove(&id);
                    tabs.close_page_finish(page, true);
                    app.sync();
                }
                None => tabs.close_page_finish(page, true),
            }
            glib::Propagation::Stop
        }
    });

    adw::StyleManager::default().connect_dark_notify({
        let app = app.clone();
        move |_| {
            for view in app.views.borrow().values() {
                terminal::apply_settings(&app, &view.terminal);
            }
        }
    });
}

/// Shows the selected project's sessions as tabs, keeping the terminals of
/// every other project alive and running in the background.
pub fn rebuild_tabs(app: &Rc<App>) {
    app.rebuilding.set(true);
    while app.tabs.n_pages() > 0 {
        let before = app.tabs.n_pages();
        let page = app.tabs.nth_page(0);
        // Free the terminal before the page goes: a closed page keeps hold of
        // its child for a while, and the terminal is needed again right now.
        if let Ok(scroller) = page.child().downcast::<gtk::ScrolledWindow>() {
            scroller.set_child(gtk::Widget::NONE);
        }
        app.tabs.close_page(&page);
        if app.tabs.n_pages() == before {
            // A refusal here would loop forever; stop instead.
            break;
        }
    }

    // The session shown in the split pane is not also a tab: one terminal
    // widget cannot have two parents.
    let split = app.split.borrow().clone();
    let sessions: Vec<_> = app
        .visible_sessions()
        .into_iter()
        .filter(|session| Some(&session.id) != split.as_ref())
        .collect();
    for session in &sessions {
        let view = terminal::ensure_view(app, session);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view.terminal)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let page = app.tabs.append(&scroller);
        page.set_title(&session.title);
        view.page.replace(Some(page));
    }
    app.rebuilding.set(false);

    if app.tabs.n_pages() > 0 {
        let first = app.tabs.nth_page(0);
        app.tabs.set_selected_page(&first);
        app.selection.borrow_mut().session = sessions.first().map(|session| session.id.clone());
    } else {
        app.selection.borrow_mut().session = None;
    }
}

// ---------------------------------------------------------------------------
// Worktrees and the split view.
// ---------------------------------------------------------------------------

/// Creates a worktree off the main loop and binds it to its session. Git work
/// blocks, and the UI must not.
pub fn create_worktree(app: &Rc<App>, id: &str, branch: &str) {
    let plan = {
        let workspace = app.workspace.borrow();
        let busy = |id: &str| {
            app.busy.borrow().contains(id)
                || app.views.borrow().get(id).is_some_and(|view| view.running())
        };
        convoy_core::worktree::plan_create(&workspace, id, branch, &busy)
    };
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => {
            app.error(error);
            return;
        }
    };

    app.busy.borrow_mut().insert(id.to_string());
    app.refresh_selection();

    let root = app.storage.worktrees();
    let project_path = plan.project_path.clone();
    let branch = plan.branch.clone();
    background(
        app,
        move || convoy_core::Git::default().create_worktree(&project_path, &root, &branch),
        {
            let plan = plan.clone();
            move |app, directory: std::path::PathBuf| {
                app.busy.borrow_mut().remove(&plan.session_id);
                let recorded = convoy_core::worktree::record_create(
                    &mut app.workspace.borrow_mut(),
                    &plan.session_id,
                    &directory,
                    &plan.branch,
                );
                if let Err(error) = recorded {
                    app.error(error);
                    app.refresh_selection();
                    return;
                }
                app.sync();
                if !plan.shared_paths.is_empty() || plan.setup_command.is_some() {
                    confirm_setup(app, &plan, &directory);
                }
            }
        },
    );
}

/// Shared files and a setup command are the project's own configuration, but
/// the command runs with the user's permissions, so it is shown and confirmed
/// before it runs.
fn confirm_setup(
    app: &Rc<App>,
    plan: &convoy_core::worktree::CreatePlan,
    directory: &std::path::Path,
) {
    let dialog = adw::AlertDialog::builder()
        .heading("Run this project's worktree setup?")
        .body(format!(
            "Copies: {}\nCommand: {}\nFolder: {}",
            if plan.shared_paths.is_empty() {
                "none".to_string()
            } else {
                plan.shared_paths.join(", ")
            },
            plan.setup_command.as_deref().unwrap_or("none"),
            directory.display()
        ))
        .build();
    dialog.add_response("skip", "Skip setup");
    dialog.add_response("run", "Run setup");
    dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("skip");

    let repo = plan.project_path.clone();
    let shared = plan.shared_paths.clone();
    let command = plan.setup_command.clone();
    let target = directory.to_path_buf();
    dialog.connect_response(None, {
        let app = app.clone();
        move |_, response| {
            if response != "run" {
                return;
            }
            let (repo, shared, command, target) =
                (repo.clone(), shared.clone(), command.clone(), target.clone());
            background(
                &app,
                move || {
                    convoy_core::worktree::setup(
                        &repo,
                        &target,
                        &shared,
                        command.as_deref(),
                        &convoy_core::StdRunner,
                    )
                },
                |app, ()| {
                    app.toasts
                        .add_toast(adw::Toast::builder().title("Worktree setup finished").build());
                },
            );
        }
    });
    dialog.present(Some(&app.window));
}

pub fn remove_worktree(app: &Rc<App>, plan: convoy_core::worktree::RemovePlan) {
    for id in &plan.linked {
        app.busy.borrow_mut().insert(id.clone());
    }
    app.refresh_selection();
    let linked = plan.linked.clone();
    let directory = plan.directory.clone();
    background(
        app,
        move || {
            convoy_core::worktree::remove_checkout(&convoy_core::Git::default(), &plan)
        },
        move |app, ()| {
            for id in &linked {
                app.busy.borrow_mut().remove(id);
            }
            let outcome = convoy_core::worktree::record_remove(
                &mut app.workspace.borrow_mut(),
                &directory,
            );
            if let Err(error) = outcome {
                app.error(error);
            }
            rebuild_tabs(app);
            app.sync();
        },
    );
}

/// Shows a second session beside the current one.
pub fn open_split(app: &Rc<App>, id: &str) {
    let Some(session) = app
        .workspace
        .borrow()
        .state()
        .sessions
        .iter()
        .find(|session| session.id == id)
        .cloned()
    else {
        return;
    };
    close_split(app);
    // Claim the session first, then rebuild: that takes its terminal out of
    // the tab strip so it can be parented here instead.
    *app.split.borrow_mut() = Some(id.to_string());
    rebuild_tabs(app);

    let view = terminal::ensure_view(app, &session);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view.terminal)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let label = gtk::Label::builder().label(&session.title).xalign(0.0).build();
    label.add_css_class("caption-heading");
    let bar = gtk::ActionBar::new();
    bar.pack_start(&label);
    let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pane.append(&bar);
    pane.append(&scroller);

    app.panes.set_end_child(Some(&pane));
    app.panes.set_resize_end_child(true);
    app.panes.set_shrink_end_child(false);
    app.refresh_selection();
}

pub fn close_split(app: &Rc<App>) {
    if let Some(pane) = app.panes.end_child() {
        if let Ok(pane) = pane.downcast::<gtk::Box>() {
            if let Some(scroller) = pane.last_child().and_downcast::<gtk::ScrolledWindow>() {
                scroller.set_child(gtk::Widget::NONE);
            }
        }
    }
    app.panes.set_end_child(gtk::Widget::NONE);
    *app.split.borrow_mut() = None;
    rebuild_tabs(app);
    app.refresh_selection();
}


/// Moves to the next or previous session tab, wrapping round.
pub fn step_session(app: &Rc<App>, delta: i32) {
    let pages = app.tabs.n_pages();
    if pages == 0 {
        return;
    }
    let current = app
        .tabs
        .selected_page()
        .map(|page| app.tabs.page_position(&page))
        .unwrap_or(0);
    let next = (current + delta).rem_euclid(pages);
    let page = app.tabs.nth_page(next);
    app.tabs.set_selected_page(&page);
}

pub fn focus_search(app: &Rc<App>) {
    app.search_entry.grab_focus();
}
