//! Skills: named Markdown instructions — "small focused commits", "write
//! tests first" — kept once and attached to sessions, tasks and automations.
//!
//! A skill applies everywhere or in one project, to Claude, Codex or both.
//! One marked `auto` is attached by default to new sessions, tasks and
//! automations in its scope. When a session starts, the skills attached to
//! it that suit its agent become instructions: Claude's appended system
//! prompt, Codex's developer instructions — joined with any other
//! instructions the launch carries into one value, since a second flag would
//! replace the first.
//!
//! They live in a file of their own beside the workspace, `skills.json`, not
//! in `workspace.json`, for the reason automations do: the macOS app rewrites
//! that file whole and drops keys it does not know. The attachments are ids
//! on the sessions, tasks and automations themselves.

use crate::automations::Instant;
use crate::json::utf16_len;
use crate::workspace::model::{Agent, Role};
use crate::workspace::{new_id, Workspace};
use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const FILE_VERSION: u32 = 1;

/// The most a skill's body may hold, in UTF-16 units as the window counts.
pub const MAX_BODY: usize = 16_000;
pub const MAX_NAME: usize = 80;
pub const MAX_DESCRIPTION: usize = 300;
/// The most one session may carry, in bytes of the instructions they become.
pub const MAX_INSTRUCTIONS: usize = 32 * 1024;
/// Skills one session, task or automation may have attached.
pub const MAX_ATTACHED: usize = 32;

/// Which agents a skill is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agents {
    Claude,
    Codex,
    #[default]
    Both,
}

impl Agents {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Agents::Claude),
            "codex" => Some(Agents::Codex),
            "both" | "" => Some(Agents::Both),
            _ => None,
        }
    }

    pub fn includes(self, agent: Agent) -> bool {
        match self {
            Agents::Both => true,
            Agents::Claude => agent == Agent::Claude,
            Agents::Codex => agent == Agent::Codex,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Markdown, given to the agent as written.
    pub body: String,
    /// The project it belongs to; none is every project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub agents: Agents,
    /// Attached by default to new sessions, tasks and automations in its scope.
    #[serde(default)]
    pub auto: bool,
    /// The task roles it is given to; none is the builder only, as skills
    /// always were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<Role>>,
    /// The projects it belongs to, when there are several; then
    /// `project_id` is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<String>>,
    pub created: Instant,
    #[serde(default)]
    pub updated: Instant,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Skill {
    /// Whether a session of this project may have it.
    pub fn in_scope(&self, project_id: &str) -> bool {
        if let Some(projects) = self.projects.as_ref().filter(|list| !list.is_empty()) {
            return projects.iter().any(|own| own == project_id);
        }
        self.project_id
            .as_deref()
            .is_none_or(|own| own == project_id)
    }

    /// The projects it is limited to; empty is every project.
    pub fn scope(&self) -> Vec<String> {
        match (&self.projects, &self.project_id) {
            (Some(projects), _) if !projects.is_empty() => projects.clone(),
            (_, Some(project)) => vec![project.clone()],
            _ => Vec::new(),
        }
    }

    /// Whether it is given to this agent in this project.
    pub fn applies(&self, agent: Agent, project_id: &str) -> bool {
        self.agents.includes(agent) && self.in_scope(project_id)
    }

    /// Whether it is for this role in a task.
    pub fn for_role(&self, role: Role) -> bool {
        match &self.roles {
            Some(roles) => roles.contains(&role),
            None => role == Role::Build,
        }
    }
}

/// What the editor sends.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SkillInput {
    /// None to create one.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub body: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub agents: String,
    #[serde(default)]
    pub auto: bool,
    /// The task roles it is for; none or empty is the builder only.
    #[serde(default)]
    pub roles: Option<Vec<Role>>,
    /// Several projects it belongs to; when given, `project_id` is ignored.
    #[serde(default)]
    pub projects: Option<Vec<String>>,
}

/// The scope a skill is saved with: one project in `project_id`, several in
/// `projects`, none for every project.
fn scope_kept(input: &SkillInput) -> (Option<String>, Option<Vec<String>>) {
    let mut projects: Vec<String> = Vec::new();
    for id in input.projects.clone().unwrap_or_default() {
        if !id.is_empty() && !projects.contains(&id) {
            projects.push(id);
        }
    }
    match projects.len() {
        0 => (input.project_id.clone().filter(|id| !id.is_empty()), None),
        1 => (projects.pop(), None),
        _ => (None, Some(projects)),
    }
}

/// A role list as kept: none for the builder alone, without repeats.
fn roles_kept(roles: Option<Vec<Role>>) -> Option<Vec<Role>> {
    let mut unique: Vec<Role> = Vec::new();
    for role in roles.unwrap_or_default() {
        if !unique.contains(&role) {
            unique.push(role);
        }
    }
    (!unique.is_empty() && unique != [Role::Build]).then_some(unique)
}

/// Checks what the editor sent. `others` are the skills already kept, so a
/// name is never taken twice: a manager names skills when it starts helpers.
fn validate(input: &SkillInput, workspace: &Workspace, others: &[Skill]) -> Result<Agents> {
    let name = input.name.trim();
    ensure!(!name.is_empty(), "Give the skill a name.");
    ensure!(
        utf16_len(name) <= MAX_NAME && !name.contains(['\n', '\r']),
        "The name is too long (80 characters at most, on one line)."
    );
    ensure!(
        utf16_len(input.description.trim()) <= MAX_DESCRIPTION,
        "The description is too long (300 characters at most)."
    );
    let body = input.body.trim();
    ensure!(!body.is_empty(), "Write what the skill tells the agent.");
    ensure!(
        utf16_len(body) <= MAX_BODY,
        "The skill is too long (16,000 characters at most)."
    );
    if let Some(project) = input.project_id.as_deref().filter(|id| !id.is_empty()) {
        workspace.project(project)?;
    }
    for project in input.projects.iter().flatten().filter(|id| !id.is_empty()) {
        workspace.project(project)?;
    }
    let taken = others.iter().any(|skill| {
        Some(&skill.id) != input.id.as_ref() && skill.name.to_lowercase() == name.to_lowercase()
    });
    ensure!(!taken, "A skill with this name already exists.");
    Agents::parse(&input.agents).ok_or_else(|| ConvoyError::message("Pick Claude, Codex or both."))
}

// ---------------------------------------------------------------------------
// The file.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsFile {
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for SkillsFile {
    fn default() -> Self {
        SkillsFile {
            version: FILE_VERSION,
            skills: Vec::new(),
            unknown: Map::new(),
        }
    }
}

/// `skills.json`, loaded once and written whole, atomically.
#[derive(Debug)]
pub struct Store {
    file: PathBuf,
    state: SkillsFile,
}

impl Store {
    pub fn load(file: impl Into<PathBuf>) -> Result<Store> {
        let file = file.into();
        let state = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str::<SkillsFile>(&text).map_err(|_| {
                ConvoyError::message(format!("The skills file is damaged: {}", file.display()))
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SkillsFile::default(),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            state.version <= FILE_VERSION,
            "The skills file was written by a newer Convoy."
        );
        Ok(Store { file, state })
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn all(&self) -> &[Skill] {
        &self.state.skills
    }

    pub fn get(&self, id: &str) -> Result<&Skill> {
        self.state
            .skills
            .iter()
            .find(|skill| skill.id == id)
            .ok_or_else(|| ConvoyError::message("Skill not found."))
    }

    /// Applies a change and writes the file; a failed write changes nothing.
    pub fn update<T>(&mut self, change: impl FnOnce(&mut SkillsFile) -> Result<T>) -> Result<T> {
        let mut next = self.state.clone();
        let value = change(&mut next)?;
        let body = serde_json::to_string_pretty(&next)?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temporary = self.file.clone().into_os_string();
        temporary.push(format!(".{}.tmp", Uuid::new_v4()));
        let temporary = PathBuf::from(temporary);
        let outcome = (|| -> Result<()> {
            let mut handle = crate::fs::create_private(&temporary)?;
            std::io::Write::write_all(&mut handle, body.as_bytes())?;
            handle.sync_all()?;
            drop(handle);
            fs::rename(&temporary, &self.file)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        outcome?;
        self.state = next;
        Ok(value)
    }

    /// Creates or edits one. Returns its id.
    pub fn save(
        &mut self,
        input: SkillInput,
        workspace: &Workspace,
        now: Instant,
    ) -> Result<String> {
        let agents = validate(&input, workspace, &self.state.skills)?;
        let (project_id, projects) = scope_kept(&input);
        let (name, description, body) = (
            input.name.trim().to_string(),
            input.description.trim().to_string(),
            input.body.trim().to_string(),
        );
        match input.id.clone() {
            Some(id) => {
                self.get(&id)?;
                self.update(move |file| {
                    let skill = file
                        .skills
                        .iter_mut()
                        .find(|skill| skill.id == id)
                        .ok_or_else(|| ConvoyError::message("Skill not found."))?;
                    skill.name = name;
                    skill.description = description;
                    skill.body = body;
                    skill.project_id = project_id;
                    skill.projects = projects;
                    skill.agents = agents;
                    skill.auto = input.auto;
                    skill.roles = roles_kept(input.roles.clone());
                    skill.updated = now;
                    Ok(id.clone())
                })
            }
            None => {
                let id = new_id();
                let skill = Skill {
                    id: id.clone(),
                    name,
                    description,
                    body,
                    project_id,
                    agents,
                    auto: input.auto,
                    roles: roles_kept(input.roles.clone()),
                    projects,
                    created: now,
                    updated: now,
                    unknown: Map::new(),
                };
                self.update(move |file| {
                    file.skills.push(skill);
                    Ok(())
                })?;
                Ok(id)
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.get(id)?;
        let id = id.to_string();
        self.update(move |file| {
            file.skills.retain(|skill| skill.id != id);
            Ok(())
        })
    }

    /// A project went: so do its own skills. Returns their ids, to detach.
    pub fn remove_project(&mut self, project_id: &str) -> Result<Vec<String>> {
        // A skill of several projects only loses this one, unless it was
        // its last.
        let only = |skill: &Skill| {
            let scope = skill.scope();
            scope.len() == 1 && scope[0] == project_id
        };
        let gone: Vec<String> = self
            .state
            .skills
            .iter()
            .filter(|skill| only(skill))
            .map(|skill| skill.id.clone())
            .collect();
        let shared = self.state.skills.iter().any(|skill| {
            skill
                .projects
                .as_ref()
                .is_some_and(|list| list.len() > 1 && list.iter().any(|id| id == project_id))
        });
        if gone.is_empty() && !shared {
            return Ok(gone);
        }
        let project_id = project_id.to_string();
        let removed = gone.clone();
        self.update(move |file| {
            file.skills.retain(|skill| !removed.contains(&skill.id));
            for skill in file.skills.iter_mut() {
                if let Some(list) = skill.projects.as_mut() {
                    list.retain(|id| *id != project_id);
                }
            }
            Ok(gone)
        })
    }

    /// Every project went: project skills go, global ones stay.
    pub fn remove_all_projects(&mut self) -> Result<Vec<String>> {
        let gone: Vec<String> = self
            .state
            .skills
            .iter()
            .filter(|skill| !skill.scope().is_empty())
            .map(|skill| skill.id.clone())
            .collect();
        if gone.is_empty() {
            return Ok(gone);
        }
        self.update(move |file| {
            file.skills.retain(|skill| skill.scope().is_empty());
            Ok(gone)
        })
    }

    /// Drops skills whose project is no longer in the workspace — one removed
    /// by another build, which knows nothing of this file. Returns their ids.
    pub fn prune(&mut self, workspace: &Workspace) -> Result<Vec<String>> {
        let known = |id: &str| workspace.state().projects.iter().any(|p| p.id == id);
        let gone: Vec<String> = self
            .state
            .skills
            .iter()
            .filter(|skill| {
                let scope = skill.scope();
                !scope.is_empty() && scope.iter().all(|id| !known(id))
            })
            .map(|skill| skill.id.clone())
            .collect();
        if gone.is_empty() {
            return Ok(gone);
        }
        let ids = gone.clone();
        self.update(move |file| {
            file.skills.retain(|skill| !ids.contains(&skill.id));
            Ok(gone)
        })
    }

    /// The `auto` skills in a project's scope, for a new session, task or
    /// automation; with `agent`, only those for it.
    pub fn defaults(&self, project_id: &str, agent: Option<Agent>) -> Vec<String> {
        self.state
            .skills
            .iter()
            .filter(|skill| skill.auto && skill.in_scope(project_id))
            .filter(|skill| skill.for_role(Role::Build))
            .filter(|skill| agent.is_none_or(|agent| skill.agents.includes(agent)))
            .map(|skill| skill.id.clone())
            .collect()
    }

    /// The `auto` skills of a role in a task, for this agent in this
    /// project, within the attachment limit.
    pub fn for_role(&self, project_id: &str, agent: Agent, role: Role) -> Vec<String> {
        self.state
            .skills
            .iter()
            .filter(|skill| skill.auto && skill.applies(agent, project_id) && skill.for_role(role))
            .map(|skill| skill.id.clone())
            .take(MAX_ATTACHED)
            .collect()
    }

    /// Checks a list to attach in a project: every id known and in scope.
    /// Returns it without repeats, in its order.
    pub fn check(&self, project_id: &str, ids: &[String]) -> Result<Vec<String>> {
        ensure!(ids.len() <= MAX_ATTACHED, "Too many skills attached.");
        let mut unique: Vec<String> = Vec::new();
        for id in ids {
            let skill = self.get(id)?;
            ensure!(
                skill.in_scope(project_id),
                "“{}” belongs to another project.",
                skill.name
            );
            if !unique.contains(id) {
                unique.push(id.clone());
            }
        }
        Ok(unique)
    }

    /// The ids of skills named, for a manager's `--skill`, as they apply to
    /// a helper of this agent in this project.
    pub fn named(&self, project_id: &str, agent: Agent, names: &[String]) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for name in names {
            let wanted = name.trim().to_lowercase();
            let Some(skill) = self
                .state
                .skills
                .iter()
                .find(|skill| skill.name.to_lowercase() == wanted)
            else {
                bail!("No skill is named “{}”.", name.trim());
            };
            ensure!(
                skill.applies(agent, project_id),
                "The skill “{}” is not for {} in this project.",
                skill.name,
                match agent {
                    Agent::Claude => "Claude Code",
                    Agent::Codex => "Codex",
                }
            );
            if !ids.contains(&skill.id) {
                ids.push(skill.id.clone());
            }
        }
        Ok(ids)
    }
}

// ---------------------------------------------------------------------------
// Instructions.
// ---------------------------------------------------------------------------

const HEADER: &str = "Skills the person attached to this session in Convoy. Follow them throughout the conversation unless the person says otherwise.";

/// The instructions a session's attached skills become: those that apply to
/// its agent and project, in the order they were attached. Ids no longer in
/// the store are passed over. None when nothing applies.
pub fn instructions(
    skills: &[Skill],
    attached: &[String],
    agent: Agent,
    project_id: &str,
) -> Result<Option<String>> {
    let picked: Vec<&Skill> = attached
        .iter()
        .filter_map(|id| skills.iter().find(|skill| &skill.id == id))
        .filter(|skill| skill.applies(agent, project_id))
        .collect();
    if picked.is_empty() {
        return Ok(None);
    }
    let mut text = String::from(HEADER);
    for skill in picked {
        text.push_str("\n\n## ");
        text.push_str(skill.name.trim());
        text.push('\n');
        text.push_str(skill.body.trim());
    }
    ensure!(
        text.len() <= MAX_INSTRUCTIONS,
        "The skills attached to this session are too long together ({} KB; 32 KB at most). Detach some or shorten them.",
        text.len().div_ceil(1024)
    );
    Ok(Some(text))
}

/// Everything a launch tells the agent beyond its prompt, as one value: an
/// agent takes one appended system prompt or one set of developer
/// instructions, and a second would replace the first.
pub fn combine(parts: &[Option<String>]) -> Option<String> {
    let parts: Vec<&str> = parts
        .iter()
        .flatten()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// The skills a session launches with, as instructions, read from the
/// skills file. A session with none attached never reads it.
pub fn for_session(
    storage: &crate::Storage,
    session: &crate::model::Session,
) -> Result<Option<String>> {
    let Some(attached) = session.skills.as_deref().filter(|ids| !ids.is_empty()) else {
        return Ok(None);
    };
    let store = Store::load(storage.skills())?;
    instructions(store.all(), attached, session.agent, &session.project_id)
}

/// Removes skills from everything that had them attached: sessions and
/// tasks in the workspace, and automations. Removing a skill calls this, as
/// does removing a project with skills of its own.
pub fn detach_everywhere(
    workspace: &mut Workspace,
    automations: &mut crate::automations::Store,
    ids: &[String],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    workspace.detach_skills(ids)?;
    automations.detach_skills(ids)?;
    Ok(())
}

pub mod local;

#[cfg(test)]
mod tests;
