//! Colour scheme. The workspace stores the choice so both builds agree on it.

use convoy_core::model::Theme;
use std::rc::Rc;

use crate::state::App;

pub fn apply(app: &Rc<App>) {
    let scheme = match app.workspace.borrow().settings().theme {
        Theme::Dark => adw::ColorScheme::ForceDark,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::System => adw::ColorScheme::Default,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}
