//! Bringing a workspace across from the macOS app.
//!
//! The mapping lives in `convoy_core::macos_import`; this only finds the macOS
//! app's files. Anywhere but macOS there is nothing to find, and the page is
//! told so rather than shown a button that cannot work.

use super::Workspace;
use crate::tasks::Trackers;
use convoy_core::macos_import::Report;
use tauri::State;

#[cfg(target_os = "macos")]
mod sources {
    use convoy_core::macos_import::MacSources;
    use serde_json::{Map, Number, Value};
    use std::path::PathBuf;

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    /// Where the macOS app keeps its workspace and its settings.
    pub fn read() -> Result<Option<MacSources>, String> {
        let Some(home) = home() else { return Ok(None) };
        let support = home.join("Library/Application Support/Convoy");
        let preferences = home.join("Library/Preferences/com.iftech.convoy.plist");
        let defaults = match plist::Value::from_file(&preferences) {
            Ok(plist::Value::Dictionary(dictionary)) => dictionary
                .into_iter()
                .map(|(key, value)| (key, json(value)))
                .collect(),
            _ => Map::new(),
        };
        MacSources::read(&support, defaults, &home.join(".claude"))
            .map_err(|error| error.to_string())
    }

    /// A property list value as JSON. The macOS app stores some settings as
    /// encoded JSON in a data value; those arrive as the text they hold.
    fn json(value: plist::Value) -> Value {
        match value {
            plist::Value::Array(items) => Value::Array(items.into_iter().map(json).collect()),
            plist::Value::Dictionary(dictionary) => Value::Object(
                dictionary
                    .into_iter()
                    .map(|(key, value)| (key, json(value)))
                    .collect(),
            ),
            plist::Value::Boolean(flag) => Value::Bool(flag),
            plist::Value::Data(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Value::String(text),
                Err(error) => Value::String(convoy_core::integrations::base64(error.as_bytes())),
            },
            plist::Value::Real(real) => Number::from_f64(real).map_or(Value::Null, Value::Number),
            plist::Value::Integer(integer) => integer
                .as_signed()
                .map(Value::from)
                .or_else(|| integer.as_unsigned().map(Value::from))
                .unwrap_or(Value::Null),
            plist::Value::String(text) => Value::String(text),
            plist::Value::Date(date) => Value::String(date.to_xml_format()),
            _ => Value::Null,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod sources {
    pub fn read() -> Result<Option<convoy_core::macos_import::MacSources>, String> {
        Ok(None)
    }
}

/// What an import would bring in, or nothing when there is no macOS app data.
#[tauri::command]
pub fn macos_import_preview(
    settings: bool,
    workspace: State<'_, Workspace>,
) -> Result<Option<Report>, String> {
    let Some(found) = sources::read()? else {
        return Ok(None);
    };
    let storage = workspace.storage.clone();
    workspace.with(|core| {
        Ok(Some(convoy_core::macos_import::preview(
            core, &storage, &found, settings,
        )))
    })
}

#[tauri::command]
pub fn macos_import_run(
    settings: bool,
    workspace: State<'_, Workspace>,
    trackers: State<'_, Trackers>,
) -> Result<Report, String> {
    let found = sources::read()?.ok_or("There is no macOS app workspace on this Mac.")?;
    let storage = workspace.storage.clone();
    let report = workspace
        .act(|core| convoy_core::macos_import::import(core, &storage, &found, settings))?;
    // The connections were written to disk beside the cached copy.
    trackers.forget();
    Ok(report)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// Reads this Mac's own macOS app data, when there is any. Run with
    /// `cargo test macos_app_data -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the macOS app's data on this Mac"]
    fn macos_app_data_reads() {
        let Some(found) = super::sources::read().expect("readable") else {
            return eprintln!("no macOS app workspace here");
        };
        let keys: Vec<_> = found.defaults.keys().cloned().collect();
        eprintln!("{} settings keys: {keys:?}", keys.len());
        let storage = convoy_core::Storage::new(std::env::temp_dir().join("convoy-import-probe"));
        let workspace =
            convoy_core::Workspace::load(storage.workspace_file()).expect("empty store");
        let report = convoy_core::macos_import::preview(&workspace, &storage, &found, true);
        eprintln!("{}", serde_json::to_string(&report).unwrap());
    }
}
