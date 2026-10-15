//! The models an agent offers, for picking one instead of typing its name.
//!
//! Codex keeps the list its server sent in `models_cache.json` in its home, so
//! that is what is offered, in its own order. Claude Code keeps no such list;
//! its aliases always name the newest model of each family, the server sends
//! the extra models an account may use (cached in `.claude.json`, which is how
//! Fable arrived), and the model set in its `settings.json` is offered too.
//! Each model says which effort levels it takes: Codex lists them per model;
//! Claude Code takes the same ones for every model, read from its `--help` so
//! a new level shows up with the CLI that brings it.

use crate::workspace::model::Agent;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// What goes after `--model`.
    pub id: String,
    pub name: String,
    /// The effort levels it takes, lowest first; empty when unknown.
    pub efforts: Vec<String>,
}

/// What `claude --effort` accepted when this was written; used only when the
/// installed CLI's `--help` cannot be read.
const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// The levels `claude --help` lists for `--effort`, as in
/// `--effort <level>  Effort level … (low, medium, high, xhigh, max)`.
pub fn claude_efforts_from_help(help: &str) -> Option<Vec<String>> {
    let start = help.find("--effort")?;
    let rest = &help[start + "--effort".len()..];
    // The option's own text ends where the next option starts.
    let own = rest.find("\n  -").map_or(rest, |end| &rest[..end]);
    let open = own.find('(')?;
    let close = open + own[open..].find(')')?;
    let levels: Vec<String> = own[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|level| !level.is_empty() && level.chars().all(|c| c.is_ascii_lowercase()))
        .map(str::to_string)
        .collect();
    (!levels.is_empty()).then_some(levels)
}

fn model(id: &str, name: &str) -> Model {
    Model {
        id: id.to_string(),
        name: name.to_string(),
        efforts: Vec::new(),
    }
}

fn claude_model(id: &str, name: &str, efforts: &[String]) -> Model {
    Model {
        efforts: efforts.to_vec(),
        ..model(id, name)
    }
}

/// The models for `agent`, read from its home (`~/.claude`, `~/.codex`, or an
/// account's). Never empty for Claude; empty for Codex before its first run.
/// `claude_efforts` is what the installed CLI's `--help` lists, when read.
pub fn list(agent: Agent, home: &Path, claude_efforts: Option<&[String]>) -> Vec<Model> {
    match agent {
        Agent::Claude => {
            let fallback: Vec<String> = CLAUDE_EFFORTS
                .iter()
                .map(|level| level.to_string())
                .collect();
            claude(
                home,
                claude_efforts
                    .filter(|levels| !levels.is_empty())
                    .unwrap_or(&fallback),
            )
        }
        Agent::Codex => codex(home),
        Agent::Gemini => gemini(),
    }
}

/// Gemini CLI's models as version 0.32 names them; its own `/model` picker
/// lists the same. It caches no list to read.
fn gemini() -> Vec<Model> {
    [
        ("auto-gemini-3", "Auto · Gemini 3"),
        ("gemini-3-pro-preview", "Gemini 3 Pro"),
        ("gemini-3-flash-preview", "Gemini 3 Flash"),
        ("auto-gemini-2.5", "Auto · Gemini 2.5"),
        ("gemini-2.5-pro", "Gemini 2.5 Pro"),
        ("gemini-2.5-flash", "Gemini 2.5 Flash"),
    ]
    .into_iter()
    .map(|(id, name)| Model {
        id: id.into(),
        name: name.into(),
        efforts: Vec::new(),
    })
    .collect()
}

/// The extra models the server offers this account, as Claude Code caches
/// them: `.claude.json` in an account's config folder, or `~/.claude.json`
/// beside the default `~/.claude`.
fn claude_server_models(home: &Path) -> Vec<(String, String)> {
    let inside = home.join(".claude.json");
    let file = if inside.is_file() {
        inside
    } else {
        match home.parent() {
            Some(parent) if home.file_name().is_some_and(|name| name == ".claude") => {
                parent.join(".claude.json")
            }
            _ => return Vec::new(),
        }
    };
    let Some(config) = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return Vec::new();
    };
    config
        .get("additionalModelOptionsCache")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("value").and_then(Value::as_str)?;
            let label = entry.get("label").and_then(Value::as_str).unwrap_or(id);
            // "Fable 5.1 · Most capable for …": the first part names it best.
            let name = entry
                .get("description")
                .and_then(Value::as_str)
                .and_then(|text| text.split(" · ").next())
                .filter(|text| !text.is_empty())
                .unwrap_or(label);
            let name = if id.ends_with("[1m]") {
                format!("{name} · 1M context")
            } else {
                name.to_string()
            };
            Some((id.to_string(), name))
        })
        .filter(|(id, _)| !id.is_empty() && crate::patterns::MODEL.is_match(id))
        .collect()
}

fn claude(home: &Path, efforts: &[String]) -> Vec<Model> {
    // The CLI's aliases: each names the newest model of its family.
    let mut models = vec![
        claude_model("fable", "Fable · most capable", efforts),
        claude_model("opus", "Opus", efforts),
        claude_model("sonnet", "Sonnet · balanced", efforts),
        claude_model("haiku", "Haiku · fastest", efforts),
    ];
    for (id, name) in claude_server_models(home) {
        if !models.iter().any(|known| known.id == id) {
            models.push(claude_model(&id, &name, efforts));
        }
    }
    let configured = std::fs::read_to_string(home.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|settings| {
            settings
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|id| !id.is_empty() && !models.iter().any(|known| &known.id == id));
    if let Some(id) = configured {
        models.insert(
            0,
            claude_model(&id, &format!("{id} · set in settings.json"), efforts),
        );
    }
    models
}

fn codex(home: &Path) -> Vec<Model> {
    let Some(cache) = std::fs::read_to_string(home.join("models_cache.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return Vec::new();
    };
    let mut listed: Vec<(i64, Model)> = cache
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .get("visibility")
                .and_then(Value::as_str)
                .is_none_or(|v| v == "list")
        })
        .filter_map(|entry| {
            let id = entry.get("slug").and_then(Value::as_str)?;
            let name = entry
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(id);
            let priority = entry
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            let efforts = entry
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|level| level.get("effort").and_then(Value::as_str))
                .filter(|level| super::EFFORTS.contains(level))
                .map(str::to_string)
                .collect();
            Some((
                priority,
                Model {
                    efforts,
                    ..model(id, name)
                },
            ))
        })
        .collect();
    listed.sort_by_key(|(priority, _)| *priority);
    listed.into_iter().map(|(_, model)| model).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_offers_its_listed_models_in_its_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("models_cache.json"),
            r#"{"models":[
                {"slug":"b","display_name":"B","visibility":"list","priority":2,
                 "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"bogus"}]},
                {"slug":"hidden","display_name":"H","visibility":"hide","priority":0},
                {"slug":"a","display_name":"A","visibility":"list","priority":1}
            ]}"#,
        )
        .unwrap();
        let b = Model {
            efforts: vec!["low".into(), "high".into()],
            ..model("b", "B")
        };
        assert_eq!(
            list(Agent::Codex, dir.path(), None),
            vec![model("a", "A"), b]
        );
        assert!(list(Agent::Codex, &dir.path().join("none"), None).is_empty());
    }

    #[test]
    fn claude_offers_its_aliases_and_the_configured_model() {
        let dir = tempfile::tempdir().unwrap();
        let ids = |home: &Path| {
            list(Agent::Claude, home, None)
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(dir.path()), ["fable", "opus", "sonnet", "haiku"]);
        assert!(list(Agent::Claude, dir.path(), None)
            .iter()
            .all(|m| m.efforts == CLAUDE_EFFORTS));
        let read = vec!["low".to_string(), "ultra".to_string()];
        assert!(list(Agent::Claude, dir.path(), Some(&read))
            .iter()
            .all(|m| m.efforts == read));
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"model":"claude-fable-5-1"}"#,
        )
        .unwrap();
        assert_eq!(
            ids(dir.path()),
            ["claude-fable-5-1", "fable", "opus", "sonnet", "haiku"]
        );
    }

    #[test]
    fn claude_offers_the_models_its_server_sent() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(".claude");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            dir.path().join(".claude.json"),
            r#"{"additionalModelOptionsCache":[
                {"value":"claude-fable-5-1[1m]","label":"Fable","description":"Fable 5.1 · Most capable"},
                {"value":"bad model","label":"Bad"}
            ]}"#,
        )
        .unwrap();
        let models = list(Agent::Claude, &home, None);
        let extra = models.last().unwrap();
        assert_eq!(extra.id, "claude-fable-5-1[1m]");
        assert_eq!(extra.name, "Fable 5.1 · 1M context");
        assert_eq!(models.len(), 5, "an id the CLI could not take is left out");
    }

    #[test]
    fn effort_levels_come_from_the_cli_help() {
        let help = "  --effort <level>     Effort level for the current session\n                       (low, medium, high, xhigh, max)\n  --environment <id>   Create (a, b)\n";
        assert_eq!(
            claude_efforts_from_help(help).unwrap(),
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(claude_efforts_from_help("  --model <m>  (x, y)\n"), None);
    }
}
