//! The models an agent offers, for picking one instead of typing its name.
//!
//! Codex keeps the list its server sent in `models_cache.json` in its home, so
//! that is what is offered, in its own order. Claude Code keeps no such list;
//! its aliases always name the current models, and the model set in its
//! `settings.json` is offered too when it is something else.

use crate::workspace::model::Agent;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// What goes after `--model`.
    pub id: String,
    pub name: String,
}

fn model(id: &str, name: &str) -> Model {
    Model {
        id: id.to_string(),
        name: name.to_string(),
    }
}

/// The models for `agent`, read from its home (`~/.claude`, `~/.codex`, or an
/// account's). Never empty for Claude; empty for Codex before its first run.
pub fn list(agent: Agent, home: &Path) -> Vec<Model> {
    match agent {
        Agent::Claude => claude(home),
        Agent::Codex => codex(home),
    }
}

fn claude(home: &Path) -> Vec<Model> {
    let mut models = vec![
        model("opus", "Opus · most capable"),
        model("sonnet", "Sonnet · balanced"),
        model("haiku", "Haiku · fastest"),
    ];
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
        models.insert(0, model(&id, &format!("{id} · set in settings.json")));
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
            Some((priority, model(id, name)))
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
                {"slug":"b","display_name":"B","visibility":"list","priority":2},
                {"slug":"hidden","display_name":"H","visibility":"hide","priority":0},
                {"slug":"a","display_name":"A","visibility":"list","priority":1}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            list(Agent::Codex, dir.path()),
            vec![model("a", "A"), model("b", "B")]
        );
        assert!(list(Agent::Codex, &dir.path().join("none")).is_empty());
    }

    #[test]
    fn claude_offers_its_aliases_and_the_configured_model() {
        let dir = tempfile::tempdir().unwrap();
        let ids = |home: &Path| {
            list(Agent::Claude, home)
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(dir.path()), ["opus", "sonnet", "haiku"]);
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"model":"claude-fable-5-1"}"#,
        )
        .unwrap();
        assert_eq!(
            ids(dir.path()),
            ["claude-fable-5-1", "opus", "sonnet", "haiku"]
        );
    }
}
