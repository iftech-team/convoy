//! Chat launches and the chat/terminal view, through the public API.

mod common;

use common::{fixture, new_session};
use convoy_core::chat::{ChatEvent, Driver};
use convoy_core::session::plan_chat_launch;
use convoy_core::workspace::model::Agent;
use convoy_core::workspace::SettingsPatch;
use convoy_core::Storage;
use serde_json::Value;

/// A workspace whose launches touch nothing outside the fixture: no folder
/// is marked trusted in the user's own agent configuration.
fn prepared(agent: Agent, yolo: bool) -> (common::Fixture, convoy_core::Workspace, String) {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_settings(SettingsPatch {
            auto_trust: Some(false),
            yolo_codex: Some(yolo),
            yolo_claude: Some(yolo),
            ..Default::default()
        })
        .unwrap();
    let mut input = new_session(&project, agent, "Chat");
    input.prompt = "Fix the login redirect".into();
    input.model = "sonnet".into();
    workspace.add_session(input).unwrap();
    let id = workspace.state().sessions[0].id.clone();
    (fixture, workspace, id)
}

#[cfg(unix)]
#[test]
fn a_claude_chat_launch_keeps_the_terminal_launch_and_adds_json_pipes() {
    let (fixture, workspace, id) = prepared(Agent::Claude, true);
    let storage = Storage::new(fixture.path().join("state"));
    let plan = plan_chat_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    let session = workspace.session(&id).unwrap();

    // The login shell, running one command line.
    assert_eq!(plan.launch.spec.args[0], "-ilc");
    let command = &plan.launch.spec.args[1];
    assert!(command.starts_with("exec env "), "{command}");
    for part in [
        "'claude' '--input-format' 'stream-json' '--output-format' 'stream-json' '--verbose' '--include-partial-messages' '--permission-prompt-tool' 'stdio'",
        &format!("'--session-id' '{}'", session.provider_id),
        "'--model' 'sonnet'",
        "'--settings'",
        "'--dangerously-skip-permissions'",
    ] {
        assert!(command.contains(part), "{part} missing from {command}");
    }
    assert!(
        !command.contains("Fix the login"),
        "the prompt is a user turn, not an argument"
    );
    assert_eq!(plan.first_prompt.as_deref(), Some("Fix the login redirect"));
    assert!(!plan.launch.resume);
    assert!(matches!(plan.driver, Driver::Claude(_)));
}

#[cfg(unix)]
#[test]
fn a_codex_chat_launch_runs_the_app_server_and_opens_the_thread_with_the_choices() {
    let (fixture, workspace, id) = prepared(Agent::Codex, true);
    let storage = Storage::new(fixture.path().join("state"));
    let mut plan = plan_chat_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    assert!(plan.launch.spec.args[1].ends_with("'codex' 'app-server' '--listen' 'stdio://'"));

    let mut driver = plan.driver;
    driver.start();
    driver.feed(r#"{"id":1,"result":{}}"#);
    let sent: Vec<Value> = driver
        .take_outgoing()
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(sent[0]["method"], "initialized");
    assert_eq!(sent[1]["method"], "thread/start");
    assert_eq!(sent[1]["params"]["model"], "sonnet");
    assert_eq!(sent[1]["params"]["approvalPolicy"], "never");
    assert_eq!(
        sent[1]["params"]["cwd"],
        plan.launch.directory.to_string_lossy().as_ref()
    );

    // A turn typed before the thread opened goes out when it does, after the
    // echo and the working state are shown.
    let (lines, events) = driver.send(plan.first_prompt.take().unwrap().as_str(), &[]);
    assert!(lines.is_empty());
    assert!(matches!(&events[0], ChatEvent::User { text, .. } if text == "Fix the login redirect"));
    assert_eq!(events[1], ChatEvent::TurnStarted);
    let events = driver.feed(r#"{"id":11,"result":{"thread":{"id":"t-1"}}}"#);
    assert_eq!(
        events,
        [ChatEvent::Session {
            provider_id: "t-1".into()
        }]
    );
    let turn: Value = serde_json::from_str(&driver.take_outgoing()[0]).unwrap();
    assert_eq!(turn["method"], "turn/start");
    assert_eq!(turn["params"]["threadId"], "t-1");
}

/// A manager is launched with its variables and instructions, in a terminal
/// and in a chat, for either agent; any other session gets neither, and a
/// helper is never a manager however it is asked.
#[cfg(unix)]
#[test]
fn a_manager_launch_carries_its_secret_and_its_instructions() {
    use convoy_core::helpers::{ManagerLaunch, ENV_BIN, ENV_SESSION, ENV_TOKEN};
    use convoy_core::session::{plan_chat_launch_with, plan_launch_with};

    let launch = ManagerLaunch {
        token: "secret-token".into(),
        bin: "/opt/convoy/convoy".into(),
        requests: "/tmp/requests".into(),
    };
    for agent in [Agent::Claude, Agent::Codex] {
        let (fixture, mut workspace, id) = prepared(agent, false);
        let storage = Storage::new(fixture.path().join("state"));
        let exe = std::path::Path::new("/bin/convoy");

        // Not a manager: the launch is told nothing, even when offered.
        let plain = plan_launch_with(&workspace, &storage, &id, exe, Some(&launch)).unwrap();
        assert!(!plain.env.contains_key(ENV_TOKEN));
        assert!(plain.instructions.is_none());
        assert!(!plain.spec.args[1].contains("--session spawn"));

        workspace.set_session_manager(&id, true).unwrap();
        let terminal = plan_launch_with(&workspace, &storage, &id, exe, Some(&launch)).unwrap();
        assert_eq!(terminal.env[ENV_SESSION], id);
        assert_eq!(terminal.env[ENV_TOKEN], "secret-token");
        assert_eq!(terminal.env[ENV_BIN], "/opt/convoy/convoy");
        let command = &terminal.spec.args[1];
        match agent {
            Agent::Claude => assert!(
                command.contains("'--append-system-prompt' 'You are a manager session"),
                "{command}"
            ),
            Agent::Codex => assert!(
                command.contains("'-c' 'developer_instructions=\"You are a manager session"),
                "{command}"
            ),
            Agent::Gemini => unreachable!("only Claude and Codex are looped over"),
        }
        assert!(command.contains("--session spawn"));
        // The flags come before the prompt, which stays the last argument.
        assert!(
            command
                .trim_end()
                .ends_with("'--' 'Fix the login redirect'"),
            "{command}"
        );
        // The secret travels in the environment, never on the command line.
        assert!(!command.contains("secret-token"));

        // Without a launch, a manager's plan carries nothing of it.
        let bare = plan_launch_with(&workspace, &storage, &id, exe, None).unwrap();
        assert!(!bare.env.contains_key(ENV_SESSION));

        let mut chat =
            plan_chat_launch_with(&workspace, &storage, &id, exe, Some(&launch)).unwrap();
        assert_eq!(chat.launch.env[ENV_TOKEN], "secret-token");
        match agent {
            Agent::Claude => assert!(chat.launch.spec.args[1].contains("'--append-system-prompt'")),
            Agent::Codex => {
                chat.driver.start();
                chat.driver.feed(r#"{"id":1,"result":{}}"#);
                let sent: Vec<Value> = chat
                    .driver
                    .take_outgoing()
                    .iter()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(sent[1]["method"], "thread/start");
                assert!(sent[1]["params"]["developerInstructions"]
                    .as_str()
                    .unwrap()
                    .contains("--session wait"));
            }
            Agent::Gemini => unreachable!("only Claude and Codex are looped over"),
        }
    }
}

#[test]
fn the_view_defaults_to_terminal_and_each_session_can_choose() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .add_session(new_session(&project, Agent::Claude, "One"))
        .unwrap();
    let id = workspace.state().sessions[0].id.clone();
    let settings = workspace.settings().clone();
    assert_eq!(settings.default_view, "terminal");
    assert_eq!(
        workspace.session(&id).unwrap().shown_as(&settings),
        "terminal"
    );

    workspace.set_session_view(&id, "chat").unwrap();
    assert!(workspace.set_session_view(&id, "split").is_err());
    workspace
        .save_settings(SettingsPatch {
            default_view: Some("chat".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(workspace
        .save_settings(SettingsPatch {
            default_view: Some("grid".into()),
            ..Default::default()
        })
        .is_err());

    let reloaded = fixture.workspace();
    assert_eq!(reloaded.session(&id).unwrap().view.as_deref(), Some("chat"));
    assert_eq!(reloaded.settings().default_view, "chat");
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.file).unwrap()).unwrap();
    assert_eq!(raw["settings"]["defaultView"], "chat");
    assert_eq!(raw["sessions"][0]["view"], "chat");
}

/// The macOS app shares the file: a session without the key, and keys this
/// build does not know, survive a save untouched.
#[test]
fn a_file_without_views_loads_and_unknown_keys_survive() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .add_session(new_session(&project, Agent::Codex, "One"))
        .unwrap();
    drop(workspace);
    let mut raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.file).unwrap()).unwrap();
    raw["sessions"][0]["macOnly"] = Value::from("kept");
    raw["settings"]
        .as_object_mut()
        .unwrap()
        .remove("defaultView");
    std::fs::write(&fixture.file, raw.to_string()).unwrap();

    let mut workspace = fixture.workspace();
    assert_eq!(workspace.settings().default_view, "terminal");
    let id = workspace.state().sessions[0].id.clone();
    assert_eq!(workspace.session(&id).unwrap().view, None);
    workspace.set_session_view(&id, "chat").unwrap();
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.file).unwrap()).unwrap();
    assert_eq!(raw["sessions"][0]["macOnly"], "kept");
    assert_eq!(raw["sessions"][0]["view"], "chat");
}

/// Model and effort are the session's, changed while it runs, kept in the
/// shared file, and passed to both the chat and the terminal launch.
#[cfg(unix)]
#[test]
fn model_and_effort_are_kept_and_reach_both_launches() {
    let (fixture, mut workspace, id) = prepared(Agent::Claude, false);
    let storage = Storage::new(fixture.path().join("state"));
    workspace.set_session_model(&id, Some("opus")).unwrap();
    workspace.set_session_effort(&id, Some("xhigh")).unwrap();
    assert!(
        workspace.set_session_effort(&id, Some("ultra")).is_err(),
        "Claude has no ultra"
    );
    assert!(workspace.set_session_effort(&id, Some("x; rm")).is_err());
    assert!(workspace.set_session_model(&id, Some("bad model")).is_err());

    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.file).unwrap()).unwrap();
    assert_eq!(raw["sessions"][0]["model"], "opus");
    assert_eq!(raw["sessions"][0]["effort"], "xhigh");

    let plan = plan_chat_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    let command = &plan.launch.spec.args[1];
    assert!(command.contains("'--model' 'opus'"), "{command}");
    assert!(command.contains("'--effort' 'xhigh'"), "{command}");
    let terminal = convoy_core::session::plan_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    let command = terminal.spec.args.join(" ");
    assert!(command.contains("'--effort' 'xhigh'"), "{command}");

    // None goes back to the agent's default, and the key leaves the file.
    workspace.set_session_effort(&id, None).unwrap();
    workspace.set_session_model(&id, Some("")).unwrap();
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture.file).unwrap()).unwrap();
    assert!(raw["sessions"][0].get("effort").is_none());
    assert!(raw["sessions"][0].get("model").is_none());

    // Codex takes its own levels, in its own form, and each turn asks.
    let (fixture, mut workspace, id) = prepared(Agent::Codex, false);
    let storage = Storage::new(fixture.path().join("state"));
    workspace.set_session_effort(&id, Some("ultra")).unwrap();
    let terminal = convoy_core::session::plan_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    assert!(terminal
        .spec
        .args
        .join(" ")
        .contains("'-c' 'model_reasoning_effort=ultra'"));
    let mut plan = plan_chat_launch(
        &workspace,
        &storage,
        &id,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    plan.driver.start();
    plan.driver.feed(r#"{"id":1,"result":{}}"#);
    plan.driver
        .feed(r#"{"id":11,"result":{"thread":{"id":"t"}}}"#);
    plan.driver.take_outgoing();
    let (lines, _) = plan.driver.send("go", &[]);
    let turn: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(turn["params"]["effort"], "ultra");
}

/// Talks to the real agents through the real launch: the login shell, the
/// chat flags, the protocol driver. Not run by default — it needs `claude`
/// and `codex` installed and signed in, and it spends a little of each.
///     cargo test -p convoy-core --test chat real_agents -- --ignored --nocapture
#[cfg(unix)]
#[test]
#[ignore = "talks to the real Claude Code and Codex"]
fn real_agents_chat_approve_and_finish_a_turn() {
    use convoy_core::chat::event::Decision;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    for agent in [Agent::Claude, Agent::Codex] {
        let fixture = fixture();
        let (mut workspace, project) = fixture.with_project();
        workspace
            .save_settings(SettingsPatch {
                auto_trust: Some(false),
                ..Default::default()
            })
            .unwrap();
        let mut input = new_session(&project, agent, "Smoke");
        input.prompt =
            "Create a file named smoke.txt containing the word ok. Then reply with: done".into();
        workspace.add_session(input).unwrap();
        let id = workspace.state().sessions[0].id.clone();
        let storage = Storage::new(fixture.path().join("state"));
        // Hooks would call this binary; `true` keeps them quiet.
        let mut plan = plan_chat_launch(
            &workspace,
            &storage,
            &id,
            std::path::Path::new("/usr/bin/true"),
        )
        .unwrap();

        let mut env = convoy_core::provider::launch::agent_environment(
            &convoy_core::provider::launch::current_environment(),
        );
        env.extend(plan.launch.env.clone());
        let mut child = Command::new(&plan.launch.spec.file)
            .args(&plan.launch.spec.args)
            .current_dir(&plan.launch.directory)
            .env_clear()
            .envs(&env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let (lines, received) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        let mut write = |lines: Vec<String>| {
            for line in lines {
                writeln!(stdin, "{line}").unwrap();
            }
        };

        write(plan.driver.start());
        let (out, _) = plan.driver.send(plan.first_prompt.as_deref().unwrap(), &[]);
        write(out);
        let started = Instant::now();
        let (mut text, mut approvals, mut ended) = (String::new(), 0, None);
        let mut listed: Option<Vec<convoy_core::chat::ChatModel>> = None;
        while ended.is_none() && started.elapsed() < Duration::from_secs(180) {
            let Ok(line) = received.recv_timeout(Duration::from_secs(5)) else {
                continue;
            };
            let events = plan.driver.feed(&line);
            write(plan.driver.take_outgoing());
            for event in events {
                match event {
                    ChatEvent::Text { delta, .. } => text.push_str(&delta),
                    ChatEvent::TextDone { text: full, .. } => text = full,
                    ChatEvent::Approval { request, .. } => {
                        approvals += 1;
                        let (line, _) = plan.driver.answer(&request, Decision::Allow).unwrap();
                        write(vec![line]);
                    }
                    ChatEvent::TurnEnded { error, usage } => ended = Some((error, usage)),
                    ChatEvent::Models { models, .. } => listed = Some(models),
                    _ => {}
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let (error, usage) = ended.unwrap_or_else(|| panic!("{agent:?}: the turn never ended"));
        let models = listed.unwrap_or_else(|| panic!("{agent:?}: no models were listed"));
        for model in &models {
            println!(
                "{agent:?} model {:?} ({:?}){} efforts {:?} default {:?}",
                model.id,
                model.name,
                if model.is_default { " [default]" } else { "" },
                model.efforts,
                model.default_effort
            );
        }
        assert!(!models.is_empty(), "{agent:?}");
        assert!(
            models.iter().any(|model| !model.efforts.is_empty()),
            "{agent:?}: no model takes an effort level"
        );
        println!("{agent:?}: approvals {approvals}, usage {usage:?}, reply {text:?}");
        assert_eq!(error, None, "{agent:?}");
        assert!(
            text.to_lowercase().contains("done"),
            "{agent:?} replied {text:?}"
        );
        let written =
            std::fs::read_to_string(plan.launch.directory.join("smoke.txt")).unwrap_or_default();
        assert!(written.contains("ok"), "{agent:?} wrote {written:?}");
    }
}

/// A saved Peshbar connection reaches every launch as an MCP server: Claude
/// through an owner-only `--mcp-config` file, Codex through `-c` overrides
/// and an environment variable. The token is never on the command line.
#[cfg(unix)]
#[test]
fn a_peshbar_connection_hands_every_launch_its_mcp_server() {
    use convoy_core::integrations::{Connection, Integrations, TrackerAuth, TrackerKind};
    use convoy_core::session::plan_launch;

    for agent in [Agent::Claude, Agent::Codex] {
        let (fixture, workspace, id) = prepared(agent, false);
        let storage = Storage::new(fixture.path().join("state"));
        let exe = std::path::Path::new("/bin/convoy");

        // Nothing saved: nothing added.
        let bare = plan_launch(&workspace, &storage, &id, exe).unwrap();
        assert!(bare.mcp_flags.is_empty());
        assert!(!bare.spec.args[1].contains("mcp"), "{}", bare.spec.args[1]);

        let mut integrations = Integrations::load(storage.integrations()).unwrap();
        integrations
            .save(Connection {
                id: String::new(),
                kind: TrackerKind::Peshbar,
                auth: TrackerAuth::ApiKey,
                name: "Peshbar".into(),
                site: None,
                username: None,
                secret: "7|token-secret".into(),
            })
            .unwrap();

        let terminal = plan_launch(&workspace, &storage, &id, exe).unwrap();
        let chat = plan_chat_launch(&workspace, &storage, &id, exe).unwrap();
        for command in [&terminal.spec.args[1], &chat.launch.spec.args[1]] {
            assert!(!command.contains("token-secret"), "{command}");
        }
        match agent {
            Agent::Claude => {
                let file = storage.telemetry().join(format!("{id}.mcp.json"));
                let flag = format!("'--mcp-config' '{}'", file.to_string_lossy());
                assert!(
                    terminal.spec.args[1].contains(&flag),
                    "{}",
                    terminal.spec.args[1]
                );
                assert!(
                    chat.launch.spec.args[1].contains(&flag),
                    "{}",
                    chat.launch.spec.args[1]
                );
                let config: Value =
                    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
                assert_eq!(
                    config["mcpServers"]["peshbar"]["url"],
                    "https://management-api.peshbar.com/mcp"
                );
            }
            Agent::Codex => {
                let url =
                    "'-c' 'mcp_servers.peshbar.url=\"https://management-api.peshbar.com/mcp\"'";
                assert!(
                    terminal.spec.args[1].contains(url),
                    "{}",
                    terminal.spec.args[1]
                );
                assert!(
                    chat.launch.spec.args[1].contains(&format!("{url} '-c' 'mcp_servers.peshbar.bearer_token_env_var=\"CONVOY_PESHBAR_TOKEN\"' 'app-server'")),
                    "{}",
                    chat.launch.spec.args[1]
                );
                for env in [&terminal.env, &chat.launch.env] {
                    assert_eq!(
                        env.get("CONVOY_PESHBAR_TOKEN").map(String::as_str),
                        Some("7|token-secret")
                    );
                }
            }
            Agent::Gemini => unreachable!("only Claude and Codex are looped over"),
        }
    }
}
