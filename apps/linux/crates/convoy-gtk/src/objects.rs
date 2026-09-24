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
