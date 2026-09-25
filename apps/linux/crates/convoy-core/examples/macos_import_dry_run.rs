//! Imports a macOS app workspace into a scratch store and prints the report.
//! Usage: `cargo run -p convoy-core --example macos_import_dry_run -- <support dir> <defaults.json> <scratch store>`
//! Nothing outside the scratch store is written.

use convoy_core::macos_import::{import, MacSources};
use convoy_core::Storage;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let support = std::path::PathBuf::from(&args[0]);
    let defaults: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let home = std::env::var("HOME").unwrap();
    let sources = MacSources::read(
        &support,
        defaults,
        &std::path::Path::new(&home).join(".claude"),
    )
    .unwrap()
    .expect("no workspace.json in the support folder");
    let storage = Storage::new(&args[2]);
    let mut workspace = convoy_core::Workspace::load(storage.workspace_file()).unwrap();
    match import(&mut workspace, &storage, &sources, true) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => println!("IMPORT REFUSED: {error}"),
    }
}
