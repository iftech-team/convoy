//! Objective-C exceptions raised while AppKit handles an event.
//!
//! AppKit and WebKit sometimes raise an `NSException` from inside
//! `-[NSApplication sendEvent:]`. A Cocoa app logs it and carries on. Here
//! the exception unwinds into tao's `sendEvent:` override, a Rust frame that
//! cannot unwind, and the process aborts: "Convoy quit unexpectedly", with
//! nothing in the report but `abort()` under `_handleEvent:`.
//!
//! So `NSApplication`'s own `sendEvent:` — the one tao calls as `super` — is
//! wrapped in an `@try`. An exception is written to `exceptions.log` in the
//! app's log folder and the event is dropped, as AppKit would.

use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

type SendEvent = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

static ORIGINAL: OnceLock<SendEvent> = OnceLock::new();
static LOG: OnceLock<PathBuf> = OnceLock::new();

unsafe extern "C-unwind" fn send_event(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let Some(original) = ORIGINAL.get().copied() else {
        return;
    };
    // SAFETY: the arguments are AppKit's own, passed on unchanged to the
    // implementation they were meant for.
    // Nothing of ours is left half-changed if it throws: AppKit's state is
    // AppKit's, as in a Cocoa app that logs the exception and carries on.
    let caught = objc2::exception::catch(std::panic::AssertUnwindSafe(move || unsafe {
        original(this, cmd, event)
    }));
    if let Err(exception) = caught {
        record(&match exception {
            Some(exception) => format!("{exception:?}"),
            None => "a nil exception".to_string(),
        });
    }
}

fn record(what: &str) {
    eprintln!("Convoy: caught an Objective-C exception in sendEvent: {what}");
    let Some(path) = LOG.get() else { return };
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            file,
            "{} sendEvent: {what}",
            convoy_core::time::now_iso8601()
        );
    }
}

/// Wraps `-[NSApplication sendEvent:]`. Call once, on the main thread,
/// before events are handled.
pub fn install(log_dir: PathBuf) {
    let _ = LOG.set(log_dir.join("exceptions.log"));
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return;
    };
    let Some(method) = class.instance_method(sel!(sendEvent:)) else {
        return;
    };
    // SAFETY: `sendEvent:` takes one object and returns nothing, which is
    // the signature both sides of the transmute have.
    let original: SendEvent =
        unsafe { std::mem::transmute::<Imp, SendEvent>(method.implementation()) };
    if ORIGINAL.set(original).is_err() {
        return;
    }
    // SAFETY: as above; the replacement has the same signature.
    unsafe {
        method.set_implementation(std::mem::transmute::<SendEvent, Imp>(send_event));
    }
}
