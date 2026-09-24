//! The Windows half of launching an agent.
//!
//! Two of these were dropped when the GTK port narrowed to Linux; they are
//! back because the Tauri client targets Windows again. All of them run on
//! Linux, because the platform is a parameter rather than a `cfg!` — which is
//! the only reason the Electron suite could test this path at all.

mod common;

use convoy_core::model::{Agent, Session};
use convoy_core::provider::launch::windows_executable;
use convoy_core::provider::{provider_spec_for, session_spec_for};
use convoy_core::{Platform, ProcessRunner, ProcessSpec, StdRunner};
use std::collections::BTreeMap;
use std::path::Path;

fn env(path: &str) -> BTreeMap<String, String> {
    [("Path".to_string(), path.to_string())]
        .into_iter()
        .collect()
}

fn session(agent: Agent, prompt: &str) -> Session {
    let mut session = Session::new("project", agent, "Session");
    session.provider_id = "11111111-1111-4111-8111-111111111111".into();
    session.prompt = prompt.into();
    session
}

#[test]
fn a_native_executable_on_path_is_preferred() {
    let directory = tempfile::Builder::new()
        .prefix("convoy-win-")
        .tempdir()
        .unwrap();
    let executable = directory.path().join("claude.exe");
    std::fs::write(&executable, "").unwrap();

    let resolved = windows_executable(Agent::Claude, &env(&directory.path().to_string_lossy()))
        .expect("resolve");
    assert_eq!(resolved.file, executable.to_string_lossy());
    assert!(
        resolved.prefix.is_empty(),
        "a native exe needs no interpreter"
    );
}

#[test]
fn a_missing_agent_says_what_to_install() {
    let directory = tempfile::Builder::new()
        .prefix("convoy-win-")
        .tempdir()
        .unwrap();
    let error = windows_executable(Agent::Codex, &env(&directory.path().to_string_lossy()))
        .expect_err("nothing is installed");
    assert_eq!(
        error.to_string(),
        "codex was not found. Install the native CLI or its standard npm package and restart Convoy."
    );
}

/// Restored from the Electron suite: the npm entry script is run directly, and
/// the argument vector survives verbatim. A `.cmd` shim would not preserve it,
/// which is why the resolver exists at all.
#[test]
fn the_npm_resolver_runs_the_script_with_a_literal_argument_vector() {
    let Some(node) = which("node") else {
        // Without a Node there is nothing to run the entry script with.
        return;
    };
    let root = tempfile::Builder::new()
        .prefix("convoy-resolver-")
        .tempdir()
        .unwrap();
    let package = root.path().join("node_modules/@openai/codex/bin");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("codex.js"),
        "process.stdout.write(JSON.stringify(process.argv.slice(2)))",
    )
    .unwrap();

    let path = format!(
        "{};{}",
        root.path().to_string_lossy(),
        Path::new(&node).parent().unwrap().to_string_lossy()
    );
    let arguments: Vec<String> = vec![
        "--model".into(),
        "gpt-example".into(),
        "--".into(),
        "spaces \"quotes\" & %VAR% $(command) `literal`\nsecond line".into(),
    ];

    let spec = provider_spec_for(Agent::Codex, &arguments, &env(&path), Platform::Windows)
        .expect("resolve");
    assert!(
        spec.file.ends_with("node"),
        "node runs the script: {}",
        spec.file
    );
    assert!(spec.args[0].ends_with("codex.js"), "{:?}", spec.args);

    let output = StdRunner
        .run(&ProcessSpec::new(spec.file, spec.args))
        .expect("run");
    assert!(output.ok(), "{}", output.stderr);
    let seen: Vec<String> = serde_json::from_str(&output.stdout).expect("argv");
    assert_eq!(
        seen, arguments,
        "every argument reached the script unchanged"
    );
}

/// Restored from the Electron suite: flags go before the positional separator,
/// and a resumed session never replays its first message.
#[test]
fn windows_launches_put_flags_before_the_prompt_and_never_replay_it() {
    let directory = tempfile::Builder::new()
        .prefix("convoy-win-")
        .tempdir()
        .unwrap();
    std::fs::write(directory.path().join("claude.exe"), "").unwrap();
    let path = directory.path().to_string_lossy().into_owned();

    let mut fresh = session(Agent::Claude, "$(touch never)");
    fresh.model = Some("sonnet".into());
    let spec = session_spec_for(
        &fresh,
        Some("C:\\settings file.json"),
        &env(&path),
        Platform::Windows,
    )
    .expect("spec");

    let position = |needle: &str| spec.args.iter().position(|arg| arg == needle);
    assert!(position("--model") < position("--"), "{:?}", spec.args);
    assert!(position("--settings") < position("--"), "{:?}", spec.args);
    assert_eq!(
        spec.args.last().map(String::as_str),
        Some("$(touch never)"),
        "the prompt is one literal argument, not shell input"
    );
    assert!(
        spec.args.contains(&"C:\\settings file.json".to_string()),
        "a path with a space is not split: {:?}",
        spec.args
    );

    let mut resumed = fresh.clone();
    resumed.started = true;
    let spec = session_spec_for(&resumed, None, &env(&path), Platform::Windows).expect("spec");
    assert!(spec.args.contains(&"--resume".to_string()));
    assert!(
        !spec.args.iter().any(|arg| arg.contains("touch never")),
        "resume replayed the first message: {:?}",
        spec.args
    );
}

#[test]
fn a_unix_launch_still_goes_through_a_login_shell() {
    let spec = session_spec_for(
        &session(Agent::Claude, "hello"),
        None,
        &BTreeMap::new(),
        Platform::Unix,
    )
    .expect("spec");
    assert_eq!(spec.file, "/bin/bash");
    assert_eq!(spec.args[0], "-ilc");
}

#[test]
fn storage_lands_where_each_platform_keeps_application_data() {
    // Both resolve to what Electron's `app.getPath('appData')` gives, which is
    // what lets the three builds read one file.
    // Rooted by the rules of the platform named, not of the one running the
    // test: `Path::is_absolute` would answer the wrong question on either.
    let unix = convoy_core::storage::config_root_for(Platform::Unix);
    assert!(unix.to_string_lossy().starts_with('/'), "{unix:?}");

    let windows = convoy_core::storage::config_root_for(Platform::Windows);
    assert!(
        windows.ends_with("Roaming") || std::env::var_os("APPDATA").is_some(),
        "{windows:?}"
    );
    assert!(
        convoy_core::storage::rooted_anywhere(&windows),
        "{windows:?}"
    );
}

#[test]
fn the_status_line_is_quoted_for_the_shell_that_will_run_it() {
    let unix = convoy_core::telemetry::status_line(
        "/usr/bin/convoy",
        "/home/a b/telemetry/abc",
        Platform::Unix,
    );
    assert_eq!(unix, "'/usr/bin/convoy' --hook '/home/a b/telemetry/abc'");

    let windows = convoy_core::telemetry::status_line(
        "C:\\Program Files\\Convoy\\convoy.exe",
        "C:\\Users\\a b\\telemetry\\abc",
        Platform::Windows,
    );
    assert_eq!(
        windows,
        "\"C:\\Program Files\\Convoy\\convoy.exe\" --hook \"C:\\Users\\a b\\telemetry\\abc\""
    );
}

fn which(program: &str) -> Option<String> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(program))
            .find(|candidate| candidate.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    })
}
