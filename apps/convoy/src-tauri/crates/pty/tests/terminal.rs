//! The terminal path without a window, on whichever platform this runs.
//!
//! A pty on Unix and ConPTY on Windows are two implementations of the same
//! promise: a command runs, its output comes back, input reaches it, and the
//! exit code survives. Both are checked here, with the shell each platform
//! has, because that promise is what the rest of Convoy is built on.

use convoy_pty::{Ended, Launch, Sink, Terminals};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A sink that hands everything to the test through a channel, and answers
/// the one question a terminal is obliged to answer.
///
/// ConPTY asks for the cursor position on startup — `ESC[6n` — and waits for
/// the reply before letting the child write anything. In the app xterm.js
/// answers it without being asked to; a test with no terminal on the other end
/// must do it itself, or the child simply never speaks.
struct Recorder {
    data: Mutex<Sender<String>>,
    ended: Mutex<Sender<Ended>>,
    terminals: Arc<Terminals>,
}

impl Recorder {
    fn new(terminals: &Arc<Terminals>) -> (Arc<Self>, Receiver<String>, Receiver<Ended>) {
        let (data, output) = channel();
        let (ended, endings) = channel();
        (
            Arc::new(Recorder {
                data: Mutex::new(data),
                ended: Mutex::new(ended),
                terminals: Arc::clone(terminals),
            }),
            output,
            endings,
        )
    }
}

impl Sink for Recorder {
    fn data(&self, id: &str, data: &str) {
        if data.contains("\u{1b}[6n") {
            let _ = self.terminals.write(id, "\u{1b}[1;1R");
        }
        if let Ok(sender) = self.data.lock() {
            let _ = sender.send(data.to_string());
        }
    }

    fn ended(&self, _id: &str, how: Ended) {
        if let Ok(sender) = self.ended.lock() {
            let _ = sender.send(how);
        }
    }
}

/// The environment the child gets, which is what Convoy passes an agent: this
/// process's own, with the terminal named. Windows will not start a shell in a
/// nearly empty environment — PowerShell needs a dozen variables it is never
/// asked about — and naming them one at a time is a list that goes stale.
fn environment() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars().collect();
    env.retain(|(key, _)| !key.eq_ignore_ascii_case("term"));
    env.push(("TERM".to_string(), "xterm-256color".to_string()));
    env
}

fn root() -> &'static std::path::Path {
    std::path::Path::new(if cfg!(windows) { "C:\\" } else { "/" })
}

/// Says READY, reads a line, echoes it back with a marker and exits 7. The
/// marker matters: PowerShell takes a second or two to reach `Read-Host`, and
/// a test that guesses how long would be flaky on a loaded runner.
#[cfg(unix)]
fn read_and_exit() -> (&'static str, Vec<String>) {
    let script = "printf 'READY\\n'; read line; printf 'GOT:%s\\n' \"$line\"; exit 7";
    ("/bin/sh", vec!["-c".into(), script.into()])
}

#[cfg(windows)]
fn read_and_exit() -> (&'static str, Vec<String>) {
    let script = "Write-Output READY; $line = Read-Host; Write-Output \"GOT:$line\"; exit 7";
    (
        "powershell.exe",
        vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-Command".into(),
            script.into(),
        ],
    )
}

/// A child that says READY and then does nothing until it is stopped.
fn sleeps() -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            "powershell.exe",
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-Command".into(),
                "Write-Output READY; Start-Sleep -Seconds 300".into(),
            ],
        )
    } else {
        (
            "/bin/sh",
            vec!["-c".into(), "printf 'READY\\n'; sleep 300".into()],
        )
    }
}

fn gather(output: &Receiver<String>, needle: &str, within: Duration) -> String {
    let deadline = Instant::now() + within;
    let mut seen = String::new();
    while Instant::now() < deadline && !seen.contains(needle) {
        if let Ok(chunk) = output.recv_timeout(Duration::from_millis(200)) {
            seen.push_str(&chunk);
        }
    }
    seen
}

fn launch(terminals: &Arc<Terminals>, sink: Arc<Recorder>, id: &str, what: (&str, Vec<String>)) {
    let (program, args) = what;
    terminals
        .start(
            sink,
            Launch {
                id,
                program,
                args: &args,
                cwd: root(),
                env: &environment(),
                cols: 80,
                rows: 24,
            },
        )
        .expect("spawn");
}

#[test]
fn a_command_runs_and_its_output_and_exit_code_come_back() {
    let terminals = Arc::new(Terminals::new());
    let (sink, output, endings) = Recorder::new(&terminals);
    launch(&terminals, sink, "probe", read_and_exit());

    assert!(terminals.running("probe"));
    let ready = gather(&output, "READY", Duration::from_secs(25));
    assert!(
        ready.contains("READY"),
        "the child never started: {ready:?}"
    );
    terminals.write("probe", "hello\r\n").expect("write");

    let seen = gather(&output, "GOT:hello", Duration::from_secs(25));
    assert!(seen.contains("GOT:hello"), "output was {seen:?}");

    let how = endings
        .recv_timeout(Duration::from_secs(25))
        .expect("no ending");
    assert_eq!(how, Ended::Exited(7), "the exit code decides a task's fate");
    assert!(!terminals.running("probe"), "the session was not cleared");
}

#[test]
fn writing_to_a_session_that_is_not_running_says_so() {
    let terminals = Arc::new(Terminals::new());
    assert_eq!(
        terminals.write("nobody", "hello"),
        Err("Start or resume the target session first.".into())
    );
}

/// Stopping is asked for, so it must not be reported as the agent failing, and
/// the session must be cleared either way.
#[test]
fn stopping_ends_the_session_and_says_it_was_asked_for() {
    let terminals = Arc::new(Terminals::new());
    let (sink, output, endings) = Recorder::new(&terminals);
    launch(&terminals, sink, "idle", sleeps());

    assert!(terminals.running("idle"));
    gather(&output, "READY", Duration::from_secs(25));
    terminals.stop("idle");

    let how = endings
        .recv_timeout(Duration::from_secs(25))
        .expect("no ending after stop");
    assert_eq!(how, Ended::Stopped);
    assert!(!terminals.running("idle"), "the session was not cleared");
}

/// Hibernation stops the process the same way but is not a failure: the task
/// behind it goes to review rather than being marked failed.
#[test]
fn hibernation_is_reported_apart_from_a_stop() {
    let terminals = Arc::new(Terminals::new());
    let (sink, output, endings) = Recorder::new(&terminals);
    launch(&terminals, sink, "dozing", sleeps());

    gather(&output, "READY", Duration::from_secs(25));
    terminals.hibernate("dozing");

    assert_eq!(
        endings
            .recv_timeout(Duration::from_secs(25))
            .expect("no ending after hibernating"),
        Ended::Hibernated
    );
}

/// Stopping signals the process group, so an agent's own children go with it
/// rather than surviving as orphans. Windows has no process groups —
/// `taskkill /T` walks the tree there, and there is no `/proc` to read back.
#[cfg(unix)]
#[test]
fn stopping_takes_the_whole_process_tree() {
    let terminals = Arc::new(Terminals::new());
    let (sink, output, _endings) = Recorder::new(&terminals);
    let script = "sleep 300 & printf 'CHILD:%s\\n' $!; sleep 300";
    launch(
        &terminals,
        sink,
        "tree",
        ("/bin/sh", vec!["-c".into(), script.into()]),
    );

    let seen = gather(&output, "CHILD:", Duration::from_secs(5));
    let child: i32 = seen
        .split("CHILD:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_else(|| panic!("no child pid in {seen:?}"));

    terminals.stop("tree");
    std::thread::sleep(Duration::from_millis(600));
    assert!(
        !std::path::Path::new(&format!("/proc/{child}")).exists(),
        "grandchild {child} survived the stop"
    );
}
