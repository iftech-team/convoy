//! The skills library, as the window edits it and the launches read it.
//!
//! The rules are `convoy_core::skills`; this keeps one writer of
//! `skills.json` at a time and takes a removed skill off everything it was
//! attached to.

use convoy_core::model::Agent;
use convoy_core::skills::{Skill, SkillInput, Store};
use convoy_core::Storage;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::commands::Workspace;

/// One writer of `skills.json` at a time.
static FILE: Mutex<()> = Mutex::new(());

/// Loads the file, applies a change and lets the store write it.
pub fn with_skills<T>(
    storage: &Storage,
    change: impl FnOnce(&mut Store) -> convoy_core::Result<T>,
) -> Result<T, String> {
    let _guard = FILE.lock().map_err(|_| "Skills unavailable.")?;
    let mut store = Store::load(storage.skills()).map_err(|error| error.to_string())?;
    change(&mut store).map_err(|error| error.to_string())
}

/// What a new session, task or automation of this project starts with: the
/// list asked for, checked; or, when none was given, the automatic ones.
pub fn resolve(
    storage: &Storage,
    project_id: &str,
    agent: Option<Agent>,
    asked: Option<&[String]>,
) -> Result<Vec<String>, String> {
    with_skills(storage, |store| match asked {
        Some(ids) => store.check(project_id, ids),
        None => Ok(store.defaults(project_id, agent)),
    })
}

/// Takes skills that are gone off every session, task and automation.
fn detach(workspace: &Workspace, ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let storage = workspace.storage.clone();
    workspace.with(|core| {
        crate::automations::with_store(&storage, |automations| {
            convoy_core::skills::detach_everywhere(core, automations, ids)
        })
    })
}

/// A project was removed: its own skills go with it.
pub fn forget_project(workspace: &Workspace, project_id: &str) {
    if let Ok(ids) = with_skills(&workspace.storage, |store| store.remove_project(project_id)) {
        let _ = detach(workspace, &ids);
    }
}

/// Every project was removed: global skills stay.
pub fn forget_all(workspace: &Workspace) {
    let _ = with_skills(&workspace.storage, |store| store.remove_all_projects());
}

fn now() -> i64 {
    crate::automations::now()
}

/// Every skill, global ones and every project's.
#[tauri::command]
pub fn skills_list(workspace: State<'_, Workspace>) -> Result<Vec<Skill>, String> {
    let storage = workspace.storage.clone();
    let (list, pruned) = workspace.with(|core| {
        with_skills(&storage, |store| {
            let pruned = store.prune(core)?;
            Ok((store.all().to_vec(), pruned))
        })
    })?;
    detach(&workspace, &pruned)?;
    Ok(list)
}

/// Creates or edits one. Returns its id.
#[tauri::command]
pub fn skill_save<R: Runtime>(
    app: AppHandle<R>,
    input: SkillInput,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let storage = workspace.storage.clone();
    let id =
        workspace.with(|core| with_skills(&storage, |store| store.save(input, core, now())))?;
    let _ = app.emit("skills:changed", id.clone());
    Ok(id)
}

/// Removes one, and takes it off everything it was attached to.
#[tauri::command]
pub fn skill_delete<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    with_skills(&workspace.storage, |store| store.remove(&id))?;
    detach(&workspace, std::slice::from_ref(&id))?;
    let _ = app.emit("skills:changed", id);
    Ok(())
}

/// The skills a session starts with from its next start. Allowed while it
/// runs; the running agent keeps what it was started with.
#[tauri::command]
pub fn session_skills(
    id: String,
    skills: Vec<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let project = workspace.act(|core| Ok(core.session(&id)?.project_id.clone()))?;
    let skills = resolve(&workspace.storage, &project, None, Some(&skills))?;
    workspace.act(|core| core.set_session_skills(&id, skills).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use convoy_core::automations::{AutomationInput, Schedule};
    use convoy_core::workspace::{NewSession, SettingsPatch};
    use convoy_core::Workspace as CoreWorkspace;
    use tauri::Manager;

    fn app() -> (
        tempfile::TempDir,
        tauri::App<tauri::test::MockRuntime>,
        String,
    ) {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        std::fs::create_dir_all(&folder).unwrap();
        let storage = Storage::new(root.path().join("state"));
        let project = {
            let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
            core.save_settings(SettingsPatch {
                auto_trust: Some(false),
                ..Default::default()
            })
            .unwrap();
            core.add_project(&folder).unwrap();
            core.state().projects[0].id.clone()
        };
        let app = tauri::test::mock_app();
        app.manage(Workspace::at(storage));
        (root, app, project)
    }

    fn input(name: &str, project: Option<&str>, auto: bool) -> SkillInput {
        SkillInput {
            id: None,
            name: name.into(),
            description: String::new(),
            body: format!("Do {name}."),
            project_id: project.map(str::to_string),
            agents: "both".into(),
            auto,
            roles: None,
        }
    }

    /// Automatic skills are what a new thing gets when it asks for none; a
    /// list asked for is checked. A deleted skill comes off the sessions and
    /// automations that had it, and a removed project takes its own skills.
    #[test]
    fn defaults_checks_and_deletes_reach_everything() {
        let (_root, app, project) = app();
        let workspace = app.state::<Workspace>();
        let handle = app.handle().clone();
        let auto =
            skill_save(handle.clone(), input("Auto", None, true), workspace.clone()).unwrap();
        let own = skill_save(
            handle.clone(),
            input("Own", Some(&project), false),
            workspace.clone(),
        )
        .unwrap();
        assert_eq!(
            resolve(&workspace.storage, &project, Some(Agent::Codex), None).unwrap(),
            std::slice::from_ref(&auto)
        );
        assert!(resolve(&workspace.storage, &project, None, Some(&["nope".into()])).is_err());

        let session = workspace
            .act(|core| {
                let mut input = NewSession::new(project.clone(), Agent::Claude, "One");
                input.skills = vec![auto.clone()];
                core.add_session(input)?;
                Ok(core.state().sessions[0].id.clone())
            })
            .unwrap();
        session_skills(
            session.clone(),
            vec![own.clone(), auto.clone()],
            workspace.clone(),
        )
        .unwrap();
        let storage = workspace.storage.clone();
        let automation = workspace
            .with(|core| {
                crate::automations::with_store(&storage, |store| {
                    store.save(
                        AutomationInput {
                            skills: Some(vec![own.clone()]),
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
                            catch_up: true,
                        },
                        core,
                        1,
                    )
                })
            })
            .unwrap();

        skill_delete(handle, own.clone(), workspace.clone()).unwrap();
        let attached = workspace
            .act(|core| Ok(core.session(&session)?.skills.clone()))
            .unwrap();
        assert_eq!(attached, Some(vec![auto.clone()]));
        let kept = crate::automations::with_store(&storage, |store| {
            Ok(store.get(&automation)?.skills.clone())
        })
        .unwrap();
        assert_eq!(kept, None);
        assert_eq!(skills_list(workspace.clone()).unwrap().len(), 1);

        forget_all(&workspace);
        assert_eq!(
            skills_list(workspace.clone()).unwrap().len(),
            1,
            "global stays"
        );
    }
}
