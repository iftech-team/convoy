//! Model providers that Claude Code reaches through an Anthropic-compatible
//! API: today DeepSeek. Such a provider is a Claude account of its own — its
//! own `CLAUDE_CONFIG_DIR`, so its conversations, hooks and usage are Claude
//! Code's as ever — whose launches point Claude Code at the provider and
//! carry its key. Everything Claude Code does then works on that provider's
//! models.
//!
//! The key is kept in an owner-only file in the account's folder, never in
//! the workspace file, and reaches Claude Code through its environment only.

use crate::model::{Agent, Profile};
use crate::provider::models::Model;
use crate::{bail, ensure, Result};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The providers Convoy knows.
pub const PROVIDERS: &[&str] = &["deepseek"];

/// The file in an account folder that holds the provider's key.
const KEY_FILE: &str = "convoy-api-key";

pub fn name(provider: &str) -> &'static str {
    match provider {
        "deepseek" => "DeepSeek",
        _ => "API",
    }
}

/// The variables that point Claude Code at the provider, as its own guide
/// gives them (api-docs.deepseek.com, Claude Code integration), with the key
/// added by the caller.
fn settings(provider: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match provider {
        "deepseek" => Some(vec![
            ("ANTHROPIC_BASE_URL", "https://api.deepseek.com/anthropic"),
            ("ANTHROPIC_MODEL", "deepseek-flash[1m]"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "deepseek-flash[1m]"),
            ("ANTHROPIC_DEFAULT_SONNET_MODEL", "deepseek-flash[1m]"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "deepseek-flash"),
            ("CLAUDE_CODE_SUBAGENT_MODEL", "deepseek-flash"),
            ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "786432"),
            // Nothing of Anthropic's own: no telemetry, no update checks.
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
        ]),
        _ => None,
    }
}

/// Variables of a provider launch that are not secret, which the login
/// shell is handed again in case the user's profile sets them otherwise.
pub const FORWARDED: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];

/// The models a provider account offers, for the pickers. DeepSeek maps
/// any `claude-opus…` name to V4 Pro, which is how Pro is reached.
pub fn models(provider: &str) -> Vec<Model> {
    let levels: Vec<String> = crate::provider::CLAUDE_EFFORTS.iter().map(|l| l.to_string()).collect();
    match provider {
        "deepseek" => vec![
            Model {
                id: "deepseek-flash[1m]".into(),
                name: "DeepSeek Flash".into(),
                efforts: levels.clone(),
            },
            Model {
                id: "claude-opus-4-5".into(),
                name: "DeepSeek V4 Pro".into(),
                efforts: levels,
            },
        ],
        _ => Vec::new(),
    }
}

/// The account a provider runs on, when it is connected.
pub fn provider_profile<'a>(profiles: &'a [Profile], provider: &str) -> Option<&'a Profile> {
    profiles
        .iter()
        .find(|profile| profile.agent == Agent::Claude && profile.provider.as_deref() == Some(provider))
}

/// What the window's agent key names: the agent, its ACP id, and the
/// account it runs on. `deepseek` is Claude Code on the DeepSeek account;
/// `claude`, `codex` and `acp:<id>` take no account of their own here.
pub fn resolve_key(profiles: &[Profile], key: &str) -> Result<(Agent, Option<String>, Option<String>)> {
    if PROVIDERS.contains(&key) {
        let Some(profile) = provider_profile(profiles, key) else {
            bail!("Connect {} in Settings → Agents first.", name(key))
        };
        return Ok((Agent::Claude, None, Some(profile.id.clone())));
    }
    match super::acp_agents::parse_key(key) {
        Some((agent, acp)) => Ok((agent, acp, None)),
        None => bail!("Unknown agent."),
    }
}

/// The key the window shows for an agent, its ACP id and its account: a
/// provider's account shows as the provider.
pub fn role_key(profiles: &[Profile], agent: Agent, acp: Option<&str>, profile: Option<&str>) -> String {
    let provider = profile.and_then(|id| {
        profiles
            .iter()
            .find(|item| item.id == id && item.agent == agent)
            .and_then(|item| item.provider.clone())
    });
    match provider {
        Some(provider) => provider,
        None => super::acp_agents::key(agent, acp),
    }
}

pub fn key_file(accounts: &Path, profile_id: &str) -> PathBuf {
    accounts
        .join(crate::hash::sha256_hex(profile_id))
        .join(KEY_FILE)
}

/// Checks a key as typed: one line of plain characters.
pub fn valid_key(key: &str) -> bool {
    let key = key.trim();
    (8..=400).contains(&key.len()) && key.chars().all(|c| c.is_ascii_graphic())
}

/// Saves the key, owner-only, and readies the account folder so Claude Code
/// starts without its first-run questions: there is no Anthropic login here.
pub fn save_key(accounts: &Path, profile_id: &str, key: &str) -> Result<()> {
    let key = key.trim();
    ensure!(valid_key(key), "Paste the API key as it was given: one line, no spaces.");
    let file = key_file(accounts, profile_id);
    let folder = file.parent().expect("a key file has a folder").to_path_buf();
    std::fs::create_dir_all(&folder)?;
    let _ = std::fs::remove_file(&file);
    let mut handle = crate::fs::create_private(&file)?;
    handle.write_all(key.as_bytes())?;
    let config = folder.join(".claude.json");
    if !config.exists() {
        std::fs::write(&config, r#"{"hasCompletedOnboarding":true}"#)?;
    }
    Ok(())
}

pub fn remove_key(accounts: &Path, profile_id: &str) {
    let _ = std::fs::remove_file(key_file(accounts, profile_id));
}

/// The variables a launch of this provider's account gets.
pub fn environment(provider: &str, accounts: &Path, profile_id: &str) -> Result<BTreeMap<String, String>> {
    let Some(pairs) = settings(provider) else {
        bail!("Unknown model provider: {provider}.")
    };
    let key = std::fs::read_to_string(key_file(accounts, profile_id))
        .map(|key| key.trim().to_string())
        .unwrap_or_default();
    ensure!(
        !key.is_empty(),
        "The {} key is missing. Add it in Settings → Agents.",
        name(provider)
    );
    let mut env: BTreeMap<String, String> = pairs
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), key);
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_key_points_claude_code_at_the_provider() {
        let root = tempfile::tempdir().unwrap();
        assert!(environment("deepseek", root.path(), "p1").is_err(), "no key yet");
        assert!(save_key(root.path(), "p1", "has space").is_err());
        save_key(root.path(), "p1", "  sk-0123456789abcdef \n").unwrap();
        let env = environment("deepseek", root.path(), "p1").unwrap();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://api.deepseek.com/anthropic");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-0123456789abcdef");
        assert_eq!(env["ANTHROPIC_MODEL"], "deepseek-flash[1m]");
        let folder = key_file(root.path(), "p1").parent().unwrap().to_path_buf();
        assert!(folder.join(".claude.json").is_file(), "no first-run questions");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_file(root.path(), "p1")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "only the owner reads the key");
        }
        assert!(FORWARDED.iter().all(|name| *name != "ANTHROPIC_AUTH_TOKEN"), "the key never goes on a command line");
        assert_eq!(models("deepseek").len(), 2);

        // The launch names the provider but never carries the key, and an
        // Anthropic key from the shell profile cannot take over.
        if !crate::Platform::current().is_windows() {
            let spec = crate::provider::provider_spec(crate::model::Agent::Claude, &["-p".into()], &env);
            let line = spec.args.join(" ");
            assert!(line.contains("'-u' 'ANTHROPIC_API_KEY'"), "{line}");
            assert!(line.contains("ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic"), "{line}");
            assert!(!line.contains("sk-0123456789abcdef"), "{line}");
        }
        remove_key(root.path(), "p1");
        assert!(environment("deepseek", root.path(), "p1").is_err());
    }
}
