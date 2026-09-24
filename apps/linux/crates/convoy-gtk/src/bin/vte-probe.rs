//! M0 terminal probe.
//!
//! Answers the one question the whole plan rests on: does a real agent CLI
//! behave correctly inside VTE? Run it, then exercise the checklist in
//! `docs/plan-refactor/05-terminal-vte.md`:
//!
//! ```sh
//! cargo run --bin convoy-vte-probe                 # login shell
//! cargo run --bin convoy-vte-probe -- claude       # the real thing
//! cargo run --bin convoy-vte-probe -- codex --no-alt-screen
//! ```
//!
//! Check alt-screen entry and exit, 256-colour and truecolour output, emoji and
//! CJK column width, mouse reporting, bracketed paste, and resize under load.
//! The buttons cover the three operations the port needs from the PTY beyond
//! drawing: reading the buffer back, writing into the child, and killing the
//! whole process group.

use gtk::pango;
use gtk::prelude::*;
use std::cell::Cell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use vte4::prelude::*;
use vte4::{Format, Terminal};

const SCROLLBACK: i64 = 10_000;

fn main() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id("com.iftech.convoy.vteprobe")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(build);
    // The probe takes a command of its own, so GTK must not try to parse it.
    // GTK must not parse the command the probe is asked to run.
    application.run_with_args(&[std::env::args().next().unwrap_or_default()])
}

fn command() -> Vec<String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.is_empty() {
        // The same shape the port uses: a POSIX login shell, never $SHELL,
        // which may be fish and cannot run POSIX syntax.
        vec!["/bin/bash".into(), "-il".into()]
    } else {
        let quoted: Vec<String> = arguments
            .iter()
            .map(|argument| convoy_core::provider::launch::quote(argument))
            .collect();
        vec![
            "/bin/bash".into(),
            "-ilc".into(),
            format!("exec {}", quoted.join(" ")),
        ]
    }
}

fn build(application: &adw::Application) {
    let terminal = Terminal::new();
    terminal.set_scrollback_lines(SCROLLBACK as _);
    terminal.set_scroll_on_output(false);
    terminal.set_mouse_autohide(true);
    terminal.set_bold_is_bright(true);
    terminal.set_allow_hyperlink(true);
    terminal.set_font_desc(Some(&pango::FontDescription::from_string("monospace 12")));
    terminal.set_hexpand(true);
    terminal.set_vexpand(true);

    let pid = Rc::new(Cell::new(0i32));

    terminal.connect_child_exited(|_, status| {
        // VTE reports the raw wait status, exactly like waitpid.
        println!(
            "[probe] child-exited: raw status {status}, exit code {}",
            status >> 8
        );
    });

    let scroller = gtk::ScrolledWindow::builder()
        .child(&terminal)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let header = adw::HeaderBar::new();
    let dump = gtk::Button::with_label("Dump text");
    let feed = gtk::Button::with_label("Bracketed paste");
    let raw = gtk::Button::with_label("Write to pty fd");
    let kill = gtk::Button::with_label("killpg SIGHUP");
    for button in [&dump, &feed, &raw, &kill] {
        header.pack_start(button);
    }

    dump.connect_clicked({
        let terminal = terminal.clone();
        move |_| {
            // `text_format` and not `text_range_format`: agents draw their UI
            // with absolute cursor positioning, and a row range returns none
            // of it. See docs/plan-refactor/05-terminal-vte.md.
            let text = terminal
                .text_format(Format::Text)
                .map(|value| value.to_string())
                .unwrap_or_default();
            println!(
                "[probe] text_format: {} chars, last 200: {:?}",
                text.chars().count(),
                convoy_core::json::tail(&text, 200)
            );
        }
    });

    feed.connect_clicked({
        let terminal = terminal.clone();
        move |_| {
            // The port's paste path: bracketed, control characters stripped,
            // and no trailing Enter unless the user asked for one.
            let payload =
                convoy_core::history::paste("probe paste line\nsecond line", false).expect("paste");
            terminal.feed_child(payload.as_bytes());
            println!("[probe] fed {} bytes through feed_child", payload.len());
        }
    });

    raw.connect_clicked({
        let terminal = terminal.clone();
        move |_| {
            let Some(pty) = terminal.pty() else {
                println!("[probe] no pty");
                return;
            };
            let message = b"echo direct-write\n";
            // Borrowed fd: writing through libc avoids taking ownership and
            // closing the master by accident.
            let written = unsafe {
                libc::write(
                    pty.fd().as_raw_fd(),
                    message.as_ptr() as *const libc::c_void,
                    message.len(),
                )
            };
            println!("[probe] wrote {written} bytes directly to pty fd");
        }
    });

    kill.connect_clicked({
        let pid = pid.clone();
        move |_| {
            let child = pid.get();
            if child <= 0 {
                println!("[probe] no child");
                return;
            }
            // The agent spawns its own children; signalling the group is what
            // actually stops the whole tree.
            let result = unsafe { libc::killpg(child, libc::SIGHUP) };
            println!("[probe] killpg({child}, SIGHUP) = {result}");
        }
    });

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&scroller));

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Convoy VTE probe")
        .default_width(1000)
        .default_height(640)
        .content(&view)
        .build();
    window.present();

    let pty = match vte4::Pty::new_sync(vte4::PtyFlags::DEFAULT, gio::Cancellable::NONE) {
        Ok(pty) => pty,
        Err(error) => {
            eprintln!("[probe] pty: {error}");
            return;
        }
    };
    terminal.set_pty(Some(&pty));

    let argv = command();
    let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let environment = convoy_core::provider::launch::agent_environment(
        &convoy_core::provider::launch::current_environment(),
    );
    let envv: Vec<String> = environment
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    let envv_refs: Vec<&str> = envv.iter().map(String::as_str).collect();
    println!("[probe] spawning {argv:?}");

    pty.spawn_async(
        None,
        &argv_refs,
        &envv_refs,
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        gio::Cancellable::NONE,
        {
            let terminal = terminal.clone();
            move |result| match result {
                Ok(child) => {
                    pid.set(child.0);
                    // Without a watch there is no `child-exited` signal and no
                    // exit code. VTE drops the pty the moment the child is
                    // gone, and `watch_child` asserts without one, so a
                    // command that fails instantly needs the GLib watch
                    // instead. Exactly one watch per pid.
                    if terminal.pty().is_some() {
                        terminal.watch_child(child);
                    } else {
                        glib::child_watch_add_local(child, |_, status| {
                            println!(
                                "[probe] glib child-watch: status {status}, exit {}",
                                status >> 8
                            );
                        });
                    }
                    println!("[probe] pid {}", child.0);
                }
                Err(error) => eprintln!("[probe] spawn failed: {error}"),
            }
        },
    );
}
