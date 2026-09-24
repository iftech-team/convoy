//! GObject wrappers for the list models.
//!
//! `ListView` binds to properties, so the rows a user sees are updated in
//! place rather than rebuilt. This is what replaces the Electron renderer's
//! habit of rebuilding the whole DOM on every change: a rebuild would lose
//! focus, selection and scroll position, which in a list of live sessions is
//! very noticeable.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp_sidebar {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::SidebarItem)]
    pub struct SidebarItem {
        #[property(get, set)]
        pub id: RefCell<String>,
        #[property(get, set)]
        pub title: RefCell<String>,
        #[property(get, set)]
        pub subtitle: RefCell<String>,
        /// A group heading rather than a project: it has children and cannot
        /// be opened itself.
        #[property(name = "is-group", get, set)]
        pub group: Cell<bool>,
        #[property(get, set)]
        pub running: Cell<u32>,
        pub children: RefCell<Option<gtk::gio::ListStore>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarItem {
        const NAME: &'static str = "ConvoySidebarItem";
        type Type = super::SidebarItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SidebarItem {}
}

mod imp_session {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::SessionObject)]
    pub struct SessionObject {
        #[property(get, set)]
        pub id: RefCell<String>,
        #[property(get, set)]
        pub title: RefCell<String>,
        #[property(get, set)]
        pub subtitle: RefCell<String>,
        #[property(get, set)]
        pub agent: RefCell<String>,
        #[property(get, set)]
        pub running: Cell<bool>,
        #[property(get, set)]
        pub pinned: Cell<bool>,
        #[property(get, set)]
        pub archived: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionObject {
        const NAME: &'static str = "ConvoySessionObject";
        type Type = super::SessionObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionObject {}
}

glib::wrapper! {
    pub struct SidebarItem(ObjectSubclass<imp_sidebar::SidebarItem>);
}

impl SidebarItem {
    pub fn project(id: &str, title: &str, subtitle: &str) -> Self {
        glib::Object::builder()
            .property("id", id)
            .property("title", title)
            .property("subtitle", subtitle)
            .property("is-group", false)
            .build()
    }

    pub fn heading(name: &str) -> Self {
        let item: Self = glib::Object::builder()
            .property("id", format!("group:{name}"))
            .property("title", name)
            .property("subtitle", "")
            .property("is-group", true)
            .build();
        item.imp()
            .children
            .replace(Some(gtk::gio::ListStore::new::<SidebarItem>()));
        item
    }

    pub fn children(&self) -> Option<gtk::gio::ListStore> {
        self.imp().children.borrow().clone()
    }

    pub fn push(&self, child: &SidebarItem) {
        if let Some(children) = self.children() {
            children.append(child);
        }
    }
}

glib::wrapper! {
    pub struct SessionObject(ObjectSubclass<imp_session::SessionObject>);
}

impl SessionObject {
    pub fn new(id: &str) -> Self {
        glib::Object::builder().property("id", id).build()
    }
}

mod imp_change {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ChangeObject)]
    pub struct ChangeObject {
        #[property(get, set)]
        pub path: RefCell<String>,
        #[property(get, set)]
        pub original: RefCell<String>,
        /// The two porcelain columns, shown as-is.
        #[property(get, set)]
        pub status: RefCell<String>,
        #[property(get, set)]
        pub untracked: Cell<bool>,
        #[property(get, set)]
        pub staged: Cell<bool>,
        #[property(get, set)]
        pub conflict: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChangeObject {
        const NAME: &'static str = "ConvoyChangeObject";
        type Type = super::ChangeObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ChangeObject {}
}

mod imp_commit {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::CommitObject)]
    pub struct CommitObject {
        #[property(get, set)]
        pub id: RefCell<String>,
        #[property(get, set)]
        pub short: RefCell<String>,
        #[property(get, set)]
        pub subject: RefCell<String>,
        #[property(get, set)]
        pub author: RefCell<String>,
        #[property(get, set)]
        pub date: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CommitObject {
        const NAME: &'static str = "ConvoyCommitObject";
        type Type = super::CommitObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for CommitObject {}
}

glib::wrapper! {
    pub struct ChangeObject(ObjectSubclass<imp_change::ChangeObject>);
}

impl ChangeObject {
    pub fn from(change: &convoy_core::git::status::Change) -> Self {
        glib::Object::builder()
            .property("path", &change.path)
            .property("original", change.original.clone().unwrap_or_default())
            .property("status", format!("{}{}", change.index, change.worktree))
            .property("untracked", change.untracked)
            // A staged change has something other than a space in the index
            // column, and `?` means it is not tracked at all.
            .property("staged", change.index != ' ' && change.index != '?')
            .property("conflict", change.conflict)
            .build()
    }
}

glib::wrapper! {
    pub struct CommitObject(ObjectSubclass<imp_commit::CommitObject>);
}

impl CommitObject {
    pub fn from(commit: &convoy_core::git::Commit) -> Self {
        glib::Object::builder()
            .property("id", &commit.id)
            .property("short", &commit.short)
            .property("subject", &commit.subject)
            .property("author", &commit.author)
            .property("date", &commit.date)
            .build()
    }
}
