//! The ACP agents the user can pick — Antigravity, opencode (and through it
//! DeepSeek and the rest), Gemini CLI, and any they add — with whether each
//! is installed. Claude and Codex are listed by the window itself.

use convoy_core::provider::acp_agents::{self, AcpAgentDef};
use convoy_core::provider::acp_registry::{self, Launch, RegistryAgent};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::State;

use super::Workspace;

/// The agent, ACP id and account a window key names: `claude`, `codex`,
/// `acp:<id>` or a provider such as `deepseek`.
pub fn resolve(
    workspace: &Workspace,
    key: &str,
) -> Result<(convoy_core::model::Agent, Option<String>, Option<String>), String> {
    workspace.act(|core| convoy_core::provider::api::resolve_key(&core.state().profiles, key))
}

#[derive(Serialize)]
pub struct AcpAgentView {
    /// `acp:<id>`, as sessions and tasks name it.
    pub key: String,
    #[serde(flatten)]
    pub definition: AcpAgentDef,
    pub builtin: bool,
    pub installed: bool,
    /// Whether it has a terminal UI of its own; otherwise chat only.
    pub terminal: bool,
    /// Its registry icon, as a `data:` URL, once downloaded.
    pub icon: Option<String>,
}

/// Marks the line the probe prints for each command, so whatever a `.zshrc`
/// prints around it cannot be mistaken for an answer.
const MARK: &str = "__CONVOY_ACP__";

/// Which of `commands` the login shell finds, in one shell for all of them.
/// A full path, as a downloaded agent has, is simply looked at.
fn installed(commands: &[String]) -> HashMap<String, bool> {
    let mut found: HashMap<String, bool> = commands.iter().map(|c| (c.clone(), false)).collect();
    let (paths, commands): (Vec<String>, Vec<String>) =
        commands.iter().cloned().partition(|command| Path::new(command).is_absolute());
    for path in paths {
        found.insert(path.clone(), Path::new(&path).is_file());
    }
    if commands.is_empty() {
        return found;
    }
    if cfg!(windows) {
        for command in &commands {
            let ok = std::process::Command::new("where")
                .arg(command)
                .output()
                .is_ok_and(|output| output.status.success());
            found.insert(command.clone(), ok);
        }
        return found;
    }
    let script: Vec<String> = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            format!(
                "command -v {} >/dev/null 2>&1 && printf '\\n{MARK}{index}\\n'",
                convoy_core::provider::launch::quote(command)
            )
        })
        .collect();
    let (shell, args) = convoy_core::provider::launch::shell_command(script.join("; "));
    if let Ok(output) = std::process::Command::new(shell).args(args).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(index) = line
                .find(MARK)
                .and_then(|at| line[at + MARK.len()..].trim().parse::<usize>().ok())
            {
                if let Some(command) = commands.get(index) {
                    found.insert(command.clone(), true);
                }
            }
        }
    }
    found
}

/// Every ACP agent, shipped and added, with whether it is installed.
/// Probing starts a login shell, so it runs off the main thread.
#[tauri::command]
pub async fn acp_agents_list(workspace: State<'_, Workspace>) -> Result<Vec<AcpAgentView>, String> {
    let agents = workspace.act(|core| Ok(acp_agents::all(core.settings())))?;
    let root = workspace.storage.acp_agents();
    let commands: Vec<String> = agents.iter().map(|agent| agent.executable().to_string()).collect();
    let found = tauri::async_runtime::spawn_blocking(move || installed(&commands))
        .await
        .map_err(|error| error.to_string())?;
    Ok(agents
        .into_iter()
        .map(|agent| AcpAgentView {
            key: agent.key(),
            installed: found.get(agent.executable()).copied().unwrap_or(false),
            terminal: agent.has_terminal(),
            icon: agent.registry.as_deref().and_then(|id| icon_data(&root, id)),
            builtin: agent.builtin,
            definition: agent,
        })
        .collect())
}

/// Replaces the user's own agents: the ones they added, and shipped ones
/// they changed. A shipped agent left as shipped is not stored.
#[tauri::command]
pub fn acp_agents_save(
    agents: Vec<AcpAgentDef>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let shipped = acp_agents::builtins();
    let custom: Vec<AcpAgentDef> = agents
        .into_iter()
        .map(|mut agent| {
            agent.builtin = false;
            agent.id = agent.id.trim().to_string();
            agent.name = agent.name.trim().to_string();
            agent.command = agent.command.trim().to_string();
            agent.tui_command = agent.tui_command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent.tui_prompt_flag = agent.tui_prompt_flag.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent.login_command = agent.login_command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent
        })
        .filter(|agent| {
            !shipped.iter().any(|known| {
                let mut known = known.clone();
                known.builtin = false;
                known == *agent
            })
        })
        .collect();
    let root = workspace.storage.acp_agents();
    let removed: Vec<String> = workspace.act(|core| {
        let before: Vec<String> = core
            .settings()
            .acp_agents
            .iter()
            .filter(|agent| agent.binary.is_some())
            .map(|agent| agent.id.clone())
            .collect();
        core.save_settings(convoy_core::workspace::SettingsPatch {
            acp_agents: Some(custom.clone()),
            ..Default::default()
        })?;
        Ok(before
            .into_iter()
            .filter(|id| !custom.iter().any(|agent| &agent.id == id))
            .collect())
    })?;
    // A downloaded agent that is gone takes its files with it.
    for id in removed {
        if acp_agents::valid_id(&id) {
            let _ = std::fs::remove_dir_all(root.join("bin").join(id));
        }
    }
    Ok(())
}

// ------------------------------------------------------------- registry --

/// How long the registry's copy is trusted before it is fetched again.
const REGISTRY_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const REGISTRY_LIMIT: u64 = 5 * 1024 * 1024;
const ICON_LIMIT: u64 = 256 * 1024;
/// The largest archive an agent may download as.
const ARCHIVE_LIMIT: u64 = 600 * 1024 * 1024;

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = http()
        .get(url)
        .call()
        .map_err(|error| format!("Could not reach {url}: {error}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("{url} is larger than expected."));
    }
    Ok(bytes)
}

/// The registry document: the saved copy while it is fresh, else a new one,
/// else the saved copy however old.
fn registry_document(root: &Path, force: bool) -> Result<String, String> {
    let file = root.join("registry.json");
    let fresh = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < REGISTRY_AGE);
    if fresh && !force {
        if let Ok(text) = std::fs::read_to_string(&file) {
            return Ok(text);
        }
    }
    match fetch(acp_registry::REGISTRY_URL, REGISTRY_LIMIT)
        .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
    {
        Ok(text) if acp_registry::parse(&text, None).is_ok() => {
            let _ = std::fs::create_dir_all(root);
            let _ = std::fs::write(&file, &text);
            Ok(text)
        }
        Ok(_) => std::fs::read_to_string(&file)
            .map_err(|_| "The ACP registry sent something unreadable.".to_string()),
        Err(error) => std::fs::read_to_string(&file).map_err(|_| error),
    }
}

fn icon_file(root: &Path, id: &str) -> PathBuf {
    root.join("icons").join(format!("{id}.svg"))
}

/// A downloaded icon as a `data:` URL, which the window shows in an `<img>`.
fn icon_data(root: &Path, id: &str) -> Option<String> {
    if !acp_agents::valid_id(id) {
        return None;
    }
    let bytes = std::fs::read(icon_file(root, id)).ok()?;
    acp_registry::plausible_svg(&bytes)
        .then(|| format!("data:image/svg+xml;base64,{}", convoy_core::integrations::base64(&bytes)))
}

/// Downloads the icons not yet saved. A failed one is tried next time.
fn fetch_icons(root: &Path, agents: &[RegistryAgent]) {
    let _ = std::fs::create_dir_all(root.join("icons"));
    for agent in agents {
        let Some(url) = agent.icon.as_deref() else { continue };
        let file = icon_file(root, &agent.id);
        if file.exists() {
            continue;
        }
        if let Ok(bytes) = fetch(url, ICON_LIMIT) {
            if acp_registry::plausible_svg(&bytes) {
                let _ = std::fs::write(&file, bytes);
            }
        }
    }
}

#[derive(Serialize)]
pub struct RegistryView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub website: Option<String>,
    pub icon: Option<String>,
    /// `npx`, `uvx` or `binary`; none when nothing runs on this computer.
    pub kind: Option<&'static str>,
    /// The version added, when it is.
    pub added: Option<String>,
}

/// The registry, with which entries are added and at what version. Icons
/// are fetched as it is read. Network work happens off the main thread.
#[tauri::command]
pub async fn acp_registry_list(
    force: Option<bool>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<RegistryView>, String> {
    let root = workspace.storage.acp_agents();
    let added: HashMap<String, String> = workspace.act(|core| {
        Ok(core
            .settings()
            .acp_agents
            .iter()
            .filter_map(|agent| Some((agent.registry.clone()?, agent.version.clone().unwrap_or_default())))
            .collect())
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        let text = registry_document(&root, force.unwrap_or(false))?;
        let agents = acp_registry::parse(&text, acp_registry::platform()).map_err(|error| error.to_string())?;
        fetch_icons(&root, &agents);
        Ok(agents
            .into_iter()
            .map(|agent| RegistryView {
                icon: icon_data(&root, &agent.id),
                kind: agent.launch.as_ref().map(Launch::kind),
                added: added.get(&agent.id).cloned(),
                id: agent.id,
                name: agent.name,
                version: agent.version,
                description: agent.description,
                website: agent.website,
            })
            .collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Downloads an archive to a file, checking its SHA-256 as it goes.
fn download_checked(url: &str, sha256: &str, into: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let response = http()
        .get(url)
        .call()
        .map_err(|error| format!("Could not download the agent: {error}"))?;
    let mut reader = response.into_reader().take(ARCHIVE_LIMIT + 1);
    let mut file = std::fs::File::create(into).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut total: u64 = 0;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| format!("The download stopped: {error}"))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > ARCHIVE_LIMIT {
            return Err("The agent's download is larger than expected.".into());
        }
        hash.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
    }
    let got: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if got != sha256 {
        return Err("The download does not match the registry's checksum, so it was not installed.".into());
    }
    Ok(())
}

/// Unpacks a `.zip` or a tarball with the system's own tools, which refuse
/// entries that climb out of the folder.
fn unpack(archive: &Path, url: &str, into: &Path) -> Result<(), String> {
    use std::process::Command;
    let lower = url.to_ascii_lowercase();
    let run = |mut command: Command| {
        command
            .output()
            .map_err(|error| error.to_string())
            .and_then(|output| {
                if output.status.success() {
                    Ok(())
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
                }
            })
    };
    let tar = |flags: &str| {
        let mut command = Command::new("tar");
        command.arg(flags).arg(archive).arg("-C").arg(into);
        command
    };
    let result = if lower.ends_with(".zip") {
        let mut unzip = Command::new("unzip");
        unzip.arg("-oq").arg(archive).arg("-d").arg(into);
        if cfg!(windows) {
            run(tar("-xf"))
        } else {
            run(unzip).or_else(|_| run(tar("-xf")))
        }
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        run(tar("-xzf"))
    } else {
        run(tar("-xf"))
    };
    result.map_err(|error| format!("Could not unpack the agent: {error}"))
}

/// Puts a binary agent in place: downloaded, checked, unpacked into a
/// folder of its own, made runnable.
fn install_binary(root: &Path, agent: &AcpAgentDef) -> Result<(), String> {
    let Some(install) = &agent.binary else { return Ok(()) };
    let program = PathBuf::from(&agent.command);
    if program.is_file() {
        return Ok(());
    }
    let version = agent.version.clone().unwrap_or_default();
    let target = acp_registry::install_dir(root, &agent.id, &version);
    let parent = target.parent().ok_or("Bad install folder.")?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
    let staging = parent.join(format!(".unpacking-{}", std::process::id()));
    let archive = parent.join(format!(".download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let result = download_checked(&install.archive, &install.sha256, &archive)
        .and_then(|_| unpack(&archive, &install.archive, &staging));
    let _ = std::fs::remove_file(&archive);
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let unpacked = staging.join(&install.cmd);
    let inside = unpacked
        .canonicalize()
        .ok()
        .zip(staging.canonicalize().ok())
        .is_some_and(|(file, folder)| file.starts_with(folder) && file.is_file());
    if !inside {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("The download has no {} in it.", install.cmd));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&unpacked, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(|error| error.to_string())?;
    // Older versions of it are no longer needed.
    if let Ok(entries) = std::fs::read_dir(&parent) {
        for entry in entries.flatten() {
            if entry.path() != target {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(())
}

/// Fetches an `npx` or `uvx` package ahead of its first chat, so that chat
/// does not wait on the download. Its stdin is closed, so an ACP agent ends
/// as soon as it has loaded; one that does not is stopped after a while.
fn warm(agent: &AcpAgentDef) -> Result<(), String> {
    let tool = agent.command.as_str();
    let found = installed(&[tool.to_string()]).get(tool).copied().unwrap_or(false);
    if !found {
        return Err(match tool {
            "uvx" => "This agent needs uv. Install it from docs.astral.sh/uv, then add the agent again.".into(),
            _ => "This agent needs Node.js. Install it from nodejs.org, then add the agent again.".into(),
        });
    }
    let line: Vec<String> = std::iter::once(agent.command.clone())
        .chain(agent.args.iter().cloned())
        .map(|arg| convoy_core::provider::launch::quote(&arg))
        .collect();
    let (shell, args) = convoy_core::provider::launch::shell_command(format!("exec {}", line.join(" ")));
    let mut child = std::process::Command::new(shell)
        .args(args)
        .envs(agent.env.clone())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// Adds a registry agent, installing it first: a binary is downloaded and
/// checked, a package fetched. Adding one already there updates it.
#[tauri::command]
pub async fn acp_registry_add(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    let root = workspace.storage.acp_agents();
    let shipped = acp_agents::builtins();
    let definition = tauri::async_runtime::spawn_blocking({
        let root = root.clone();
        move || -> Result<AcpAgentDef, String> {
            let text = registry_document(&root, false)?;
            let agents = acp_registry::parse(&text, acp_registry::platform()).map_err(|error| error.to_string())?;
            let agent = agents
                .iter()
                .find(|agent| agent.id == id)
                .ok_or("The registry no longer lists this agent.")?;
            let mut definition = acp_registry::definition(agent, &root)
                .ok_or_else(|| format!("{} has no build for this computer.", agent.name))?;
            // A shipped agent's own terminal UI and sign-in carry over.
            if let Some(known) = shipped.iter().find(|known| known.registry.as_deref() == Some(id.as_str())) {
                definition.login_command = known.login_command.clone();
                if known.has_terminal() {
                    let launch_args = match &agent.launch {
                        Some(Launch::Npx { args, .. } | Launch::Uvx { args, .. } | Launch::Binary { args, .. }) => args.len(),
                        None => 0,
                    };
                    let keep = definition.args.len().saturating_sub(launch_args);
                    definition.tui_command = Some(definition.command.clone());
                    definition.tui_args = definition.args[..keep].to_vec();
                    definition.tui_prompt_flag = known.tui_prompt_flag.clone();
                }
            }
            match definition.binary {
                Some(_) => install_binary(&root, &definition)?,
                None => warm(&definition)?,
            }
            Ok(definition)
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    workspace.act(|core| {
        let mut agents = core.settings().acp_agents.clone();
        agents.retain(|agent| agent.id != definition.id);
        agents.push(definition);
        core.save_settings(convoy_core::workspace::SettingsPatch {
            acp_agents: Some(agents),
            ..Default::default()
        })
        .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against the real registry and a real download; run by hand with
    /// `cargo test real_registry -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_registry_installs_binary_agents() {
        let root = tempfile::tempdir().unwrap();
        let text = registry_document(root.path(), true).unwrap();
        let agents = acp_registry::parse(&text, acp_registry::platform()).unwrap();
        println!("{} agents", agents.len());
        fetch_icons(root.path(), &agents);
        assert!(icon_data(root.path(), "gemini").is_some(), "icons are fetched");
        for id in ["opencode", "antigravity-acp"] {
            let agent = agents.iter().find(|agent| agent.id == id).unwrap();
            let definition = acp_registry::definition(agent, root.path()).unwrap();
            install_binary(root.path(), &definition).unwrap();
            assert!(Path::new(&definition.command).is_file(), "{}", definition.command);
            println!("{id}: {}", definition.command);
        }
    }
}
