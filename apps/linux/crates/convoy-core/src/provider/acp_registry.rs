//! The official ACP agent registry: the agents anyone can add in one step.
//!
//! An entry is run one of three ways. `npx` and `uvx` fetch the package on
//! first run, so nothing is installed by hand; `binary` is an archive per
//! platform, with its SHA-256, that Convoy downloads, checks and unpacks into
//! its own folder. Fetching and unpacking happen in the app; reading the
//! document, picking the platform and turning an entry into an agent
//! definition happen here, where they can be tested.

use super::acp_agents::{valid_id, AcpAgentDef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// More than any real registry; a larger document is cut.
const MAX_AGENTS: usize = 1000;

/// A download the app makes itself: the archive for this platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryInstall {
    pub archive: String,
    pub sha256: String,
    /// The program inside the archive, relative to where it unpacks.
    pub cmd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// `npx -y <package> <args>`.
    Npx { package: String, args: Vec<String>, env: BTreeMap<String, String> },
    /// `uvx <package> <args>`.
    Uvx { package: String, args: Vec<String>, env: BTreeMap<String, String> },
    Binary { install: BinaryInstall, args: Vec<String>, env: BTreeMap<String, String> },
}

impl Launch {
    pub fn kind(&self) -> &'static str {
        match self {
            Launch::Npx { .. } => "npx",
            Launch::Uvx { .. } => "uvx",
            Launch::Binary { .. } => "binary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub website: Option<String>,
    pub icon: Option<String>,
    /// How it runs on this computer; none when the registry offers nothing
    /// for this platform.
    pub launch: Option<Launch>,
}

/// The registry's name for this computer's platform.
pub fn platform() -> Option<&'static str> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        _ => return None,
    };
    Some(match (os, arch) {
        ("darwin", "aarch64") => "darwin-aarch64",
        ("darwin", _) => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", _) => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        _ => "windows-x86_64",
    })
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn env(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(name, _)| {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
        .collect()
}

/// A path inside the unpacked archive: relative, and never climbing out.
pub fn safe_cmd(cmd: &str) -> bool {
    let path = Path::new(cmd);
    !cmd.is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
}

fn https(url: &str) -> bool {
    url.starts_with("https://") && !url.contains(char::is_whitespace)
}

fn launch(distribution: &Value, platform: Option<&str>) -> Option<Launch> {
    let package = |key: &str| {
        let entry = distribution.get(key)?;
        let package = text(entry, "package")?;
        // A package spec is one word; anything else is not passed on.
        (!package.starts_with('-') && !package.contains(char::is_whitespace))
            .then(|| (package, strings(entry.get("args")), env(entry.get("env"))))
    };
    if let Some((package, args, env)) = package("npx") {
        return Some(Launch::Npx { package, args, env });
    }
    if let Some((package, args, env)) = package("uvx") {
        return Some(Launch::Uvx { package, args, env });
    }
    let target = distribution.get("binary")?.get(platform?)?;
    let archive = text(target, "archive").filter(|url| https(url))?;
    let sha256 = text(target, "sha256")
        .map(|hash| hash.to_ascii_lowercase())
        .filter(|hash| hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))?;
    let cmd = text(target, "cmd").filter(|cmd| safe_cmd(cmd))?;
    Some(Launch::Binary {
        install: BinaryInstall { archive, sha256, cmd },
        args: strings(target.get("args")),
        env: env(target.get("env")),
    })
}

/// The agents in a registry document, for `platform`. Malformed entries and
/// repeated ids are left out rather than failing the whole list.
pub fn parse(document: &str, platform: Option<&str>) -> crate::Result<Vec<RegistryAgent>> {
    let root: Value = serde_json::from_str(document)?;
    let Some(entries) = root.get("agents").and_then(Value::as_array) else {
        crate::bail!("The ACP registry sent something that is not a registry.")
    };
    let mut seen = std::collections::HashSet::new();
    let mut agents = Vec::new();
    for entry in entries.iter().take(MAX_AGENTS) {
        let (Some(id), Some(name), Some(version)) =
            (text(entry, "id"), text(entry, "name"), text(entry, "version"))
        else {
            continue;
        };
        if !valid_id(&id) || !seen.insert(id.clone()) {
            continue;
        }
        let distribution = entry.get("distribution").unwrap_or(&Value::Null);
        agents.push(RegistryAgent {
            launch: launch(distribution, platform),
            description: text(entry, "description").unwrap_or_default(),
            website: text(entry, "website")
                .or_else(|| text(entry, "repository"))
                .filter(|url| https(url)),
            icon: text(entry, "icon").filter(|url| https(url)),
            id,
            name: name.chars().take(60).collect(),
            version,
        });
    }
    Ok(agents)
}

/// Where a downloaded agent version is unpacked.
pub fn install_dir(root: &Path, id: &str, version: &str) -> PathBuf {
    let version: String = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        .collect();
    root.join("bin").join(id).join(if version.is_empty() { "current".into() } else { version })
}

/// The agent definition an entry becomes. `root` is Convoy's agents folder,
/// where a binary is unpacked. None when it cannot run here.
pub fn definition(agent: &RegistryAgent, root: &Path) -> Option<AcpAgentDef> {
    let (command, args, env, binary) = match agent.launch.clone()? {
        Launch::Npx { package, args, env } => (
            "npx".to_string(),
            [vec!["-y".to_string(), package], args].concat(),
            env,
            None,
        ),
        Launch::Uvx { package, args, env } => {
            ("uvx".to_string(), [vec![package], args].concat(), env, None)
        }
        Launch::Binary { install, args, env } => {
            let program = install_dir(root, &agent.id, &agent.version).join(&install.cmd);
            (program.to_string_lossy().into_owned(), args, env, Some(install))
        }
    };
    Some(AcpAgentDef {
        id: agent.id.clone(),
        name: agent.name.clone(),
        command,
        args,
        env,
        tui_command: None,
        tui_args: Vec::new(),
        tui_prompt_flag: None,
        login_command: None,
        install_url: agent.website.clone(),
        registry: Some(agent.id.clone()),
        version: Some(agent.version.clone()),
        binary,
        builtin: false,
    })
}

/// SHA-256 of downloaded bytes, as the registry writes it.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether an icon file is an SVG and nothing else: shown through an
/// `<img>`, which runs no script, but still kept to what it says it is.
pub fn plausible_svg(bytes: &[u8]) -> bool {
    bytes.len() <= 256 * 1024
        && std::str::from_utf8(bytes).is_ok_and(|text| text.contains("<svg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"{
      "version": "1.0.0",
      "agents": [
        { "id": "gemini", "name": "Gemini CLI", "version": "0.71.1", "description": "Google's agent",
          "icon": "https://cdn.agentclientprotocol.com/registry/v1/latest/gemini.svg",
          "distribution": { "npx": { "package": "@google/gemini-cli@0.71.1", "args": ["--acp"] } } },
        { "id": "fast", "name": "fast-agent", "version": "1",
          "distribution": { "uvx": { "package": "fast-agent-acp==0.10.66", "args": ["-x"] } } },
        { "id": "opencode", "name": "OpenCode", "version": "2.4.1",
          "distribution": { "binary": {
            "darwin-aarch64": { "archive": "https://github.com/x/opencode-darwin-arm64.zip", "cmd": "./opencode",
              "args": ["acp"], "sha256": "95A101EDA95108478F1DB43CFF8FCBD4811F6C0E79AF1E167301843C6A48D731" },
            "linux-x86_64": { "archive": "http://insecure/x.tar.gz", "cmd": "./opencode",
              "sha256": "95a101eda95108478f1db43cff8fcbd4811f6c0e79af1e167301843c6a48d731" } } } },
        { "id": "escape", "name": "Escape", "version": "1",
          "distribution": { "binary": { "darwin-aarch64": { "archive": "https://x/y.zip", "cmd": "../../bin/sh",
            "sha256": "95a101eda95108478f1db43cff8fcbd4811f6c0e79af1e167301843c6a48d731" } } } },
        { "id": "Bad Id", "name": "x", "version": "1", "distribution": {} },
        { "id": "gemini", "name": "Again", "version": "2", "distribution": {} }
      ]
    }"#;

    #[test]
    fn entries_become_agents_for_this_platform() {
        let agents = parse(DOCUMENT, Some("darwin-aarch64")).unwrap();
        let ids: Vec<&str> = agents.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(ids, vec!["gemini", "fast", "opencode", "escape"], "bad and repeated ids are left out");

        let root = Path::new("/data/acp-agents");
        let gemini = definition(&agents[0], root).unwrap();
        assert_eq!(gemini.command, "npx");
        assert_eq!(gemini.args, vec!["-y", "@google/gemini-cli@0.71.1", "--acp"]);
        assert_eq!(gemini.registry.as_deref(), Some("gemini"));
        assert!(agents[0].icon.is_some());

        let fast = definition(&agents[1], root).unwrap();
        assert_eq!((fast.command.as_str(), fast.args.clone()), ("uvx", vec!["fast-agent-acp==0.10.66".to_string(), "-x".into()]));

        let opencode = definition(&agents[2], root).unwrap();
        assert_eq!(opencode.command, "/data/acp-agents/bin/opencode/2.4.1/./opencode");
        assert_eq!(opencode.args, vec!["acp"]);
        let install = opencode.binary.unwrap();
        assert_eq!(install.sha256, "95a101eda95108478f1db43cff8fcbd4811f6c0e79af1e167301843c6a48d731");

        assert!(agents[3].launch.is_none(), "a program outside its own folder is refused");
        assert!(definition(&agents[3], root).is_none());
    }

    #[test]
    fn a_binary_needs_https_and_this_platform() {
        let linux = parse(DOCUMENT, Some("linux-x86_64")).unwrap();
        let opencode = linux.iter().find(|agent| agent.id == "opencode").unwrap();
        assert!(opencode.launch.is_none(), "plain http is not downloaded");
        let windows = parse(DOCUMENT, Some("windows-x86_64")).unwrap();
        assert!(windows.iter().find(|agent| agent.id == "opencode").unwrap().launch.is_none());
        assert!(parse("{\"nope\":1}", None).is_err());
    }

    #[test]
    fn hashes_and_paths_are_checked() {
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(safe_cmd("./opencode") && safe_cmd("bin/agent"));
        assert!(!safe_cmd("../x") && !safe_cmd("/bin/sh") && !safe_cmd(""));
        assert!(plausible_svg(b"<?xml?><svg viewBox='0 0 1 1'/>"));
        assert!(!plausible_svg(b"\x89PNG"));
        assert_eq!(install_dir(Path::new("/r"), "a", "1.2/../3"), Path::new("/r/bin/a/1.2..3"));
    }
}
