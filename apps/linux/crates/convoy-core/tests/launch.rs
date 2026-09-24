//! Ported from `test/core.test.cjs` and `test/provider.test.cjs` — how a
//! provider CLI is actually invoked. The Windows cases are dropped: this build
//! targets Linux and the Electron preview remains the Windows implementation.

mod common;

use convoy_core::provider::launch::{agent_args, agent_environment, launch_spec, Bindings};
use convoy_core::provider::session_spec;
use convoy_core::workspace::model::{Agent, Session};
use std::collections::BTreeMap;
use std::process::Command;

fn session(agent: Agent, provider_id: &str, prompt: &str) -> Session {
    let mut session = Session::new("project", agent, "Session");
    session.provider_id = provider_id.into();
    session.prompt = prompt.into();
    session
}

#[test]
fn resume_uses_exact_provider_identity_and_never_repeats_the_initial_prompt() {
    let claude = session(Agent::Claude, "123", "Do something");
    assert_eq!(agent_args(&claude, true), ["claude", "--resume", "123"]);

    let anonymous = session(Agent::Codex, "", "Do something");
    assert_eq!(
        agent_args(&anonymous, true),
        ["codex", "resume", "--no-alt-screen"]
    );

    let known = session(Agent::Codex, "123", "Do something");
    assert_eq!(
        agent_args(&known, true),
        ["codex", "resume", "123", "--no-alt-screen"]
    );

    // A first launch does carry the prompt, as a literal positional argument.
    assert_eq!(
        agent_args(&claude, false),
        ["claude", "--session-id", "123", "--", "Do something"]
    );
}

#[test]
fn posix_launch_preserves_shell_metacharacters_as_literal_arguments() {
    let prompt = "quotes ' \" `echo injected` $(echo injected) ; & | %PATH%\nПривет";
    let session = session(Agent::Claude, "123", prompt);
    let spec = launch_spec(&session, false, &Bindings::default());
    assert_eq!(spec.file, "/bin/bash");

    // Replace the exec with a printf that reports the argument vector the
    // shell actually built.
    let script = spec.args[1].replacen("exec ", "printf '%s\\0' ", 1);
    let output = Command::new("/bin/bash")
        .args(["-c", &script])
        .output()
        .expect("bash");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut parts: Vec<&str> = stdout.split('\0').collect();
    parts.pop();
    assert_eq!(parts, agent_args(&session, false));
}

#[test]
fn account_bindings_survive_the_login_shell() {
    let session = session(Agent::Codex, "", "");
    let bindings = Bindings {
        codex_home: Some("/tmp/account dir".into()),
        ..Bindings::default()
    };
    let spec = launch_spec(&session, true, &bindings);
    assert!(spec.args[1].starts_with("exec env "));
    assert!(spec.args[1].contains("'CODEX_HOME=/tmp/account dir'"));
}

#[test]
fn agent_environment_removes_parent_conversation_markers_without_losing_login_configuration() {
    let source: BTreeMap<String, String> = [
        ("PATH", "/bin"),
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_SESSION_ID", "parent"),
        ("CLAUDE_CONFIG_DIR", "/account"),
        ("CODEX_HOME", "/codex"),
        ("CODEX_THREAD_ID", "parent"),
    ]
    .iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect();

    let env = agent_environment(&source);
    assert_eq!(env.get("CLAUDECODE"), None);
    assert_eq!(env.get("CLAUDE_CODE_SESSION_ID"), None);
    assert_eq!(env.get("CODEX_THREAD_ID"), None);
    assert_eq!(env.get("CLAUDE_CONFIG_DIR").unwrap(), "/account");
    assert_eq!(env.get("CODEX_HOME").unwrap(), "/codex");
    assert_eq!(env.get("TERM").unwrap(), "xterm-256color");
    assert_eq!(
        source.get("CLAUDECODE").unwrap(),
        "1",
        "the source map is not mutated"
    );
}

#[test]
fn enhanced_launch_passes_model_and_settings_before_a_literal_prompt() {
    let mut session = session(Agent::Claude, "abc", "$(touch never)");
    session.model = Some("sonnet".into());
    let env: BTreeMap<String, String> =
        [("CLAUDE_CONFIG_DIR".to_string(), "/tmp/account".to_string())]
            .into_iter()
            .collect();

    let spec = session_spec(&session, Some("/tmp/settings file.json"), &env);
    let script = &spec.args[1];
    let model = script.find("--model").expect("model flag");
    let settings = script.find("--settings").expect("settings flag");
    let separator = script.find("'--'").expect("prompt separator");
    let prompt = script.find("touch never").expect("prompt");
    assert!(model < settings && settings < separator && separator < prompt);
    assert!(script.contains("'/tmp/settings file.json'"));

    // Resume never replays the first message.
    let mut resumed = session.clone();
    resumed.started = true;
    let resumed = session_spec(&resumed, None, &BTreeMap::new());
    assert!(!resumed.args[1].contains("touch never"));
    assert!(resumed.args[1].contains("--resume"));
}
