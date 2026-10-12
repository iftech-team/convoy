use super::cli::{self, Parsed, Timing};
use super::*;
use crate::chat::Usage;
use crate::workspace::model::Agent;
use std::time::{Duration, SystemTime};

fn workspace() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::load(dir.path().join("workspace.json")).unwrap();
    workspace.add_project(dir.path()).unwrap();
    let project = workspace.state().projects[0].id.clone();
    let mut input = NewSession::new(project.clone(), Agent::Claude, "Lead");
    input.prompt = "Plan the work".into();
    workspace.add_session(input).unwrap();
    let id = workspace.state().sessions[0].id.clone();
    workspace.set_session_manager(&id, true).unwrap();
    (dir, workspace, id)
}

fn spawn(agent: &str, prompt: &str) -> Spawn {
    Spawn {
        agent: agent.into(),
        prompt: prompt.into(),
        ..Default::default()
    }
}

#[test]
fn requests_and_results_round_trip_through_their_files() {
    let dir = tempfile::tempdir().unwrap();
    let requests = dir.path().join("requests");
    let request = Request::new(
        "s1",
        "secret",
        Command::Spawn(Spawn {
            agent: "codex".into(),
            model: Some("gpt-5".into()),
            worktree_auto: true,
            prompt: "Write tests".into(),
            ..Default::default()
        }),
    );
    let path = write_request(&requests, &request).unwrap();
    assert_eq!(path, request_path(&requests, &request.id));
    assert_eq!(pending(&requests), vec![path.clone()]);
    let text = fs::read_to_string(&path).unwrap();
    let raw: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(raw["command"]["type"], "spawn");
    assert_eq!(raw["command"]["worktree_auto"], true);
    assert!(raw["command"].get("title").is_none(), "absent stays absent");
    assert_eq!(read_request(&path).unwrap(), request);

    for command in [
        Command::Status {
            ids: vec!["a".into(), "b".into()],
        },
        Command::Output { id: "a".into() },
        Command::List,
    ] {
        let encoded = serde_json::to_string(&command).unwrap();
        assert_eq!(
            serde_json::from_str::<Command>(&encoded).unwrap(),
            command,
            "{encoded}"
        );
    }

    write_result(&requests, &request.id, &Response::value(json_id("child"))).unwrap();
    // A result is not a request.
    assert_eq!(pending(&requests), vec![path]);
    let response = take_result(&requests, &request.id).unwrap();
    assert!(response.ok);
    assert_eq!(response.value["id"], "child");
    assert!(take_result(&requests, &request.id).is_none(), "taken once");

    let failed = Response::from_result::<()>(Err(ConvoyError::message("No.")));
    let encoded: Response = serde_json::from_str(&serde_json::to_string(&failed).unwrap()).unwrap();
    assert!(!encoded.ok);
    assert_eq!(encoded.error.as_deref(), Some("No."));
}

fn json_id(id: &str) -> Value {
    serde_json::json!({ "id": id })
}

#[test]
fn a_request_from_another_version_or_with_a_bad_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut request = Request::new("s", "t", Command::List);
    request.id = "../escape".into();
    assert!(write_request(dir.path(), &request).is_err());
    assert!(write_result(dir.path(), "../escape", &Response::error("x")).is_err());

    let request = Request::new("s", "t", Command::List);
    let path = write_request(dir.path(), &request).unwrap();
    let mut raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    raw["version"] = serde_json::json!(99);
    fs::write(&path, raw.to_string()).unwrap();
    assert!(read_request(&path)
        .unwrap_err()
        .to_string()
        .contains("version"));
    fs::write(&path, "{not json").unwrap();
    assert!(read_request(&path).is_err());
}

#[test]
fn only_the_secret_a_manager_was_launched_with_gets_through() {
    let (_dir, mut workspace, id) = workspace();
    let mut tokens = Tokens::default();
    let token = tokens.issue(&id);
    assert_eq!(token.len(), 64);
    assert_ne!(token, tokens.issue("other"), "each launch has its own");

    let ok = Request::new(&id, &token, Command::List);
    assert_eq!(authorize(&workspace, &tokens, &ok).unwrap().id, id);

    let wrong = Request::new(&id, &new_token(), Command::List);
    assert!(authorize(&workspace, &tokens, &wrong).is_err());
    let empty = Request::new(&id, "", Command::List);
    assert!(authorize(&workspace, &tokens, &empty).is_err());
    let someone_else = Request::new("other", &token, Command::List);
    assert!(authorize(&workspace, &tokens, &someone_else).is_err());

    assert!(tokens_match("abc", "abc"));
    assert!(!tokens_match("abc", "abd"));
    assert!(!tokens_match("abc", "ab"));
    assert!(!tokens_match("", ""));

    // No longer a manager: the secret alone is not enough.
    workspace.set_session_manager(&id, false).unwrap();
    assert!(authorize(&workspace, &tokens, &ok).is_err());
    workspace.set_session_manager(&id, true).unwrap();
    tokens.revoke(&id);
    assert!(authorize(&workspace, &tokens, &ok).is_err());
}

#[test]
fn a_spawn_is_checked_and_limited() {
    let (_dir, mut workspace, id) = workspace();
    let caller = workspace.session(&id).unwrap().clone();

    let plan = plan_spawn(
        &workspace,
        &caller,
        &spawn("codex", "  Add tests\nfor parse()  "),
        0,
    )
    .unwrap();
    assert_eq!(plan.parent, id);
    assert_eq!(plan.session.agent, Agent::Codex);
    assert_eq!(plan.session.project_id, caller.project_id);
    assert_eq!(plan.session.title, "Add tests");
    assert_eq!(plan.session.prompt, "Add tests\nfor parse()");
    assert_eq!(plan.branch, None);

    let refused = |spawn: Spawn, running: usize| {
        plan_spawn(&workspace, &caller, &spawn, running)
            .unwrap_err()
            .to_string()
    };
    assert!(refused(spawn("gpt", "x"), 0).contains("Unknown agent"));
    assert!(refused(spawn("codex", "   "), 0).contains("prompt"));
    assert!(refused(spawn("codex", &"x".repeat(32_001)), 0).contains("too long"));
    assert!(refused(
        Spawn {
            model: Some("bad model; rm".into()),
            ..spawn("claude", "x")
        },
        0
    )
    .contains("model"));
    assert!(refused(
        Spawn {
            effort: Some("ultra".into()),
            ..spawn("claude", "x")
        },
        0
    )
    .contains("effort"));
    assert!(refused(
        Spawn {
            worktree: Some("--force".into()),
            ..spawn("claude", "x")
        },
        0
    )
    .contains("branch"));
    assert!(refused(spawn("claude", "x"), MAX_RUNNING).contains("already running"));
    assert!(plan_spawn(&workspace, &caller, &spawn("claude", "x"), MAX_RUNNING - 1).is_ok());

    let named = plan_spawn(
        &workspace,
        &caller,
        &Spawn {
            title: Some("Fix the Login page!".into()),
            worktree_auto: true,
            ..spawn("claude", "x")
        },
        0,
    )
    .unwrap();
    let branch = named.branch.unwrap();
    assert!(branch.starts_with("convoy/fix-the-login-page-"), "{branch}");
    assert!(valid_branch(&branch));
    let own = plan_spawn(
        &workspace,
        &caller,
        &Spawn {
            worktree: Some("feature/parser".into()),
            ..spawn("claude", "x")
        },
        0,
    )
    .unwrap();
    assert_eq!(own.branch.as_deref(), Some("feature/parser"));

    // Created under the manager, as a chat; a helper is never a manager.
    let child = create_helper(&mut workspace, &plan).unwrap();
    let session = workspace.session(&child).unwrap();
    assert_eq!(session.parent_id.as_deref(), Some(id.as_str()));
    assert_eq!(session.view.as_deref(), Some("chat"));
    assert!(!session.is_manager());
    assert_eq!(children(&workspace, &id).len(), 1);
    assert!(workspace.set_session_manager(&child, true).is_err());
    let helper = workspace.session(&child).unwrap().clone();
    assert!(plan_spawn(&workspace, &helper, &spawn("claude", "x"), 0).is_err());

    assert!(own_helpers(&workspace, &id, std::slice::from_ref(&child)).is_ok());
    assert!(own_helpers(&workspace, &id, std::slice::from_ref(&id)).is_err());
    assert!(own_helpers(&workspace, &id, &[]).is_err());
}

#[test]
fn titles_and_branches_are_made_from_what_is_given() {
    assert_eq!(title_from("# Fix the parser\nDetails"), "Fix the parser");
    assert_eq!(title_from("\n\n"), "Helper");
    let long = title_from(&"word ".repeat(30));
    assert!(long.ends_with('…') && long.chars().count() <= 60, "{long}");
    for good in ["feature/x", "convoy/fix-1", "a.b_c"] {
        assert!(valid_branch(good), "{good}");
    }
    for bad in ["", "-x", "a..b", "a/", "a b", "a//b", "x.lock", "/x", "a;b"] {
        assert!(!valid_branch(bad), "{bad}");
    }
}

#[test]
fn wait_follows_each_helper_through_its_turn() {
    let mut tracker = Tracker::default();
    tracker.track("c1", "p");
    // In its first turn from the start: never idle before its prompt.
    assert_eq!(tracker.status("c1", true), HelperStatus::Running);
    assert!(tracker.observe("c1", &ChatEvent::TurnStarted).is_none());
    tracker.observe(
        "c1",
        &ChatEvent::TextDone {
            id: "m".into(),
            text: "All tests pass.".into(),
        },
    );
    let settled = tracker
        .observe(
            "c1",
            &ChatEvent::TurnEnded {
                usage: Usage::default(),
                error: None,
            },
        )
        .unwrap();
    assert_eq!(
        settled,
        Settled {
            parent: "p".into(),
            child: "c1".into(),
            status: HelperStatus::Done
        }
    );
    assert_eq!(tracker.status("c1", true), HelperStatus::Done);
    assert_eq!(
        tracker.get("c1").unwrap().last_text.as_deref(),
        Some("All tests pass.")
    );
    // Done stays done when the process ends afterwards, and that is not news.
    assert!(tracker.ended("c1", Exit::Stopped).is_none());
    assert_eq!(tracker.status("c1", false), HelperStatus::Done);

    tracker.track("c2", "p");
    tracker.observe(
        "c2",
        &ChatEvent::TurnEnded {
            usage: Usage::default(),
            error: Some("Interrupted".into()),
        },
    );
    assert_eq!(tracker.status("c2", true), HelperStatus::Failed);
    // A new turn runs again.
    tracker.observe("c2", &ChatEvent::TurnStarted);
    assert_eq!(tracker.status("c2", true), HelperStatus::Running);

    tracker.track("c3", "p");
    let settled = tracker.ended("c3", Exit::Code(1)).unwrap();
    assert_eq!(settled.status, HelperStatus::Failed);
    tracker.track("c4", "p");
    assert_eq!(
        tracker.ended("c4", Exit::Stopped).unwrap().status,
        HelperStatus::Stopped
    );
    tracker.track("c5", "p");
    assert_eq!(
        tracker.ended("c5", Exit::Code(0)).unwrap().status,
        HelperStatus::Done
    );

    // One this app did not start: only whether it runs is known.
    assert_eq!(tracker.status("x", true), HelperStatus::Running);
    assert_eq!(tracker.status("x", false), HelperStatus::Stopped);
    // Events of sessions that are not helpers change nothing.
    assert!(tracker.observe("x", &ChatEvent::TurnStarted).is_none());

    let mut views = vec![
        HelperView {
            id: "a".into(),
            status: HelperStatus::Done,
            title: "A".into(),
            agent: "codex".into(),
        },
        HelperView {
            id: "b".into(),
            status: HelperStatus::Running,
            title: "B".into(),
            agent: "claude".into(),
        },
    ];
    cli::final_statuses(&mut views, true);
    assert_eq!(views[1].status, HelperStatus::Timeout);
    assert_eq!(cli::wait_code(&[HelperStatus::Done, HelperStatus::Done]), 0);
    assert_eq!(
        cli::wait_code(&[HelperStatus::Done, HelperStatus::Failed]),
        1
    );
    assert_eq!(cli::wait_code(&[HelperStatus::Stopped]), 1);
    assert_eq!(
        cli::wait_code(&[HelperStatus::Done, HelperStatus::Timeout]),
        124
    );
    assert_eq!(
        serde_json::to_value(HelperStatus::Timeout).unwrap(),
        serde_json::json!("timeout")
    );
}

#[test]
fn stale_requests_are_cleaned_up_and_the_heartbeat_says_who_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let requests = dir.path();
    assert!(!alive(requests, SystemTime::now()), "no heartbeat yet");
    heartbeat(requests).unwrap();
    assert!(alive(requests, SystemTime::now()));
    let later = SystemTime::now() + Duration::from_millis(ALIVE_MS + 1_000);
    assert!(!alive(requests, later));

    let request = Request::new("s", "t", Command::List);
    write_request(requests, &request).unwrap();
    write_result(requests, &request.id, &Response::value(1)).unwrap();
    assert_eq!(cleanup(requests, SystemTime::now()), 0, "fresh files stay");
    let much_later = SystemTime::now() + Duration::from_millis(STALE_MS + 1_000);
    assert_eq!(cleanup(requests, much_later), 2);
    assert!(pending(requests).is_empty());
    assert!(
        requests.join(".alive").exists(),
        "the heartbeat is never cleaned"
    );
}

#[test]
fn the_command_line_parses_and_fails_clearly() {
    let args = |text: &str| -> Vec<String> { text.split(' ').map(str::to_string).collect() };
    let Parsed::Spawn(spawn) = cli::parse(&args(
        "spawn --agent codex --model gpt-5 --effort high --worktree-auto --title T --prompt hello --skill Tests --skill Small",
    ))
    .unwrap() else {
        panic!()
    };
    assert_eq!(spawn.agent, "codex");
    assert_eq!(spawn.model.as_deref(), Some("gpt-5"));
    assert!(spawn.worktree_auto);
    assert_eq!(spawn.prompt, "hello");
    assert_eq!(spawn.skills, ["Tests", "Small"]);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("prompt.md");
    fs::write(&file, "From a file\nwith lines").unwrap();
    let Parsed::Spawn(spawn) = cli::parse(&[
        "spawn".into(),
        "--agent".into(),
        "claude".into(),
        "--prompt".into(),
        format!("@{}", file.display()),
    ])
    .unwrap() else {
        panic!()
    };
    assert_eq!(spawn.prompt, "From a file\nwith lines");

    assert_eq!(
        cli::parse(&args("wait a b a --timeout 5")).unwrap(),
        Parsed::Wait {
            ids: vec!["a".into(), "b".into()],
            timeout: Some(Duration::from_secs(5))
        }
    );
    assert_eq!(
        cli::parse(&args("output a")).unwrap(),
        Parsed::Output("a".into())
    );
    assert_eq!(cli::parse(&args("list")).unwrap(), Parsed::List);
    for bad in [
        "spawn --prompt x",
        "spawn --agent codex",
        "spawn --agent codex --prompt x --worktree b --worktree-auto",
        "spawn --agent codex --prompt x --skill",
        "spawn --agent",
        "wait",
        "wait a --timeout soon",
        "output",
        "output a b",
        "list x",
        "launch",
    ] {
        assert_eq!(cli::parse(&args(bad)).unwrap_err().code, 2, "{bad}");
    }

    // Not from a manager: says so, exit 2.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::run(&args("list"), &|_| None, &mut out, &mut err);
    assert_eq!(code, 2);
    assert!(String::from_utf8_lossy(&err).contains("manager session"));

    // Convoy not running: fails at once, exit 3.
    let requests = dir.path().join("requests");
    let env = |key: &str| match key {
        ENV_SESSION => Some("s".to_string()),
        ENV_TOKEN => Some("t".to_string()),
        ENV_REQUESTS => Some(requests.to_string_lossy().into_owned()),
        _ => None,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let started = std::time::Instant::now();
    let code = cli::run(&args("list"), &env, &mut out, &mut err);
    assert_eq!(code, 3);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(String::from_utf8_lossy(&err).contains("not running"));
    assert!(pending(&requests).is_empty(), "nothing left behind");

    // Running but silent: gives up after its patience and removes its request.
    heartbeat(&requests).unwrap();
    let timing = Timing {
        poll: Duration::from_millis(10),
        quick: Duration::from_millis(200),
        ..Default::default()
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::run_with(&args("list"), &env, &mut out, &mut err, &timing);
    assert_eq!(code, 3, "{}", String::from_utf8_lossy(&err));
    assert!(pending(&requests).is_empty());
}

/// The command line against a stand-in for the app: spawn prints the id,
/// wait polls until settled and prints one line each, output prints text.
#[test]
fn the_command_line_talks_to_whoever_answers() {
    let dir = tempfile::tempdir().unwrap();
    let requests = dir.path().join("requests");
    heartbeat(&requests).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = {
        let requests = requests.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut asked = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                for path in pending(&requests) {
                    let request = read_request(&path).unwrap();
                    let _ = fs::remove_file(&path);
                    assert_eq!(
                        (request.session.as_str(), request.token.as_str()),
                        ("s", "t")
                    );
                    let response = match request.command {
                        Command::Spawn(spawn) => {
                            assert_eq!(spawn.prompt, "hi");
                            Response::value(serde_json::json!({ "id": "child" }))
                        }
                        Command::Status { ids } => {
                            asked += 1;
                            let status = if asked < 3 {
                                HelperStatus::Running
                            } else {
                                HelperStatus::Done
                            };
                            Response::value(
                                ids.iter()
                                    .map(|id| HelperView {
                                        id: id.clone(),
                                        status,
                                        title: "T".into(),
                                        agent: "codex".into(),
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        }
                        Command::Output { id } => Response::value(
                            serde_json::json!({ "text": format!("answer of {id}") }),
                        ),
                        Command::List => Response::error("Nope."),
                    };
                    write_result(&requests, &request.id, &response).unwrap();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })
    };
    let env = |key: &str| match key {
        ENV_SESSION => Some("s".to_string()),
        ENV_TOKEN => Some("t".to_string()),
        ENV_REQUESTS => Some(requests.to_string_lossy().into_owned()),
        _ => None,
    };
    let timing = Timing {
        poll: Duration::from_millis(5),
        wait_step: Duration::from_millis(10),
        ..Default::default()
    };
    let run = |text: &str| {
        let args: Vec<String> = text.split(' ').map(str::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = cli::run_with(&args, &env, &mut out, &mut err, &timing);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    assert_eq!(
        run("spawn --agent codex --prompt hi"),
        (0, "child\n".into(), String::new())
    );
    let (code, out, _) = run("wait child");
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "{\"id\":\"child\",\"status\":\"done\",\"title\":\"T\"}\n"
    );
    assert_eq!(run("output child").1, "answer of child\n");
    let (code, _, err) = run("list");
    assert_eq!(code, 1);
    assert_eq!(err, "convoy: Nope.\n");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    app.join().unwrap();
}

#[test]
fn a_manager_launch_carries_its_variables_and_instructions() {
    let launch = ManagerLaunch {
        token: "tok".into(),
        bin: PathBuf::from("/Applications/Convoy.app/Contents/MacOS/convoy"),
        requests: PathBuf::from("/data/telemetry/requests"),
    };
    let env: HashMap<String, String> = launch.env("s1").into_iter().collect();
    assert_eq!(env[ENV_SESSION], "s1");
    assert_eq!(env[ENV_TOKEN], "tok");
    assert_eq!(
        env[ENV_BIN],
        "/Applications/Convoy.app/Contents/MacOS/convoy"
    );
    assert_eq!(env[ENV_REQUESTS], "/data/telemetry/requests");

    let text = instructions(crate::Platform::Unix);
    for part in [
        "--session spawn",
        "--session wait",
        "--session output",
        "\"$CONVOY_BIN\"",
        "At most 4",
    ] {
        assert!(text.contains(part), "{part} missing");
    }
    assert!(instructions(crate::Platform::Windows).contains("& $env:CONVOY_BIN"));
    assert_eq!(
        terminal_flags(Agent::Claude, "hi")[0],
        "--append-system-prompt"
    );
    let codex = terminal_flags(Agent::Codex, "say \"hi\"\nnow");
    assert_eq!(codex[0], "-c");
    assert_eq!(codex[1], r#"developer_instructions="say \"hi\"\nnow""#);
}

/// `parentID` and `manager` are written as the other clients would read
/// them, a field this build does not know survives, and a helper outlives
/// its manager as an ordinary session.
#[test]
fn parent_and_manager_round_trip_through_the_workspace_file() {
    let (dir, mut workspace, id) = workspace();
    let caller = workspace.session(&id).unwrap().clone();
    let plan = plan_spawn(&workspace, &caller, &spawn("codex", "Help"), 0).unwrap();
    let child = create_helper(&mut workspace, &plan).unwrap();

    let file = dir.path().join("workspace.json");
    let mut raw: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let sessions = raw["sessions"].as_array_mut().unwrap();
    let manager = sessions.iter().find(|s| s["id"] == id.as_str()).unwrap();
    assert_eq!(manager["manager"], true);
    assert!(manager.get("parentID").is_none());
    let helper = sessions
        .iter_mut()
        .find(|s| s["id"] == child.as_str())
        .unwrap();
    assert_eq!(helper["parentID"], id.as_str());
    assert!(helper.get("manager").is_none(), "absent, not false");
    helper["fromTheFuture"] = serde_json::json!({ "kept": true });
    fs::write(&file, raw.to_string()).unwrap();

    let mut reloaded = Workspace::load(&file).unwrap();
    assert_eq!(
        reloaded.session(&child).unwrap().parent_id.as_deref(),
        Some(id.as_str())
    );
    assert!(reloaded.session(&id).unwrap().is_manager());
    reloaded.set_session_view(&child, "chat").unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let helper = raw["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == child.as_str())
        .unwrap();
    assert_eq!(helper["fromTheFuture"]["kept"], true);

    // A parent of the wrong type is refused like any damaged field.
    let mut bad = raw.clone();
    bad["sessions"][1]["parentID"] = serde_json::json!(5);
    assert!(crate::workspace::validate::validate(&bad).is_err());
    let mut bad = raw.clone();
    bad["sessions"][0]["manager"] = serde_json::json!("yes");
    assert!(crate::workspace::validate::validate(&bad).is_err());

    reloaded.remove_sessions(std::slice::from_ref(&id)).unwrap();
    assert_eq!(reloaded.session(&child).unwrap().parent_id, None);
}

#[test]
fn last_answer_is_the_final_assistant_message() {
    let events = vec![
        ChatEvent::TextDone {
            id: "1".into(),
            text: "First".into(),
        },
        ChatEvent::User {
            id: "u".into(),
            text: "More".into(),
        },
        ChatEvent::TextDone {
            id: "2".into(),
            text: "Final answer".into(),
        },
        ChatEvent::TextDone {
            id: "3".into(),
            text: "  ".into(),
        },
    ];
    assert_eq!(last_answer(&events).as_deref(), Some("Final answer"));
    assert_eq!(last_answer(&[]), None);
}
