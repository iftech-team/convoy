use super::*;
use crate::workspace::model::Agent;
use crate::workspace::Workspace;

fn world() -> (tempfile::TempDir, Workspace, String) {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let mut workspace = Workspace::load(root.path().join("workspace.json")).unwrap();
    workspace.add_project(&folder).unwrap();
    let project = workspace.state().projects[0].id.clone();
    (root, workspace, project)
}

fn input(name: &str, body: &str) -> SkillInput {
    SkillInput {
        id: None,
        name: name.into(),
        description: String::new(),
        body: body.into(),
        project_id: None,
        agents: "both".into(),
        auto: false,
        roles: None,
    }
}

fn skill(id: &str, name: &str, body: &str, agents: Agents, project: Option<&str>) -> Skill {
    Skill {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        body: body.into(),
        project_id: project.map(str::to_string),
        agents,
        auto: false,
        roles: None,
        created: 0,
        updated: 0,
        unknown: Map::new(),
    }
}

#[test]
fn a_skill_is_kept_in_its_own_file_and_read_back_whole() {
    let (root, workspace, project) = world();
    let file = root.path().join("skills.json");
    let mut store = Store::load(&file).unwrap();
    assert!(store.all().is_empty(), "no file is an empty library");
    let id = store
        .save(
            SkillInput {
                description: "  Keep history readable  ".into(),
                project_id: Some(project.clone()),
                agents: "codex".into(),
                auto: true,
                ..input("  Small commits ", "\n Commit often.\n")
            },
            &workspace,
            100,
        )
        .unwrap();

    // A field a newer build wrote survives a rewrite.
    let mut raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    raw["skills"][0]["colour"] = "teal".into();
    raw["future"] = true.into();
    std::fs::write(&file, raw.to_string()).unwrap();

    let mut store = Store::load(&file).unwrap();
    store
        .save(
            SkillInput {
                id: Some(id.clone()),
                ..input(
                    "Small commits",
                    "Commit often. Never mix refactors with fixes.",
                )
            },
            &workspace,
            200,
        )
        .unwrap();
    let store = Store::load(&file).unwrap();
    let saved = store.get(&id).unwrap();
    assert_eq!(saved.name, "Small commits");
    assert_eq!(saved.body, "Commit often. Never mix refactors with fixes.");
    assert_eq!(saved.project_id, None, "edited to everywhere");
    assert_eq!(saved.agents, Agents::Both);
    assert!(!saved.auto);
    assert_eq!((saved.created, saved.updated), (100, 200));
    assert_eq!(saved.unknown["colour"], "teal");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"future\": true"), "{text}");
    // No temporary file is left behind.
    let names: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|name| !name.ends_with(".tmp")),
        "{names:?}"
    );

    std::fs::write(&file, "{ not json").unwrap();
    assert!(Store::load(&file)
        .unwrap_err()
        .to_string()
        .contains("skills file is damaged"));
    std::fs::write(&file, r#"{"version": 9, "skills": []}"#).unwrap();
    assert!(Store::load(&file)
        .unwrap_err()
        .to_string()
        .contains("newer"));
}

#[test]
fn what_the_editor_sends_is_checked() {
    let (root, workspace, _project) = world();
    let mut store = Store::load(root.path().join("skills.json")).unwrap();
    store
        .save(input("Tests first", "Write a failing test."), &workspace, 1)
        .unwrap();
    let refused = |input: SkillInput| {
        Store::load(root.path().join("skills.json"))
            .unwrap()
            .save(input, &workspace, 2)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(refused(input("  ", "x")), "Give the skill a name.");
    assert!(refused(input(&"n".repeat(81), "x")).contains("too long"));
    assert!(refused(input("Two\nlines", "x")).contains("one line"));
    assert_eq!(
        refused(input("Blank", "   ")),
        "Write what the skill tells the agent."
    );
    assert!(refused(input("Huge", &"x".repeat(MAX_BODY + 1))).contains("16,000"));
    assert_eq!(
        refused(input("tests FIRST", "again")),
        "A skill with this name already exists."
    );
    assert_eq!(
        refused(SkillInput {
            agents: "gemini".into(),
            ..input("Other", "x")
        }),
        "Pick Claude, Codex or both."
    );
    assert_eq!(
        refused(SkillInput {
            project_id: Some("nowhere".into()),
            ..input("Other", "x")
        }),
        "Project not found."
    );
    assert!(refused(SkillInput {
        description: "d".repeat(301),
        ..input("Other", "x")
    })
    .contains("description"));
    assert_eq!(
        refused(SkillInput {
            id: Some("missing".into()),
            ..input("Other", "x")
        }),
        "Skill not found."
    );
    // Nothing refused was written.
    assert_eq!(
        Store::load(root.path().join("skills.json"))
            .unwrap()
            .all()
            .len(),
        1
    );
}

#[test]
fn defaults_checks_and_names_follow_scope_and_agent() {
    let (root, mut workspace, project) = world();
    let other_folder = root.path().join("other");
    std::fs::create_dir_all(&other_folder).unwrap();
    workspace.add_project(&other_folder).unwrap();
    let other = workspace.state().projects[1].id.clone();
    let mut store = Store::load(root.path().join("skills.json")).unwrap();
    let mut save = |input: SkillInput| store.save(input, &workspace, 1).unwrap();
    let global = save(SkillInput {
        auto: true,
        ..input("Global", "g")
    });
    let mine = save(SkillInput {
        auto: true,
        agents: "claude".into(),
        project_id: Some(project.clone()),
        ..input("Mine", "m")
    });
    let theirs = save(SkillInput {
        auto: true,
        project_id: Some(other.clone()),
        ..input("Theirs", "t")
    });
    let manual = save(input("Manual", "x"));

    assert_eq!(
        store.defaults(&project, None),
        [global.clone(), mine.clone()]
    );
    assert_eq!(
        store.defaults(&project, Some(Agent::Codex)),
        std::slice::from_ref(&global)
    );
    assert_eq!(
        store.defaults(&other, None),
        [global.clone(), theirs.clone()]
    );

    assert_eq!(
        store
            .check(&project, &[manual.clone(), mine.clone(), manual.clone()])
            .unwrap(),
        [manual.clone(), mine.clone()]
    );
    assert!(store
        .check(&project, std::slice::from_ref(&theirs))
        .unwrap_err()
        .to_string()
        .contains("another project"));
    assert_eq!(
        store
            .check(&project, &["gone".into()])
            .unwrap_err()
            .to_string(),
        "Skill not found."
    );

    assert_eq!(
        store
            .named(&project, Agent::Claude, &["mine".into(), " MANUAL ".into()])
            .unwrap(),
        [mine.clone(), manual.clone()]
    );
    assert!(store
        .named(&project, Agent::Codex, &["Mine".into()])
        .unwrap_err()
        .to_string()
        .contains("not for Codex"));
    assert!(store
        .named(&project, Agent::Claude, &["Nope".into()])
        .unwrap_err()
        .to_string()
        .contains("No skill is named “Nope”"));
}

#[test]
fn instructions_keep_the_attached_order_and_only_what_applies() {
    let skills = vec![
        skill("a", "Alpha", "Do alpha.", Agents::Both, None),
        skill("b", "Beta", "  Do beta.\n", Agents::Claude, None),
        skill("c", "Gamma", "Do gamma.", Agents::Codex, None),
        skill("d", "Delta", "Do delta.", Agents::Both, Some("p2")),
    ];
    let attached: Vec<String> = ["c", "gone", "b", "a", "d"]
        .iter()
        .map(|id| id.to_string())
        .collect();
    let claude = instructions(&skills, &attached, Agent::Claude, "p1")
        .unwrap()
        .unwrap();
    assert!(claude.starts_with(HEADER));
    assert!(
        claude.ends_with("## Beta\nDo beta.\n\n## Alpha\nDo alpha."),
        "{claude}"
    );
    assert!(!claude.contains("Gamma") && !claude.contains("Delta"));
    let codex = instructions(&skills, &attached, Agent::Codex, "p2")
        .unwrap()
        .unwrap();
    assert!(codex.ends_with("## Gamma\nDo gamma.\n\n## Alpha\nDo alpha.\n\n## Delta\nDo delta."));
    assert_eq!(
        instructions(&skills, &["c".into()], Agent::Claude, "p1").unwrap(),
        None,
        "nothing that applies is nothing at all"
    );
    assert_eq!(
        instructions(&skills, &[], Agent::Claude, "p1").unwrap(),
        None
    );

    // Too much together is refused with a reason, not cut short.
    let big: Vec<Skill> = (0..3)
        .map(|n| {
            skill(
                &n.to_string(),
                &format!("S{n}"),
                &"x".repeat(MAX_BODY),
                Agents::Both,
                None,
            )
        })
        .collect();
    let all: Vec<String> = (0..3).map(|n| n.to_string()).collect();
    let error = instructions(&big, &all, Agent::Claude, "p1")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("too long together (48 KB; 32 KB at most)"),
        "{error}"
    );
    assert!(instructions(&big, &all[..1], Agent::Claude, "p1")
        .unwrap()
        .is_some());
}

#[test]
fn instructions_are_combined_into_one_value() {
    assert_eq!(combine(&[None, None]), None);
    assert_eq!(combine(&[Some("  ".into()), None]), None);
    assert_eq!(
        combine(&[Some("Manager.\n".into()), Some("Skills.".into())]).as_deref(),
        Some("Manager.\n\nSkills.")
    );
    assert_eq!(
        combine(&[None, Some("Skills.".into())]).as_deref(),
        Some("Skills.")
    );
}

/// Removing a skill takes it off every session, task and automation;
/// removing a project removes its own skills and detaches them too.
#[test]
fn removing_skills_detaches_them_everywhere() {
    use crate::automations::{AutomationInput, Schedule, Store as Automations};
    use crate::planning::TaskInput;
    use crate::workspace::NewSession;

    let (root, mut workspace, project) = world();
    let mut store = Store::load(root.path().join("skills.json")).unwrap();
    let keep = store.save(input("Keep", "k"), &workspace, 1).unwrap();
    let drop = store
        .save(
            SkillInput {
                project_id: Some(project.clone()),
                ..input("Drop", "d")
            },
            &workspace,
            1,
        )
        .unwrap();

    let mut session = NewSession::new(project.clone(), Agent::Claude, "One");
    session.skills = vec![drop.clone(), keep.clone(), drop.clone()];
    workspace.add_session(session).unwrap();
    let mut only = NewSession::new(project.clone(), Agent::Codex, "Two");
    only.skills = vec![drop.clone()];
    workspace.add_session(only).unwrap();
    assert_eq!(
        workspace.state().sessions[0].skills,
        Some(vec![drop.clone(), keep.clone()]),
        "kept once each, in order"
    );
    workspace
        .save_task(TaskInput {
            skills: Some(vec![keep.clone(), drop.clone()]),
            id: None,
            project_id: project.clone(),
            spec_id: None,
            title: "Task".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: crate::model::PublishMode::None,
            auto_review: false,
        })
        .unwrap();
    let mut automations = Automations::load(root.path().join("automations.json")).unwrap();
    let automation = automations
        .save(
            AutomationInput {
                skills: Some(vec![drop.clone()]),
                id: None,
                project_id: project.clone(),
                title: "Nightly".into(),
                prompt: "Check".into(),
                agent: "claude".into(),
                model: String::new(),
                effort: String::new(),
                profile_id: None,
                view: "chat".into(),
                worktree: false,
                schedule: Schedule::Daily { at: "03:00".into() },
                enabled: true,
                catch_up: false,
            },
            &workspace,
            1,
        )
        .unwrap();

    store.remove(&drop).unwrap();
    detach_everywhere(
        &mut workspace,
        &mut automations,
        std::slice::from_ref(&drop),
    )
    .unwrap();
    let state = Workspace::load(workspace.file()).unwrap().state().clone();
    assert_eq!(state.sessions[0].skills, Some(vec![keep.clone()]));
    assert_eq!(state.sessions[1].skills, None, "an empty list is no list");
    assert_eq!(state.tasks[0].skills, Some(vec![keep.clone()]));
    let reloaded = Automations::load(root.path().join("automations.json")).unwrap();
    assert_eq!(reloaded.get(&automation).unwrap().skills, None);
    assert_eq!(Store::load(store.file()).unwrap().all().len(), 1);

    // A project's own skills go with it; global ones stay.
    let own = store
        .save(
            SkillInput {
                project_id: Some(project.clone()),
                ..input("Own", "o")
            },
            &workspace,
            1,
        )
        .unwrap();
    assert_eq!(
        store.remove_project(&project).unwrap(),
        std::slice::from_ref(&own)
    );
    assert_eq!(
        store.remove_project(&project).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(store.all().len(), 1);
    let again = store
        .save(
            SkillInput {
                project_id: Some(project.clone()),
                ..input("Again", "a")
            },
            &workspace,
            1,
        )
        .unwrap();
    // Removed by a build that knows nothing of the file: pruned on sight.
    workspace.remove_project(&project).unwrap();
    assert_eq!(store.prune(&workspace).unwrap(), [again]);
    assert_eq!(store.remove_all_projects().unwrap(), Vec::<String>::new());
    assert_eq!(store.all()[0].id, keep);
}
