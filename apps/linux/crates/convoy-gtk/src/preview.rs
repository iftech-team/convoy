//! The preview pane of Files & Changes.
//!
//! Four things can be shown, and each gets what suits it: a diff or a source
//! file goes through GtkSourceView, which brings syntax highlighting and line
//! numbers the web renderer had to fake; Markdown is rendered as formatted
//! text; an image is drawn by GTK from its bytes, with no base64 round trip.

use convoy_core::git::diff::SplitRow;
use adw::prelude::*;
use sourceview::prelude::*;

pub struct Preview {
    pub root: gtk::Stack,
    text: sourceview::View,
    left: sourceview::View,
    right: sourceview::View,
    markdown: gtk::Label,
    image: gtk::Picture,
    notice: adw::StatusPage,
}

fn source_view(show_numbers: bool) -> sourceview::View {
    let view = sourceview::View::builder()
        .editable(false)
        .monospace(true)
        .show_line_numbers(show_numbers)
        .highlight_current_line(false)
        .wrap_mode(gtk::WrapMode::None)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    view.set_hexpand(true);
    view.set_vexpand(true);
    view
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(true)
        .build()
}

impl Default for Preview {
    fn default() -> Self {
        Preview::new()
    }
}

impl Preview {
    pub fn new() -> Self {
        let text = source_view(true);
        let left = source_view(false);
        let right = source_view(false);

        // One scrollbar drives both columns, so the two sides never drift.
        let left_pane = scrolled(&left);
        let right_pane = scrolled(&right);
        right_pane.set_vadjustment(Some(&left_pane.vadjustment()));
        let split = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&left_pane)
            .end_child(&right_pane)
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();

        let markdown = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .yalign(0.0)
            .selectable(true)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();

        let image = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::ScaleDown)
            .build();

        let notice = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("Nothing selected")
            .description("Choose a file or a commit.")
            .build();

        let root = gtk::Stack::new();
        root.add_named(&scrolled(&text), Some("text"));
        root.add_named(&split, Some("split"));
        root.add_named(&scrolled(&markdown), Some("markdown"));
        root.add_named(&scrolled(&image), Some("image"));
        root.add_named(&notice, Some("notice"));
        root.set_visible_child_name("notice");

        Preview {
            root,
            text,
            left,
            right,
            markdown,
            image,
            notice,
        }
    }

    pub fn show_notice(&self, title: &str, detail: &str) {
        self.notice.set_title(title);
        self.notice.set_description(Some(detail));
        self.root.set_visible_child_name("notice");
    }

    /// Plain text with highlighting chosen by file name, or by content for a
    /// diff.
    pub fn show_text(&self, text: &str, language: Option<&str>) {
        apply(&self.text, text, language);
        self.root.set_visible_child_name("text");
    }

    /// Side by side, with the line numbers each side actually has. Gaps are
    /// real blank lines, so the two columns stay aligned while scrolling.
    pub fn show_split(&self, rows: &[SplitRow]) {
        let mut left = String::new();
        let mut right = String::new();
        for row in rows {
            if let Some(separator) = &row.separator {
                left.push_str(separator);
                right.push_str(separator);
            } else {
                match &row.left {
                    Some((number, body)) => left.push_str(&format!("{number:>6}  {body}")),
                    None => left.push_str("        "),
                }
                match &row.right {
                    Some((number, body)) => right.push_str(&format!("{number:>6}  {body}")),
                    None => right.push_str("        "),
                }
            }
            left.push('\n');
            right.push('\n');
        }
        apply(&self.left, &left, Some("diff"));
        apply(&self.right, &right, Some("diff"));
        self.root.set_visible_child_name("split");
    }

    pub fn show_markdown(&self, text: &str) {
        self.markdown.set_markup(&markup(text));
        self.root.set_visible_child_name("markdown");
    }

    pub fn show_image(&self, bytes: &[u8]) {
        let bytes = glib::Bytes::from(bytes);
        match gtk::gdk::Texture::from_bytes(&bytes) {
            Ok(texture) => {
                self.image.set_paintable(Some(&texture));
                self.root.set_visible_child_name("image");
            }
            Err(error) => self.show_notice("Image unavailable", &error.to_string()),
        }
    }
}

fn apply(view: &sourceview::View, text: &str, language: Option<&str>) {
    let buffer = view
        .buffer()
        .downcast::<sourceview::Buffer>()
        .expect("a source view has a source buffer");
    buffer.set_text(text);
    let language = language.and_then(|name| sourceview::LanguageManager::default().language(name));
    buffer.set_language(language.as_ref());
    buffer.set_highlight_syntax(true);
    let scheme = sourceview::StyleSchemeManager::default()
        .scheme(if adw::StyleManager::default().is_dark() {
            "Adwaita-dark"
        } else {
            "Adwaita"
        });
    buffer.set_style_scheme(scheme.as_ref());
}

/// The language GtkSourceView should use for a path, by extension.
pub fn language_for(name: &str) -> Option<String> {
    let manager = sourceview::LanguageManager::default();
    manager
        .guess_language(Some(name), None)
        .map(|language| language.id().to_string())
}

/// Basic Markdown, as in the Electron preview: headings, emphasis, code and
/// list structure, nothing more. Pango markup, so the text stays selectable.
fn markup(text: &str) -> String {
    use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

    let mut output = String::new();
    let mut in_code = false;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let size = match level {
                    HeadingLevel::H1 => "xx-large",
                    HeadingLevel::H2 => "x-large",
                    HeadingLevel::H3 => "large",
                    _ => "medium",
                };
                output.push_str(&format!("\n<span size=\"{size}\" weight=\"bold\">"));
            }
            Event::End(TagEnd::Heading(_)) => output.push_str("</span>\n\n"),
            Event::Start(Tag::Emphasis) => output.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => output.push_str("</i>"),
            Event::Start(Tag::Strong) => output.push_str("<b>"),
            Event::End(TagEnd::Strong) => output.push_str("</b>"),
            Event::Start(Tag::CodeBlock(_)) => {
                in_code = true;
                output.push_str("<tt>");
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                output.push_str("</tt>\n");
            }
            Event::Start(Tag::Item) => output.push_str("\n • "),
            Event::End(TagEnd::Paragraph) => output.push_str("\n\n"),
            Event::Code(code) => {
                output.push_str(&format!("<tt>{}</tt>", glib::markup_escape_text(&code)))
            }
            Event::Text(text) => output.push_str(&glib::markup_escape_text(&text)),
            Event::SoftBreak => output.push(if in_code { '\n' } else { ' ' }),
            Event::HardBreak => output.push('\n'),
            Event::Rule => output.push_str("\n───\n"),
            _ => {}
        }
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::markup;

    #[test]
    fn markdown_becomes_escaped_pango_markup() {
        let rendered = markup("# Title\n\nSome **bold** and `a < b`.");
        assert!(rendered.contains("weight=\"bold\""));
        assert!(rendered.contains("<b>bold</b>"));
        assert!(rendered.contains("a &lt; b"), "{rendered}");
    }
}
