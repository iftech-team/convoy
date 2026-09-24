//! Isolated provider homes.
//!
//! A session bound to a profile gets its own `CLAUDE_CONFIG_DIR` / `CODEX_HOME`
//! named by the SHA-256 of the profile id, and loses any API-key variables so
//! the profile's own sign-in is the only credential in play. A session that has
//! already run keeps the home recorded at launch, even if the root moves.

use crate::hash::sha256_hex;
use crate::provider::launch::agent_environment;
use crate::workspace::model::{Agent, Profile, Session};
use crate::{bail, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const KEY_VARIABLES: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
];

#[derive(Debug, Clone)]
pub struct Account {
    pub home: PathBuf,
    pub env: BTreeMap<String, String>,
}

pub fn home_key(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "CLAUDE_CONFIG_DIR",
        Agent::Codex => "CODEX_HOME",
    }
}

pub fn account_environment(
    session: &Session,
    profiles: &[Profile],
    root: &Path,
    source: &BTreeMap<String, String>,
) -> Result<Account> {
    let key = home_key(session.agent);
    let profile = match session.profile_id.as_deref().filter(|id| !id.is_empty()) {
        None => None,
        Some(id) => {
            let found = profiles
                .iter()
                .find(|profile| profile.id == id && profile.agent == session.agent);
            if found.is_none() {
                bail!("Session account profile is missing.")
            }
            found
        }
    };
    let home = match session.agent_home.clone() {
        Some(home) => home,
        None => match profile {
            Some(profile) => root.join(sha256_hex(&profile.id)),
            None => {
                let fallback = source
                    .get(key)
                    .cloned()
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        default_home_root(source).join(match session.agent {
                            Agent::Claude => ".claude",
                            Agent::Codex => ".codex",
                        })
                    });
                resolve(&fallback)
            }
        },
    };
    let mut env = agent_environment(source);
    env.insert(key.to_string(), home.to_string_lossy().into_owned());
    if profile.is_some() {
        for name in KEY_VARIABLES {
            env.remove(name);
        }
    }
    Ok(Account { home, env })
}

fn default_home_root(source: &BTreeMap<String, String>) -> PathBuf {
    source
        .get(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(crate::storage::home)
}

/// `path.resolve()` — relative values are taken against the working directory.
fn resolve(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    }
}
