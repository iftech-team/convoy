//! Automated half of the M0 terminal checklist.
//!
//! Everything a machine can decide is decided here: whether output survives a
//! round trip through the buffer, whether input reaches the child by all three
//! routes the port needs, whether the alternate screen stays out of the
//! scrollback, whether exit codes arrive, and whether signalling the process
//! group stops the whole tree. What is left for a human is how a real agent
//! *looks*: emoji and CJK column width, mouse reporting, redraw under load.
//!
//! ```sh
//! xvfb-run -a cargo run --bin convoy-vte-selftest
//! ```
//!
//! Exits non-zero if any check fails.

use gtk::prelude::*;
use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::Duration;
use vte4::prelude::*;
use vte4::{Format, Pty, PtyFlags, Terminal};

const SHELL: &str = "/bin/bash";

struct Outcome {
    name: &'static str,
    passed: bool,
    detail: String,
}

fn main() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id("com.iftech.convoy.vteselftest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let failures = Rc::new(RefCell::new(0usize));

    application.connect_activate({
        let failures = failures.clone();
        move |application| {
            let terminal = Terminal::new();
            terminal.set_scrollback_lines(10_000);
            terminal.set_size(80, 24);

            let window = gtk::ApplicationWindow::builder()
                .application(application)
                .default_width(800)
                .default_height(480)
                .child(&terminal)
                .build();
            window.present();

            glib::spawn_future_local({
                let application = application.clone();
                let failures = failures.clone();
                async move {
                    let outcomes = run_all(&terminal).await;
                    println!("\n── VTE selftest ──");
                    for outcome in &outcomes {
                        println!(
                            "{} {:<18} {}",
                            if outcome.passed { "PASS" } else { "FAIL" },
                            outcome.name,
                            outcome.detail
                        );
                    }
                    let failed = outcomes.iter().filter(|outcome| !outcome.passed).count();
                    println!(
                        "── {} of {} checks passed ──",
                        outcomes.len() - failed,
                        outcomes.len()
                    );
                    *failures.borrow_mut() = failed;
                    application.quit();
                }
            });
        }
    });

    let code = application.run_with_args(&[std::env::args().next().unwrap_or_default()]);
    if *failures.borrow() > 0 {
        return glib::ExitCode::FAILURE;
    }
    code
}

async fn run_all(terminal: &Terminal) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    outcomes.push(check_output(terminal).await);
    outcomes.push(check_feed_child(terminal).await);
    outcomes.push(check_direct_write(terminal).await);
    outcomes.push(check_bracketed_paste(terminal).await);
    outcomes.push(check_alt_screen(terminal).await);
    outcomes.push(check_exit_code(terminal, "exit 7", "exit code (instant)").await);
    outcomes.push(check_exit_code(terminal, "sleep 0.3; exit 7", "exit code (delayed)").await);
    outcomes.push(check_killpg(terminal).await);
    outcomes.push(check_resize(terminal).await);
    outcomes.push(check_character_width(terminal).await);
    outcomes.push(check_agent_launch(terminal, convoy_core::model::Agent::Claude).await);
    outcomes.push(check_agent_launch(terminal, convoy_core::model::Agent::Codex).await);
    outcomes
}

/// A running child and the channel its exit status will arrive on.
struct Child {
    pid: i32,
    exited: async_channel::Receiver<i32>,
    handler: glib::SignalHandlerId,
}

async fn spawn(terminal: &Terminal, script: &str) -> Result<Child, String> {
    let environment = convoy_core::provider::launch::agent_environment(
        &convoy_core::provider::launch::current_environment(),
    );
    spawn_argv(terminal, &[SHELL, "-c", script], &environment).await
}

async fn spawn_argv(
    terminal: &Terminal,
    argv: &[&str],
    environment: &std::collections::BTreeMap<String, String>,
) -> Result<Child, String> {
    terminal.reset(true, true);
    let pty = Pty::new_sync(PtyFlags::DEFAULT, gio::Cancellable::NONE)
        .map_err(|error| error.to_string())?;
    terminal.set_pty(Some(&pty));

    let (exit_sender, exited) = async_channel::bounded(1);
    let handler = terminal.connect_child_exited({
        let exit_sender = exit_sender.clone();
        move |_, status| {
            let _ = exit_sender.try_send(status);
        }
    });

    let envv: Vec<String> = environment
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    let envv: Vec<&str> = envv.iter().map(String::as_str).collect();

    let (spawned, wait) = async_channel::bounded(1);
    pty.spawn_async(
        None,
        argv,
        &envv,
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        gio::Cancellable::NONE,
        {
            let terminal = terminal.clone();
            move |result| {
                // Watch from inside the callback, not after awaiting it: a
                // command that exits at once is already gone by the time the
                // result reaches the awaiting task.
                let outcome = match result {
                    Ok(pid) => {
                        // VTE tears the pty down as soon as the child is gone,
                        // and `watch_child` asserts on a terminal without one.
                        let watched = terminal.pty().is_some();
                        if watched {
                            terminal.watch_child(pid);
                        }
                        Ok((pid.0, watched))
                    }
                    Err(error) => Err(error.to_string()),
                };
                let _ = spawned.send_blocking(outcome);
            }
        },
    );
    let (pid, watched) = wait
        .recv()
        .await
        .map_err(|_| "spawn callback never ran".to_string())??;

    if !watched {
        // Exactly one child watch may exist per pid, so this is registered
        // only when VTE did not take it. The spawn used
        // G_SPAWN_DO_NOT_REAP_CHILD, so the status is still collectable.
        glib::child_watch_add_local(glib::Pid(pid), move |_, status| {
            let _ = exit_sender.try_send(status);
        });
    }

    Ok(Child {
        pid,
        exited,
        handler,
    })
}

/// Stops the child and waits for it. VTE watches one child at a time, so a
/// scenario must leave none behind before the next one spawns.
async fn finish(terminal: &Terminal, child: Child) {
    if !child.exited.is_closed() && child.exited.is_empty() {
        unsafe {
            libc::killpg(child.pid, libc::SIGKILL);
        }
    }
    let _ = child.exited.recv().await;
    terminal.disconnect(child.handler);
    terminal.set_pty(None);
}

/// The whole buffer, scrollback included, as plain text.
///
/// `text_format` returns everything VTE holds; `text_range_format` is kept as
/// a fallback and as a cross-check, because a row range is what the port will
/// use when it wants only the tail of a long session.
fn dump(terminal: &Terminal) -> String {
    let whole = terminal
        .text_format(Format::Text)
        .map(|value| value.to_string())
        .unwrap_or_default();
    if !whole.trim().is_empty() {
        return whole;
    }
    let last = terminal
        .vadjustment()
        .map(|adjustment| adjustment.upper() as i64)
        .unwrap_or_else(|| terminal.row_count());
    terminal
        .text_range_format(Format::Text, 0, 0, last, -1)
        .0
        .map(|value| value.to_string())
        .unwrap_or_default()
}

/// Reports how the two extraction routes compare, so a divergence shows up
/// here rather than as a mysteriously short review brief later.
fn dump_report(terminal: &Terminal) -> String {
    let whole = terminal
        .text_format(Format::Text)
        .map(|value| value.to_string())
        .unwrap_or_default();
    let last = terminal
        .vadjustment()
        .map(|adjustment| adjustment.upper() as i64)
        .unwrap_or_else(|| terminal.row_count());
    let ranged = terminal
        .text_range_format(Format::Text, 0, 0, last, -1)
        .0
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!(
        "text_format {} chars, text_range_format 0..{last} {} chars",
        whole.trim().chars().count(),
        ranged.trim().chars().count()
    )
}

async fn settle(milliseconds: u64) {
    glib::timeout_future(Duration::from_millis(milliseconds)).await;
}

fn contains_all(text: &str, needles: &[&str]) -> Result<(), String> {
    let missing: Vec<&str> = needles
        .iter()
        .copied()
        .filter(|needle| !text.contains(needle))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("missing {missing:?}"))
    }
}

fn outcome(name: &'static str, result: Result<String, String>) -> Outcome {
    match result {
        Ok(detail) => Outcome {
            name,
            passed: true,
            detail,
        },
        Err(detail) => Outcome {
            name,
            passed: false,
            detail,
        },
    }
}

/// Colours, truecolour, emoji and CJK reach the buffer, and reading it back
/// gives plain text with the escape sequences already applied and gone.
async fn check_output(terminal: &Terminal) -> Outcome {
    let script = r#"printf 'PROBE-START\n'; printf '\033[31mRED\033[0m \033[38;2;0;255;0mTRUE\033[0m \xF0\x9F\x99\x82 \xE6\x97\xA5\xE6\x9C\xAC\n'; sleep 30"#;
    let result = async {
        let child = spawn(terminal, script).await?;
        settle(600).await;
        let text = dump(terminal);
        finish(terminal, child).await;
        contains_all(&text, &["PROBE-START", "RED", "TRUE", "🙂", "日本"])?;
        if text.contains('\u{1b}') {
            return Err("escape sequences leaked into the text".into());
        }
        Ok(format!("{} chars, colours applied, emoji and CJK intact", text.trim().chars().count()))
    }
    .await;
    Outcome {
        name: "output",
        passed: result.is_ok(),
        detail: result.unwrap_or_else(|error| error),
    }
}

/// `feed_child` is how quick commands and review feedback reach an agent.
async fn check_feed_child(terminal: &Terminal) -> Outcome {
    let result = async {
        let child = spawn(terminal, "read line; printf 'GOT:%s\\n' \"$line\"; sleep 30").await?;
        settle(250).await;
        terminal.feed_child(b"hello-from-feed\n");
        settle(400).await;
        let text = dump(terminal);
        finish(terminal, child).await;
        contains_all(&text, &["GOT:hello-from-feed"])?;
        Ok("input reached the child".to_string())
    }
    .await;
    outcome("feed_child", result)
}

/// Writing to the master fd directly, the fallback the plan considered.
async fn check_direct_write(terminal: &Terminal) -> Outcome {
    let result = async {
        let child = spawn(terminal, "read line; printf 'RAW:%s\\n' \"$line\"; sleep 30").await?;
        settle(250).await;
        let Some(pty) = terminal.pty() else {
            return Err("no pty".to_string());
        };
        let message = b"hello-from-fd\n";
        let written = unsafe {
            libc::write(
                pty.fd().as_raw_fd(),
                message.as_ptr() as *const libc::c_void,
                message.len(),
            )
        };
        settle(400).await;
        let text = dump(terminal);
        finish(terminal, child).await;
        contains_all(&text, &["RAW:hello-from-fd"])?;
        Ok(format!("{written} bytes written to the master fd"))
    }
    .await;
    outcome("pty fd write", result)
}

/// The port pastes with bracketed paste and without a trailing Enter, so the
/// agent receives the text but never sends it on the user's behalf. The child
/// writes the raw bytes to a file, because comparing what the terminal *shows*
/// would prove nothing: the markers are an input sequence, and echoing them
/// back is not how an agent reads them.
async fn check_bracketed_paste(terminal: &Terminal) -> Outcome {
    let result = async {
        let directory = tempfile::Builder::new()
            .prefix("convoy-paste-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let file = directory.path().join("received");

        let payload = convoy_core::history::paste("pasted-line-one\npasted-line-two", false)
            .map_err(|error| error.to_string())?;
        let script = format!(
            "stty raw -echo; head -c {} > {}; sleep 30",
            payload.len(),
            convoy_core::provider::launch::quote(&file.to_string_lossy())
        );

        let child = spawn(terminal, &script).await?;
        settle(400).await;
        terminal.feed_child(payload.as_bytes());
        settle(600).await;
        finish(terminal, child).await;

        let received = std::fs::read(&file).map_err(|error| error.to_string())?;
        if received != payload.as_bytes() {
            return Err(format!(
                "child received {:?}, expected {:?}",
                String::from_utf8_lossy(&received),
                payload
            ));
        }
        if received.ends_with(b"\r") {
            return Err("an Enter was appended without being asked for".into());
        }

        // With submit, and only then, a carriage return follows the close marker.
        let submitted = convoy_core::history::paste("run it", true)
            .map_err(|error| error.to_string())?;
        if !submitted.ends_with("\u{1b}[201~\r") {
            return Err("submit did not append exactly one Enter".into());
        }
        Ok(format!("{} bytes delivered verbatim, no Enter", received.len()))
    }
    .await;
    outcome("bracketed paste", result)
}

/// A full-screen TUI must not leave its frames in the scrollback: that is what
/// makes reading the buffer back a usable source for a review brief.
async fn check_alt_screen(terminal: &Terminal) -> Outcome {
    let script = r#"printf 'MAIN-BEFORE\n'; printf '\033[?1049h'; printf 'INSIDE-ALT\n'; sleep 0.4; printf '\033[?1049l'; printf 'MAIN-AFTER\n'; sleep 30"#;
    let result = async {
        let child = spawn(terminal, script).await?;
        settle(1000).await;
        let text = dump(terminal);
        finish(terminal, child).await;
        contains_all(&text, &["MAIN-BEFORE", "MAIN-AFTER"])?;
        if text.contains("INSIDE-ALT") {
            return Err("alternate screen content leaked into the scrollback".into());
        }
        Ok("alternate buffer entered and left cleanly".to_string())
    }
    .await;
    outcome("alt screen", result)
}

/// Exit codes decide whether a task moves to review or to failed. An agent
/// that fails at once — a missing CLI exits 127 — must report its code just as
/// reliably as one that runs for an hour.
async fn check_exit_code(terminal: &Terminal, script: &str, name: &'static str) -> Outcome {
    let result = async {
        let child = spawn(terminal, script).await?;
        let status = child
            .exited
            .recv()
            .await
            .map_err(|_| "no child-exited signal".to_string())?;
        terminal.disconnect(child.handler);
        terminal.set_pty(None);
        let code = status >> 8;
        if code != 7 {
            return Err(format!("raw status {status}, decoded {code}, expected 7"));
        }
        Ok(format!("raw status {status} decodes to exit {code}"))
    }
    .await;
    outcome(name, result)
}

/// Stopping a session must stop the agent *and* everything it started.
async fn check_killpg(terminal: &Terminal) -> Outcome {
    let result = async {
        let child = spawn(terminal, "sleep 300 & printf 'GRANDCHILD:%s\\n' $!; sleep 300").await?;
        settle(500).await;
        let text = dump(terminal);
        let grandchild: i32 = text
            .split("GRANDCHILD:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| format!("no grandchild pid in {text:?}"))?;

        let signalled = unsafe { libc::killpg(child.pid, libc::SIGHUP) };
        if signalled != 0 {
            return Err("killpg failed".to_string());
        }
        let status = child
            .exited
            .recv()
            .await
            .map_err(|_| "no child-exited after killpg".to_string())?;
        terminal.disconnect(child.handler);
        terminal.set_pty(None);

        // The group leader is gone; the grandchild must be too.
        settle(300).await;
        let alive = std::path::Path::new(&format!("/proc/{grandchild}")).exists();
        if alive {
            return Err(format!("grandchild {grandchild} survived the signal"));
        }
        Ok(format!("group stopped, status {status}, grandchild reaped"))
    }
    .await;
    outcome("killpg", result)
}

/// The child must see the widget's real geometry: VTE derives the size from
/// the GTK allocation, so a TUI redraws at the width the user actually sees.
async fn check_resize(terminal: &Terminal) -> Outcome {
    let result = async {
        let child = spawn(terminal, "sleep 0.5; stty size; sleep 30").await?;
        settle(1000).await;
        let text = dump(terminal);
        let expected = format!("{} {}", terminal.row_count(), terminal.column_count());
        finish(terminal, child).await;
        if !text.contains(&expected) {
            return Err(format!("child saw {:?}, widget is {expected}", text.trim()));
        }
        Ok(format!("child and widget agree on {expected}"))
    }
    .await;
    outcome("resize", result)
}


/// Emoji and CJK must occupy two cells, or every box-drawn agent UI is one
/// column out of line for the rest of the session. Asked of the terminal
/// itself: the child prints a character and reads back the cursor column.
async fn check_character_width(terminal: &Terminal) -> Outcome {
    let script = r#"stty raw -echo
report() { printf '%b' "$1"; printf '\033[6n'; read -r -d R pos; printf 'WIDTH %s %s\r\n' "$2" "${pos##*;}"; printf '\r\033[2K'; }
report 'A' ascii
report '\xF0\x9F\x99\x82' emoji
report '\xE6\x97\xA5' cjk
report '\xC3\xA9' accent
stty sane
sleep 30"#;
    let result = async {
        let child = spawn(terminal, script).await?;
        settle(900).await;
        let text = dump(terminal);
        finish(terminal, child).await;

        let column = |name: &str| -> Option<u32> {
            text.split(&format!("WIDTH {name} "))
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        };
        let mut widths = Vec::new();
        for (name, expected) in [("ascii", 2u32), ("emoji", 3), ("cjk", 3), ("accent", 2)] {
            let Some(actual) = column(name) else {
                return Err(format!("no report for {name} in {text:?}"));
            };
            if actual != expected {
                return Err(format!(
                    "{name} left the cursor at column {actual}, expected {expected}"
                ));
            }
            widths.push(format!("{name}={}", actual - 1));
        }
        Ok(widths.join(" "))
    }
    .await;
    outcome("character width", result)
}

/// The real thing: Claude Code launched exactly the way the port will launch
/// it, through `session_spec` and a login shell. Its own configuration
/// directory is a throwaway, so the user's sessions and credentials are not
/// touched and no conversation is created.
async fn check_agent_launch(
    terminal: &Terminal,
    agent: convoy_core::model::Agent,
) -> Outcome {
    let name: &'static str = match agent {
        convoy_core::model::Agent::Claude => "claude launch",
        convoy_core::model::Agent::Codex => "codex launch",
    };
    let result = async {
        if which(agent.as_str()).is_none() {
            return Ok(format!("skipped: {} is not on PATH", agent.as_str()));
        }
        // Not /tmp: Codex refuses to install its helper binaries under a
        // temporary directory and then has nowhere to put its state.
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".cache")
            });
        std::fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
        let home = tempfile::Builder::new()
            .prefix("convoy-selftest-")
            .tempdir_in(&cache)
            .map_err(|error| error.to_string())?;

        let mut session = convoy_core::model::Session::new("project", agent, "Probe");
        session.provider_id = match agent {
            convoy_core::model::Agent::Claude => uuid_like(),
            convoy_core::model::Agent::Codex => String::new(),
        };

        let mut environment = convoy_core::provider::launch::agent_environment(
            &convoy_core::provider::launch::current_environment(),
        );
        environment.insert(
            convoy_core::accounts::home_key(agent).to_string(),
            home.path().to_string_lossy().into_owned(),
        );
        let spec = convoy_core::provider::session_spec(&session, None, &environment);

        let mut argv: Vec<&str> = vec![spec.file.as_str()];
        argv.extend(spec.args.iter().map(String::as_str));
        let child = spawn_argv(terminal, &argv, &environment).await?;
        settle(5000).await;
        let text = dump(terminal);
        let report = dump_report(terminal);
        let visible = text.trim();

        unsafe {
            libc::killpg(child.pid, libc::SIGHUP);
        }
        finish(terminal, child).await;

        if visible.is_empty() {
            return Err(format!("the agent produced no output ({report})"));
        }
        if text.contains('\u{1b}') {
            return Err("escape sequences leaked into the buffer".into());
        }
        let sample: Vec<&str> = visible.lines().take(8).collect();
        println!("     ── {name} ──");
        for line in &sample {
            println!("     │ {line}");
        }
        Ok(format!("{} lines rendered, {report}", visible.lines().count()))
    }
    .await;
    outcome(name, result)
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(program))
            .find(|candidate| candidate.is_file())
    })
}

/// A syntactically valid session id; the probe never reaches a provider.
fn uuid_like() -> String {
    "00000000-0000-4000-8000-000000000000".to_string()
}
