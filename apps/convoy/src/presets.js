// Three ways to run a project's tasks, as starting points: Simple, Reviewed
// and Autopilot. A preset is not stored: it is read from the project's own
// settings, and one that matches none is Custom. Choosing a preset writes
// its settings in one go; any of them can then be changed one by one.
//
// Pure: no imports. Labels are English keys; views call t().

/// key → [name, what it does].
export const PRESETS = {
  simple: ["Simple", "An agent builds each task; you check it and mark it done."],
  reviewed: ["With review", "Another agent reviews each task, and the builder fixes what it finds before you look."],
  autopilot: ["Autopilot", "Planned, built, reviewed and tested by themselves, several at once, then published as a pull request."],
};
export const PRESET_ORDER = ["simple", "reviewed", "autopilot"];
export const CUSTOM = ["Custom", "Your own mix of the steps below."];

/// Whether the project can test tasks: checks to run or a QA agent, as the
/// core's `qa::configured` says.
export const canTest = (project) => !!(String(project?.test_commands ?? "").trim() || project?.qa_agent);
/// Whether it is a git repository, so it can have worktrees and publish.
export const isRepository = (project) => project?.is_repository !== false;

/// The settings a preset sets. A key it leaves out is not part of it: not
/// compared, not changed. `max_parallel` is set but never compared.
function settings(name, project) {
  const testing = canTest(project);
  const repository = isRepository(project);
  const ifTesting = testing ? { qa_auto: true } : {};
  const ifRepository = repository ? { convoy_publishes: true, task_worktrees: true } : {};
  switch (name) {
    case "simple":
      return {
        plan_first_default: false,
        review_auto: false,
        qa_auto: false,
        task_mode: "none",
        convoy_publishes: false,
        task_worktrees: false,
        auto_run_tasks: false,
      };
    case "reviewed":
      return { plan_first_default: false, review_auto: true, ...ifTesting, task_mode: "pr", ...ifRepository, auto_run_tasks: false };
    case "autopilot":
      return { plan_first_default: true, plan_auto_approve: true, review_auto: true, ...ifTesting, task_mode: "pr", ...ifRepository, auto_run_tasks: true };
    default:
      return null;
  }
}

const value = (project, key) => (key === "task_mode" ? project?.task_mode || "none" : !!project?.[key]);

/// Which preset the project's settings are, or "custom".
export function presetOf(project) {
  for (const name of PRESET_ORDER) {
    const wanted = settings(name, project);
    if (Object.entries(wanted).every(([key, want]) => value(project, key) === want)) return name;
  }
  return "custom";
}

/// The one change that makes the project this preset.
export function presetPatch(name, project) {
  const patch = settings(name, project);
  if (!patch) return null;
  if (name === "autopilot") patch.max_parallel = isRepository(project) ? 2 : 1;
  return patch;
}
