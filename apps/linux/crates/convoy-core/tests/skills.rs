//! Skills at launch, through the public API: attached skills become the
//! agent's instructions, joined with a manager's into one value, in a
//! terminal and in a chat, for either agent.

mod common;

use common::{fixture, new_session};
use convoy_core::helpers::ManagerLaunch;
use convoy_core::planning::TaskInput;
use convoy_core::session::{plan_chat_launch_with, plan_launch, plan_launch_with};
use convoy_core::skills::{SkillInput, Store};
use convoy_core::workspace::model::{Agent, PublishMode};
use convoy_core::workspace::SettingsPatch;
use convoy_core::Storage;
use serde_json::Value;

struct World {
    fixture: common::Fixture,
    workspace: convoy_core::Workspace,
    storage: Storage,
    project: String,
}

fn world() -> World {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_settings(SettingsPatch {
            auto_trust: Some(false),
            status_hooks: Some(false),
            ..Default::default()
        })
        .unwrap();
    let storage = Storage::new(fixture.path().join("state"));
    World {
        fixture,
        workspace,
        storage,
        project,
    }
}

fn add_skill(world: &World, name: &str, body: &str, agents: &str) -> String {
    let mut store = Store::load(world.storage.skills()).unwrap();
    store
        .save(
            SkillInput {
                id: None,
                name: name.into(),
                description: String::new(),
                body: body.into(),
                project_id: None,
                agents: agents.into(),
                auto: false,
                roles: None,
            },
            &world.workspace,
            1,
        )
        .unwrap()
}

fn session(world: &mut World, agent: Agent, skills: Vec<String>) -> String {
    let mut input = new_session(&world.project, agent, "Skilled");
    input.prompt = "Fix the login redirect".into();
    input.skills = skills;
    world.workspace.add_session(input).unwrap();
    world.workspace.state().sessions.last().unwrap().id.clone()
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

const EXE: &str = "/bin/convoy";

#[cfg(unix)]
#[test]
fn a_terminal_launch_gives_the_skills_that_suit_its_agent() {
    let mut world = world();
    let both = add_skill(&world, "Small commits", "Commit in small steps.", "both");
    let claude_only = add_skill(&world, "Claude tone", "Be brief.", "claude");
    let codex_only = add_skill(&world, "Codex tone", "Be thorough.", "codex");
    let attached = vec![codex_only.clone(), claude_only.clone(), both.clone()];

    let id = session(&mut world, Agent::Claude, attached.clone());
    let plan = plan_launch(&world.workspace, &world.storage, &id, EXE.as_ref()).unwrap();
    let command = &plan.spec.args[1];
    assert_eq!(count(command, "'--append-system-prompt'"), 1, "{command}");
    assert!(
        command.contains("## Claude tone\nBe brief.\n\n## Small commits\nCommit in small steps."),
        "{command}"
    );
    assert!(!command.contains("Codex tone"));
    assert!(
        command
            .trim_end()
            .ends_with("'--' 'Fix the login redirect'"),
        "{command}"
    );
    let text = plan.instructions.unwrap();
    assert!(text.starts_with("Skills the person attached"));

    let id = session(&mut world, Agent::Codex, attached);
    let plan = plan_launch(&world.workspace, &world.storage, &id, EXE.as_ref()).unwrap();
    let command = &plan.spec.args[1];
    assert_eq!(count(command, "developer_instructions="), 1, "{command}");
    assert!(
        command.contains(r#"## Codex tone\nBe thorough.\n\n## Small commits"#),
        "{command}"
    );
    assert!(!command.contains("Claude tone"));

    // No skills: no flag, and the file is never needed.
    let plain = session(&mut world, Agent::Claude, vec![]);
    let plan = plan_launch(&world.workspace, &world.storage, &plain, EXE.as_ref()).unwrap();
    assert!(!plan.spec.args[1].contains("--append-system-prompt"));
    assert!(plan.instructions.is_none());
}

/// A manager with skills gets one appended system prompt, or one set of
/// developer instructions, holding both — two would replace each other.
#[cfg(unix)]
#[test]
fn a_managers_instructions_and_its_skills_go_in_one_value() {
    let launch = ManagerLaunch {
        token: "secret".into(),
        bin: "/opt/convoy/convoy".into(),
        requests: "/tmp/requests".into(),
    };
    for agent in [Agent::Claude, Agent::Codex] {
        let mut world = world();
        let skill = add_skill(
            &world,
            "Pineapple",
            "End every reply with PINEAPPLE.",
            "both",
        );
        let id = session(&mut world, agent, vec![skill]);
        world.workspace.set_session_manager(&id, true).unwrap();

        let terminal = plan_launch_with(
            &world.workspace,
            &world.storage,
            &id,
            EXE.as_ref(),
            Some(&launch),
        )
        .unwrap();
        let command = &terminal.spec.args[1];
        let text = terminal.instructions.clone().unwrap();
        assert!(text.starts_with("You are a manager session"), "{text}");
        assert!(
            text.ends_with("## Pineapple\nEnd every reply with PINEAPPLE."),
            "{text}"
        );
        match agent {
            Agent::Claude => {
                assert_eq!(count(command, "'--append-system-prompt'"), 1, "{command}")
            }
            Agent::Codex => assert_eq!(count(command, "developer_instructions="), 1, "{command}"),
        }
        assert!(command.contains("--session spawn") && command.contains("PINEAPPLE"));

        let mut chat = plan_chat_launch_with(
            &world.workspace,
            &world.storage,
            &id,
            EXE.as_ref(),
            Some(&launch),
        )
        .unwrap();
        match agent {
            Agent::Claude => {
                let command = &chat.launch.spec.args[1];
                assert_eq!(count(command, "'--append-system-prompt'"), 1, "{command}");
                assert!(command.contains("--session spawn") && command.contains("PINEAPPLE"));
                assert!(command.contains("'--input-format' 'stream-json'"));
            }
            Agent::Codex => {
                assert!(!chat.launch.spec.args[1].contains("developer_instructions"));
                chat.driver.start();
                chat.driver.feed(r#"{"id":1,"result":{}}"#);
                let sent: Vec<Value> = chat
                    .driver
                    .take_outgoing()
                    .iter()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(sent[1]["method"], "thread/start");
                let given = sent[1]["params"]["developerInstructions"].as_str().unwrap();
                assert!(given.contains("--session wait"), "{given}");
                assert!(
                    given.ends_with("End every reply with PINEAPPLE."),
                    "{given}"
                );
            }
        }
    }
}

/// Changed while it runs, the skills apply at the next start; a detached or
/// deleted skill is not given; too much together stops the start with a
/// reason.
#[cfg(unix)]
#[test]
fn skills_are_read_at_each_start() {
    let mut world = world();
    let first = add_skill(&world, "First", "One.", "both");
    let second = add_skill(&world, "Second", "Two.", "both");
    let id = session(&mut world, Agent::Claude, vec![first.clone()]);
    world
        .workspace
        .set_session_skills(&id, vec![second.clone(), first.clone()])
        .unwrap();
    let command = plan_launch(&world.workspace, &world.storage, &id, EXE.as_ref())
        .unwrap()
        .spec
        .args[1]
        .clone();
    assert!(
        command.contains("## Second\nTwo.\n\n## First\nOne."),
        "{command}"
    );

    // Deleted from the library: passed over even before it is detached.
    let mut store = Store::load(world.storage.skills()).unwrap();
    store.remove(&second).unwrap();
    let command = plan_launch(&world.workspace, &world.storage, &id, EXE.as_ref())
        .unwrap()
        .spec
        .args[1]
        .clone();
    assert!(!command.contains("Second") && command.contains("## First"));

    let huge: Vec<String> = (0..3)
        .map(|n| add_skill(&world, &format!("Huge {n}"), &"x".repeat(16_000), "both"))
        .collect();
    world.workspace.set_session_skills(&id, huge).unwrap();
    let error = plan_launch(&world.workspace, &world.storage, &id, EXE.as_ref())
        .unwrap_err()
        .to_string();
    assert!(error.contains("32 KB at most"), "{error}");
    drop(world.fixture);
}

/// A task's skills go to the session prepared for it, and a change to them
/// reaches a session already prepared.
#[test]
fn a_tasks_skills_go_to_its_session() {
    let mut world = world();
    let skill = add_skill(&world, "Tests first", "Write a failing test.", "both");
    let base = TaskInput {
        skills: Some(vec![skill.clone()]),
        id: None,
        project_id: world.project.clone(),
        spec_id: None,
        title: "Fix login".into(),
        details: "The redirect loops.".into(),
        findings: String::new(),
        agent: Agent::Codex,
        mode: PublishMode::None,
        auto_review: false,
    };
    world.workspace.save_task(base.clone()).unwrap();
    let task = world.workspace.state().tasks[0].id.clone();
    assert_eq!(
        world.workspace.state().tasks[0].skills,
        Some(vec![skill.clone()])
    );
    world.workspace.prepare_task(&task, None).unwrap();
    let session = world.workspace.state().tasks[0].session_id.clone().unwrap();
    assert_eq!(
        world.workspace.session(&session).unwrap().skills,
        Some(vec![skill.clone()])
    );

    // Saved without skills: unchanged. Saved with none: gone, from the
    // prepared session too.
    world
        .workspace
        .save_task(TaskInput {
            id: Some(task.clone()),
            skills: None,
            ..base.clone()
        })
        .unwrap();
    assert_eq!(
        world.workspace.state().tasks[0].skills,
        Some(vec![skill.clone()])
    );
    world
        .workspace
        .save_task(TaskInput {
            id: Some(task.clone()),
            skills: Some(vec![]),
            ..base
        })
        .unwrap();
    assert_eq!(world.workspace.state().tasks[0].skills, None);
    assert_eq!(world.workspace.session(&session).unwrap().skills, None);
}

/// The attachments are plain ids on the records, written only when there
/// are some; a file without them reads as before.
#[test]
fn attachments_round_trip_in_the_workspace_file() {
    let mut world = world();
    let id = session(&mut world, Agent::Claude, vec![]);
    let text = std::fs::read_to_string(world.workspace.file()).unwrap();
    assert!(!text.contains("\"skills\""), "{text}");
    world
        .workspace
        .set_session_skills(&id, vec!["a".into(), "b".into(), "a".into()])
        .unwrap();
    let reloaded = world.fixture.workspace();
    assert_eq!(
        reloaded.session(&id).unwrap().skills,
        Some(vec!["a".to_string(), "b".to_string()])
    );
    world.workspace.set_session_skills(&id, vec![]).unwrap();
    assert_eq!(world.fixture.workspace().session(&id).unwrap().skills, None);
}

/// The real thing: a chat session with one skill, asked to say hi, follows
/// the skill — for Claude Code and for Codex. Spends a little of each
/// subscription, so it only runs when asked:
///
///     cargo test -p convoy-core --test skills real_skill -- --ignored --nocapture
///
/// Run it with a clean environment (`env -i HOME=… PATH=… SHELL=…`) when
/// started from inside Claude Code.
#[cfg(unix)]
#[test]
#[ignore = "talks to the real Claude Code and Codex"]
fn real_skill_is_followed_by_both_agents() {
    use convoy_core::chat::ChatEvent;
    use convoy_core::session::plan_chat_launch;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    for agent in [Agent::Claude, Agent::Codex] {
        let mut world = world();
        let skill = add_skill(
            &world,
            "Pineapple sign-off",
            "Always end your reply with the word PINEAPPLE.",
            "both",
        );
        let mut input = new_session(&world.project, agent, "Skill check");
        input.prompt = "Say hi".into();
        if agent == Agent::Claude {
            input.model = "haiku".into();
        }
        input.skills = vec![skill];
        world.workspace.add_session(input).unwrap();
        let id = world.workspace.state().sessions[0].id.clone();
        let mut plan = plan_chat_launch(
            &world.workspace,
            &world.storage,
            &id,
            std::path::Path::new("/usr/bin/true"),
        )
        .unwrap();
        assert!(plan
            .launch
            .instructions
            .as_deref()
            .unwrap()
            .contains("PINEAPPLE"));

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
        let (mut text, mut ended) = (String::new(), None);
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
                    ChatEvent::TurnEnded { error, .. } => ended = Some(error),
                    _ => {}
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let error = ended.unwrap_or_else(|| panic!("{agent:?}: the turn never ended"));
        println!("{agent:?} replied {text:?}");
        assert_eq!(error, None, "{agent:?}");
        assert!(
            text.trim_end()
                .trim_end_matches(['.', '!', '*', '"'])
                .to_uppercase()
                .ends_with("PINEAPPLE"),
            "{agent:?} replied {text:?}"
        );
    }
}
