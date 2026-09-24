//! OS boundaries shared by the desktop clients.
use crate::provider::launch::LaunchSpec;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Unix files are owner-only. Windows files inherit the user's profile ACL.
pub fn private_file_options() -> std::fs::OpenOptions {
    #[allow(unused_mut)]
    let mut options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub fn symlink(target: &Path, destination: &Path, directory: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = directory;
        std::os::unix::fs::symlink(target, destination)
    }
    #[cfg(windows)]
    {
        if directory {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        }
    }
}

pub fn setup_shell(command: &str) -> LaunchSpec {
    if cfg!(windows) {
        LaunchSpec {
            file: "powershell.exe".into(),
            args: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-Command".into(),
                command.into(),
            ],
        }
    } else {
        LaunchSpec {
            file: "/bin/bash".into(),
            args: vec!["-lc".into(), command.into()],
        }
    }
}

/// Resolve a native executable or a standard npm entry point. Never pass an
/// agent's prompt through cmd.exe, PowerShell or an npm .cmd shim: those can
/// reinterpret quotes, newlines and shell metacharacters.
pub fn windows_provider(
    agent: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> LaunchSpec {
    let path = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    let directories: Vec<PathBuf> = path
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|p| PathBuf::from(p.trim_matches('"')))
        .filter(|p| p.is_absolute())
        .collect();
    windows_provider_in(agent, args, &directories)
}

fn windows_provider_in(agent: &str, args: &[String], directories: &[PathBuf]) -> LaunchSpec {
    let module = if agent == "claude" {
        "@anthropic-ai/claude-code/cli.js"
    } else {
        "@openai/codex/bin/codex.js"
    };
    let node = directories
        .iter()
        .map(|dir| dir.join("node.exe"))
        .find(|p| p.is_file());
    for directory in directories {
        let executable = directory.join(format!("{agent}.exe"));
        if executable.is_file() {
            return LaunchSpec {
                file: executable.to_string_lossy().into_owned(),
                args: args.to_vec(),
            };
        }
        if let Some(node) = &node {
            for root in [
                directory.join("node_modules"),
                directory.parent().unwrap_or(directory).to_path_buf(),
            ] {
                let script = root.join(module);
                if script.is_file() {
                    return LaunchSpec {
                        file: node.to_string_lossy().into_owned(),
                        args: std::iter::once(script.to_string_lossy().into_owned())
                            .chain(args.iter().cloned())
                            .collect(),
                    };
                }
            }
        }
    }
    // Spawn reports the missing native CLI; a .cmd shim is deliberately never
    // used as a fallback. Install the native CLI or Node + the standard package.
    LaunchSpec {
        file: format!("{agent}.exe"),
        args: args.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_launch_preserves_prompts_and_resolves_npm_without_a_shell() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("Program Files/node");
        let npm = temp.path().join("npm");
        let script = npm.join("node_modules/@openai/codex/bin/codex.js");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("node.exe"), "").unwrap();
        std::fs::write(&script, "").unwrap();
        let args = vec![
            "--".into(),
            "quotes \" ' & %PATH% $(whoami)\nnext line".into(),
        ];
        let spec = windows_provider_in("codex", &args, &[npm.clone(), bin.clone()]);
        assert_eq!(Path::new(&spec.file), bin.join("node.exe"));
        assert_eq!(Path::new(&spec.args[0]), script);
        assert_eq!(&spec.args[1..], args);
        std::fs::write(npm.join("codex.exe"), "").unwrap();
        let native = windows_provider_in("codex", &args, &[npm.clone(), bin]);
        assert_eq!(Path::new(&native.file), npm.join("codex.exe"));
        assert_eq!(native.args, args);
    }
}
