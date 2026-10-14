//! Roles: the parts an agent plays in a task — planner, builder, reviewer,
//! QA, documentation writer — each with its own instructions.
//!
//! A role's instructions come from the project, then the global settings,
//! then the built-in text. `reviewTemplate`, the older single field for the
//! reviewer, still counts as the review role's instructions wherever the new
//! field leaves that role unset, so nothing a person wrote is lost.

use crate::model::{Project, Role, Settings};

/// The planner's built-in instructions.
pub const PLAN: &str = "Plan this task before anything is built. Read the code it touches, then write a plan another engineer could follow: the approach, the files to change and why, the risks, and how the change will be verified. Keep it short and concrete.";

/// The QA agent's built-in instructions.
pub const QA: &str = "Test the change end to end against the task and its acceptance criteria: run the project's tests, and if it has a user interface or an API, use it the way a user would. Look for regressions near the change too.";

/// The documentation writer's built-in instructions.
pub const DOCS: &str = "Write the document that closes this task, for the people who test and integrate it: what changed and why, how to use it (endpoints, screens, settings), how it was verified, and the known risks. Write it as Markdown unless your instructions ask for another format.";

/// What a role does when nobody has said otherwise.
pub fn builtin(role: Role) -> &'static str {
    match role {
        Role::Plan => PLAN,
        Role::Build => "",
        Role::Review => crate::review::DEFAULT_TEMPLATE,
        Role::Qa => QA,
        Role::Docs => DOCS,
    }
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The project's own instructions for a role, if it has any.
pub fn project_instructions(project: &Project, role: Role) -> Option<&str> {
    filled(
        project
            .role_instructions
            .as_ref()
            .and_then(|roles| roles.get(&role))
            .map(String::as_str),
    )
    .or_else(|| match role {
        Role::Review => filled(project.review_template.as_deref()),
        _ => None,
    })
}

/// The global instructions for a role, if there are any.
pub fn global_instructions(settings: &Settings, role: Role) -> Option<&str> {
    filled(settings.role_instructions.get(&role).map(String::as_str)).or_else(|| match role {
        Role::Review => filled(Some(settings.review_template.as_str())),
        _ => None,
    })
}

/// A role's instructions in a project: the project's, the global ones, or
/// the built-in text, in that order. Empty for a builder nobody instructed.
pub fn brief(settings: &Settings, project: &Project, role: Role) -> String {
    project_instructions(project, role)
        .or_else(|| global_instructions(settings, role))
        .unwrap_or_else(|| builtin(role))
        .to_string()
}
