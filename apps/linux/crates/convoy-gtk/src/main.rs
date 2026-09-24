//! Convoy for Linux.
//!
//! One process: no renderer, no IPC bridge, no bundled browser. All rules live
//! in `convoy-core`; this crate shows them and passes input back.

use convoy_core::telemetry::hook;
use std::path::Path;

fn main() -> glib::ExitCode {
    // Claude runs the hook once per event, with no display and no time to
    // spare, so this branch is taken before GTK is initialised at all. It must
    // never fail loudly: a broken hook would block the agent.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|value| value == "--hook") {
        if let Some(output) = arguments.get(1) {
            if let Some(line) = hook::run(Path::new(output), &mut std::io::stdin().lock()) {
                print!("{line}");
            }
        }
        return glib::ExitCode::SUCCESS;
    }
    convoy_gtk::run()
}
