//! The main window.
//!
//! Layout follows `docs/plan-refactor/04-ui-spec.md`: a navigation split view
//! with the project tree on the left and the selected project's sessions as
//! tabs on the right. Groups are real expandable nodes rather than the text
//! prefix the Electron sidebar used.

use adw::prelude::*;
use convoy_core::{Storage, Workspace};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::objects::SidebarItem;
use crate::state::{App, Chrome, Selection};
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

    let menu = gtk::gio::Menu::new();
    menu.append(Some("Open folder…"), Some("app.open-folder"));
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
    let content_header = adw::HeaderBar::builder().title_widget(&title).build();
    content_header.pack_end(&new_session);

    let empty = adw::StatusPage::builder()
        .icon_name("utilities-terminal-symbolic")
        .title("No project selected")
        .description("Open a folder to begin.")
        .build();
    let stack = gtk::Stack::new();
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&tabs, Some("terminals"));
    stack.set_vexpand(true);

    let start = gtk::Button::with_label("Start");
    start.add_css_class("suggested-action");
    start.set_sensitive(false);
    let stop = gtk::Button::with_label("Stop");
    stop.add_css_class("destructive-action");
    stop.set_sensitive(false);
    let actions = gtk::ActionBar::new();
    actions.pack_start(&start);
    actions.pack_start(&stop);

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
    });

    wire(&app, &selection, &search, &start, &stop, &new_session);
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

    let sessions = app.visible_sessions();
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
