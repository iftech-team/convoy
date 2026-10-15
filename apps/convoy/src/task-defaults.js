// What a new task starts with, in one place: every way of making a task (the
// New task form, a spec, a doc, an import) reads its defaults from here, so
// the same project gives the same task whichever door it comes through.
//
// Pure: no imports, so the unit tests load it as it is. Labels are English
// source keys; the views put them through t().

/// Nothing leaves the machine unless the project opts in. The backend's own
/// default (`PublishMode::None`) is the same.
export const DEFAULT_TASK_MODE = "none";

/// How a finished task is published, in the order the choices are shown.
export const TASK_MODES = [
  ["none", "Don't publish"],
  ["pr", "Pull request"],
  ["push", "Push"],
];

/// The label for one mode, as an English key.
export const taskModeLabel = (mode) => TASK_MODES.find(([id]) => id === mode)?.[1] ?? TASK_MODES[0][1];

/// The project's publish mode, or the default when it never chose one.
export function taskModeOf(project) {
  const mode = project?.task_mode;
  return TASK_MODES.some(([id]) => id === mode) ? mode : DEFAULT_TASK_MODE;
}

/// The fields a new task takes from its project and the app's settings. The
/// model, effort and account are per machine and added by the caller.
export function newTaskDefaults(project, settings) {
  return {
    agent: project?.default_agent || settings?.default_agent || "claude",
    mode: taskModeOf(project),
    auto_review: !!project?.review_auto,
    plan_first: !!project?.plan_first_default,
  };
}

const AGENT_NAMES = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI" };

/// An agent's name as people know it. Product names, not translated.
export const agentName = (agent) => AGENT_NAMES[agent] ?? AGENT_NAMES.claude;
