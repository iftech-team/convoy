//! Keeping the machine awake while agents work, screen included.
//!
//! This prevents idle sleep and the display going dark. Closing the lid or
//! choosing Sleep still sleeps the machine, and the running agents go with it.
//!
//! No dependency for this: macOS ships `caffeinate`, Windows has a system call
//! for it, and every Linux desktop of the last decade ships `systemd-inhibit`.

use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
pub struct KeepAwake {
    held: Mutex<Option<Holder>>,
}

struct Holder {
    #[cfg(unix)]
    child: std::process::Child,
    // Windows ties the request to the thread that made it, so the request owns
    // a thread of its own and the handle is how it is told to let go.
    #[cfg(windows)]
    stop: std::sync::mpsc::Sender<()>,
    #[cfg(windows)]
    thread: std::thread::JoinHandle<()>,
}

/// Holds or lets go, and says whether the machine is being kept awake now, so
/// the window never claims it is when the request could not be made.
#[tauri::command]
pub fn keep_awake(wanted: bool, lock: State<'_, KeepAwake>) -> Result<bool, String> {
    let mut held = lock.held.lock().map_err(|_| "Keep-awake unavailable.")?;
    // A holder whose process has gone no longer holds anything.
    #[cfg(unix)]
    if held
        .as_mut()
        .is_some_and(|holder| !matches!(holder.child.try_wait(), Ok(None)))
    {
        held.take();
    }
    if wanted && held.is_none() {
        *held = acquire();
    } else if !wanted {
        if let Some(holder) = held.take() {
            release(holder);
        }
    }
    Ok(held.is_some())
}

/// `caffeinate -d -i`: the display and the system stay awake. `-w` ties it to
/// Convoy's own process, so a crash cannot leave the Mac unable to sleep.
#[cfg(target_os = "macos")]
fn acquire() -> Option<Holder> {
    std::process::Command::new("/usr/bin/caffeinate")
        .args(["-d", "-i", "-w", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .map(|child| Holder { child })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn acquire() -> Option<Holder> {
    // The inhibition lasts as long as the child does, so a long-lived sleep is
    // held and killed rather than a flag being toggled.
    std::process::Command::new("systemd-inhibit")
        .args([
            "--what=idle:sleep",
            "--who=Convoy",
            "--why=An agent session is running",
            "--mode=block",
            "sleep",
            "86400",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
        .map(|child| Holder { child })
}

#[cfg(unix)]
fn release(mut holder: Holder) {
    let _ = holder.child.kill();
    let _ = holder.child.wait();
}

#[cfg(windows)]
fn acquire() -> Option<Holder> {
    // `SetThreadExecutionState` applies to the calling thread and is undone
    // when that thread ends. A command runs on a pooled thread that may end at
    // any time, so the request lives on a thread that does nothing else.
    let (stop, wait) = std::sync::mpsc::channel();
    let (ready, started) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("keep-awake".into())
        .spawn(move || {
            let held = set_state(true);
            let _ = ready.send(held);
            if !held {
                return;
            }
            let _ = wait.recv();
            set_state(false);
        })
        .ok()?;
    match started.recv() {
        Ok(true) => Some(Holder { stop, thread }),
        _ => None,
    }
}

#[cfg(windows)]
fn release(holder: Holder) {
    let _ = holder.stop.send(());
    let _ = holder.thread.join();
}

#[cfg(windows)]
fn set_state(on: bool) -> bool {
    // ES_CONTINUOUS keeps the request in force until it is cleared.
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }
    let flags = if on {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe { SetThreadExecutionState(flags) != 0 }
}
