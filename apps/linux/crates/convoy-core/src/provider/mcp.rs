//! MCP servers Convoy hands every agent it starts, so a session can read and
//! update tracker tasks without the user running `claude mcp add` first.
//!
//! Today that is Peshbar: each Peshbar connection with a saved token becomes
//! an HTTP server named `peshbar` (`peshbar-2`, … for further ones). The token
//! never travels in an argument vector, where any local user could read it in
//! the process list: Claude reads it from an owner-only config file, Codex from
//! an environment variable named in its config.
//!
//! A session in a project linked to a Peshbar workspace sends that workspace
//! with every call (`X-Workspace`), so its agent reads and updates tasks there
//! rather than in whichever workspace the web app has open.

use crate::integrations::scopes::WORKSPACE_HEADER;
use crate::integrations::{Connection, ProjectLink, TrackerAuth, TrackerKind};
use crate::workspace::model::Agent;
use crate::Result;
use serde_json::{json, Map};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    pub token: String,
    /// The Peshbar workspace every call runs in; `None` is the user's current one.
    pub workspace: Option<String>,
}

impl McpServer {
    /// The headers beside the token: the workspace, when there is one.
    fn headers(&self) -> Vec<(&'static str, String)> {
        self.workspace
            .iter()
            .map(|workspace| (WORKSPACE_HEADER, workspace.clone()))
            .collect()
    }
}

impl McpServer {
    /// The variable Codex reads the bearer token from: `CONVOY_PESHBAR_2_TOKEN`.
    pub fn token_variable(&self) -> String {
        format!(
            "CONVOY_{}_TOKEN",
            self.name
                .to_uppercase()
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    }
}

/// The servers for these connections: Peshbar connections that use a token.
/// A Peshbar connection in MCP mode is the user's own server and is left alone.
/// The one the session's project is linked to runs in the linked workspace.
pub fn servers(connections: &[Connection], link: Option<&ProjectLink>) -> Vec<McpServer> {
    connections
        .iter()
        .filter(|connection| {
            connection.kind == TrackerKind::Peshbar
                && connection.auth == TrackerAuth::ApiKey
                && !connection.secret.is_empty()
        })
        .enumerate()
        .map(|(index, connection)| McpServer {
            name: if index == 0 {
                "peshbar".into()
            } else {
                format!("peshbar-{}", index + 1)
            },
            url: format!("{}/mcp", connection.peshbar_api()),
            token: connection.secret.clone(),
            workspace: link
                .filter(|link| link.connection_id == connection.id)
                .and_then(|link| link.workspace.as_ref())
                .map(|workspace| workspace.id.clone()),
        })
        .collect()
}

/// The `--mcp-config` file for a Claude session, beside its hook settings.
pub fn config_path(root: &Path, session_id: &str) -> PathBuf {
    root.join(format!("{session_id}.mcp.json"))
}

/// What a launch adds for `servers`: Claude gets `--mcp-config` with an
/// owner-only file written under `root`; Codex gets `-c mcp_servers.…` and the
/// tokens in `env`. Nothing when there are no servers, and a stale Claude
/// file from an earlier launch is removed then.
pub fn flags(
    agent: Agent,
    servers: &[McpServer],
    root: &Path,
    session_id: &str,
    env: &mut BTreeMap<String, String>,
) -> Result<Vec<String>> {
    if servers.is_empty() {
        if agent == Agent::Claude {
            let _ = std::fs::remove_file(config_path(root, session_id));
        }
        return Ok(Vec::new());
    }
    match agent {
        Agent::Claude => {
            let mut entries = Map::new();
            for server in servers {
                let mut headers = Map::new();
                headers.insert(
                    "Authorization".into(),
                    format!("Bearer {}", server.token).into(),
                );
                for (name, value) in server.headers() {
                    headers.insert(name.into(), value.into());
                }
                entries.insert(
                    server.name.clone(),
                    json!({ "type": "http", "url": server.url, "headers": headers }),
                );
            }
            std::fs::create_dir_all(root)?;
            let file = config_path(root, session_id);
            let mut handle = crate::fs::create_private(&file)?;
            handle
                .write_all(serde_json::to_string(&json!({ "mcpServers": entries }))?.as_bytes())?;
            Ok(vec![
                "--mcp-config".into(),
                file.to_string_lossy().into_owned(),
            ])
        }
        Agent::Codex => {
            let mut flags = Vec::new();
            for server in servers {
                let variable = server.token_variable();
                env.insert(variable.clone(), server.token.clone());
                let key = format!("mcp_servers.{}", toml_key(&server.name));
                flags.push("-c".into());
                flags.push(format!("{key}.url={}", toml_string(&server.url)));
                flags.push("-c".into());
                flags.push(format!(
                    "{key}.bearer_token_env_var={}",
                    toml_string(&variable)
                ));
                let headers = server.headers();
                if !headers.is_empty() {
                    let table: Vec<String> = headers
                        .iter()
                        .map(|(name, value)| {
                            format!("{} = {}", toml_string(name), toml_string(value))
                        })
                        .collect();
                    flags.push("-c".into());
                    flags.push(format!("{key}.http_headers={{ {} }}", table.join(", ")));
                }
            }
            Ok(flags)
        }
    }
}

/// A TOML key: bare when it can be, quoted otherwise (`peshbar-2` is bare).
fn toml_key(name: &str) -> String {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        name.to_string()
    } else {
        toml_string(name)
    }
}

/// A TOML basic string; JSON string escaping is a valid subset of it.
fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peshbar(id: &str, site: Option<&str>, auth: TrackerAuth, secret: &str) -> Connection {
        Connection {
            id: id.into(),
            kind: TrackerKind::Peshbar,
            auth,
            name: id.into(),
            site: site.map(str::to_string),
            username: None,
            secret: secret.into(),
        }
    }

    #[test]
    fn only_peshbar_connections_with_a_token_become_servers() {
        let mut linear = peshbar("l", None, TrackerAuth::ApiKey, "lin_api_x");
        linear.kind = TrackerKind::Linear;
        let found = servers(
            &[
                linear,
                peshbar("a", None, TrackerAuth::ApiKey, "1|aaa"),
                peshbar("m", None, TrackerAuth::Mcp, ""),
                peshbar(
                    "b",
                    Some("http://localhost:8000/"),
                    TrackerAuth::ApiKey,
                    "2|bbb",
                ),
            ],
            None,
        );
        assert_eq!(
            found,
            vec![
                McpServer {
                    name: "peshbar".into(),
                    url: "https://management-api.peshbar.com/mcp".into(),
                    token: "1|aaa".into(),
                    workspace: None,
                },
                McpServer {
                    name: "peshbar-2".into(),
                    url: "http://localhost:8000/mcp".into(),
                    token: "2|bbb".into(),
                    workspace: None,
                },
            ]
        );
        assert_eq!(found[1].token_variable(), "CONVOY_PESHBAR_2_TOKEN");
    }

    #[test]
    fn claude_reads_the_token_from_a_private_file_not_the_command_line() {
        let root = tempfile::tempdir().unwrap();
        let servers = servers(&[peshbar("a", None, TrackerAuth::ApiKey, "1|secret")], None);
        let mut env = BTreeMap::new();
        let flags = flags(Agent::Claude, &servers, root.path(), "s1", &mut env).unwrap();

        let file = config_path(root.path(), "s1");
        assert_eq!(
            flags,
            vec![
                "--mcp-config".to_string(),
                file.to_string_lossy().into_owned()
            ]
        );
        assert!(!flags.iter().any(|flag| flag.contains("secret")));
        assert!(env.is_empty());
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["peshbar"]["type"], "http");
        assert_eq!(
            config["mcpServers"]["peshbar"]["url"],
            "https://management-api.peshbar.com/mcp"
        );
        assert_eq!(
            config["mcpServers"]["peshbar"]["headers"]["Authorization"],
            "Bearer 1|secret"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        // Without servers the next launch drops the stale file and adds nothing.
        assert!(flags_for_none(root.path()).is_empty());
        assert!(!file.exists());
    }

    fn flags_for_none(root: &Path) -> Vec<String> {
        flags(Agent::Claude, &[], root, "s1", &mut BTreeMap::new()).unwrap()
    }

    #[test]
    fn codex_gets_config_overrides_and_the_token_in_its_environment() {
        let root = tempfile::tempdir().unwrap();
        let servers = servers(&[peshbar("a", None, TrackerAuth::ApiKey, "1|secret")], None);
        let mut env = BTreeMap::new();
        let flags = flags(Agent::Codex, &servers, root.path(), "s1", &mut env).unwrap();

        assert_eq!(
            flags,
            vec![
                "-c",
                "mcp_servers.peshbar.url=\"https://management-api.peshbar.com/mcp\"",
                "-c",
                "mcp_servers.peshbar.bearer_token_env_var=\"CONVOY_PESHBAR_TOKEN\"",
            ]
        );
        assert_eq!(
            env.get("CONVOY_PESHBAR_TOKEN").map(String::as_str),
            Some("1|secret")
        );
        assert!(!config_path(root.path(), "s1").exists());
    }

    fn linked(connection_id: &str) -> ProjectLink {
        ProjectLink {
            connection_id: connection_id.into(),
            workspace: Some(crate::integrations::Scope {
                id: "7".into(),
                name: "Tojiron".into(),
                key: None,
            }),
            project: None,
        }
    }

    #[test]
    fn only_the_linked_connection_runs_in_the_linked_workspace() {
        let connections = [
            peshbar("a", None, TrackerAuth::ApiKey, "1|aaa"),
            peshbar("b", None, TrackerAuth::ApiKey, "2|bbb"),
        ];
        let found = servers(&connections, Some(&linked("b")));
        assert_eq!(found[0].workspace, None);
        assert_eq!(found[1].workspace.as_deref(), Some("7"));
        // A link with no workspace leaves the current one.
        let mut whole = linked("b");
        whole.workspace = None;
        assert_eq!(servers(&connections, Some(&whole))[1].workspace, None);
    }

    #[test]
    fn the_workspace_reaches_both_agents_as_a_header() {
        let root = tempfile::tempdir().unwrap();
        let servers = servers(
            &[peshbar("a", None, TrackerAuth::ApiKey, "1|secret")],
            Some(&linked("a")),
        );

        flags(
            Agent::Claude,
            &servers,
            root.path(),
            "s1",
            &mut BTreeMap::new(),
        )
        .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(config_path(root.path(), "s1")).unwrap())
                .unwrap();
        assert_eq!(
            config["mcpServers"]["peshbar"]["headers"]["X-Workspace"],
            "7"
        );
        assert_eq!(
            config["mcpServers"]["peshbar"]["headers"]["Authorization"],
            "Bearer 1|secret"
        );

        let codex = flags(
            Agent::Codex,
            &servers,
            root.path(),
            "s2",
            &mut BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            codex.last().map(String::as_str),
            Some("mcp_servers.peshbar.http_headers={ \"X-Workspace\" = \"7\" }")
        );
    }
}
