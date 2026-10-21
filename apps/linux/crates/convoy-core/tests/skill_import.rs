//! Claude Code's skills, found on disk and imported into Convoy's library
//! for the projects a person picks.

mod common;

use common::fixture;
use convoy_core::model::{Agent, Role};
use convoy_core::skills::local;
use convoy_core::skills::Store;

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn front_matter_is_read_in_its_common_shapes() {
    let (name, description, body) = local::parse(
        "---\nname: eskhata-qa\ndescription: Test an Eskhata task end to end.\n---\n\n# Eskhata QA\n\nRun it.\n",
    );
    assert_eq!(name.as_deref(), Some("eskhata-qa"));
    assert_eq!(description, "Test an Eskhata task end to end.");
    assert_eq!(body, "# Eskhata QA\n\nRun it.");

    let (name, description, _) = local::parse(
        "---\nname: \"quoted\"\ndescription: >\n  Folded over\n  two lines.\nallowed-tools: Bash\n---\nBody",
    );
    assert_eq!(name.as_deref(), Some("quoted"));
    assert_eq!(description, "Folded over two lines.");

    let (name, _, body) = local::parse("# No front matter\nJust text.");
    assert_eq!(name, None);
    assert_eq!(body, "# No front matter\nJust text.");
}

#[test]
fn skills_are_found_and_imported_for_the_projects_picked() {
    let fixture = fixture();
    let (mut workspace, admin) = fixture.with_project();
    let api_dir = fixture.path().join("api");
    std::fs::create_dir_all(&api_dir).unwrap();
    workspace.add_project(&api_dir).unwrap();
    let api = workspace.state().projects.last().unwrap().id.clone();
    let third_dir = fixture.path().join("bot");
    std::fs::create_dir_all(&third_dir).unwrap();
    workspace.add_project(&third_dir).unwrap();
    let bot = workspace.state().projects.last().unwrap().id.clone();

    let home = tempfile::tempdir().unwrap();
    let qa = home.path().join(".claude/skills/eskhata-qa/SKILL.md");
    write(
        &qa,
        "---\nname: eskhata-qa\ndescription: Test it on the local stack.\n---\n# QA\nRun scripts/qa-up.sh first.",
    );
    write(
        &home
            .path()
            .join(".claude/skills/eskhata-qa/scripts/qa-up.sh"),
        "#!/bin/sh\n",
    );
    write(
        &api_dir.join(".claude/skills/api-style/SKILL.md"),
        "---\nname: api-style\ndescription: How the API is written.\n---\nUse records.",
    );

    let mut store = Store::load(fixture.path().join("skills.json")).unwrap();
    let found = local::discover(&workspace, home.path(), &store);
    assert_eq!(
        found
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        ["eskhata-qa", "api-style"]
    );
    assert_eq!(found[0].project_id, None);
    assert_eq!(found[1].project_id.as_deref(), Some(api.as_str()));
    assert!(!found[0].imported);

    // For two of the three projects, for QA and the builder, attached by itself.
    let id = local::import(
        &mut store,
        &workspace,
        &qa,
        vec![admin.clone(), api.clone()],
        Some(vec![Role::Qa, Role::Build]),
        true,
        1,
    )
    .unwrap();
    let skill = store.get(&id).unwrap().clone();
    assert!(skill.in_scope(&admin) && skill.in_scope(&api) && !skill.in_scope(&bot));
    assert!(skill.body.contains("Run scripts/qa-up.sh first."));
    assert!(skill.body.contains(&format!(
        "files (scripts, templates) are in {}",
        qa.parent().unwrap().display()
    )));
    assert_eq!(
        store.for_role(&api, Agent::Codex, Role::Qa),
        std::slice::from_ref(&id)
    );
    assert!(store.for_role(&bot, Agent::Codex, Role::Qa).is_empty());
    assert!(store.defaults(&admin, None).contains(&id));

    // Found again, it shows as imported; importing again refreshes it.
    assert!(local::discover(&workspace, home.path(), &store)[0].imported);
    write(
        &qa,
        "---\nname: eskhata-qa\ndescription: Newer.\n---\n# QA\nNever touch es_wallet_restored.",
    );
    let again = local::import(&mut store, &workspace, &qa, Vec::new(), None, false, 2).unwrap();
    assert_eq!(again, id, "the same skill, not a second one");
    assert_eq!(store.all().len(), 1);
    let skill = store.get(&id).unwrap();
    assert!(skill.body.contains("es_wallet_restored"));
    assert!(skill.in_scope(&bot), "now for every project");

    // A project removed takes itself off a shared skill, which stays.
    local::import(
        &mut store,
        &workspace,
        &qa,
        vec![admin.clone(), api.clone()],
        None,
        false,
        3,
    )
    .unwrap();
    store.remove_project(&api).unwrap();
    assert_eq!(
        store.get(&id).unwrap().scope(),
        std::slice::from_ref(&admin)
    );
    store.remove_project(&admin).unwrap();
    assert!(
        store.get(&id).is_err(),
        "its last project gone, it goes too"
    );
}

#[test]
fn a_long_skill_is_shortened_with_where_to_read_the_rest() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("big/SKILL.md");
    write(
        &path,
        &format!("---\nname: big\n---\n{}", "line\n".repeat(5_000)),
    );
    let (name, _, body) = local::body(&path).unwrap();
    assert_eq!(name, "big");
    assert!(body.encode_utf16().count() <= convoy_core::skills::MAX_BODY);
    assert!(body.ends_with(&format!("read the whole skill at {})", path.display())));
}

#[test]
fn codex_and_shared_agent_skills_are_found_too() {
    let fixture = fixture();
    let (workspace, project) = fixture.with_project();
    let home = tempfile::tempdir().unwrap();
    write(
        &home.path().join(".codex/skills/release/SKILL.md"),
        "---\nname: release\n---\nShip it.",
    );
    write(
        &home.path().join(".codex/skills/.system/imagegen/SKILL.md"),
        "---\nname: imagegen\n---\nBuilt in.",
    );
    write(
        &home.path().join(".agents/skills/better-ui/SKILL.md"),
        "---\nname: better-ui\n---\nPolish.",
    );
    write(
        &fixture.path().join(".agents/skills/house-style/SKILL.md"),
        "---\nname: house-style\n---\nOurs.",
    );
    // Claude's folder links to the shared one: listed once.
    std::fs::create_dir_all(home.path().join(".claude/skills")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        home.path().join(".agents/skills/better-ui"),
        home.path().join(".claude/skills/better-ui"),
    )
    .unwrap();

    let store = Store::load(fixture.path().join("skills.json")).unwrap();
    let found = local::discover(&workspace, home.path(), &store);
    let seen: Vec<(&str, &str, bool)> = found
        .iter()
        .map(|skill| {
            (
                skill.name.as_str(),
                skill.source.as_str(),
                skill.project_id.is_some(),
            )
        })
        .collect();
    // Without the link, as on Windows, the shared skill is found where it
    // lives instead.
    let expected = if cfg!(unix) {
        [
            ("better-ui", "Claude Code", false),
            ("release", "Codex", false),
            ("house-style", "Agent Skills", true),
        ]
    } else {
        [
            ("release", "Codex", false),
            ("better-ui", "Agent Skills", false),
            ("house-style", "Agent Skills", true),
        ]
    };
    assert_eq!(seen, expected);
    assert_eq!(found[2].project_id.as_deref(), Some(project.as_str()));
}
