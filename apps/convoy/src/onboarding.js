// The four things a first run needs, as a checklist: an agent installed, it
// logged in, a project, and a first task. Shown on Home, in the empty
// window and atop Settings → Get started until it is done or put away.
//
// Pure: no imports. Labels are English keys; views call t().

const AGENTS = ["claude", "codex", "gemini"];
const NAMES = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI" };

/// Every label the checklist shows, for the translation check.
export const CHECKLIST_LABELS = [
  "Get started",
  "Install an agent",
  "Claude Code, Codex or Gemini CLI, found on your PATH.",
  "Found {agents}.",
  "Log in to it",
  "Checking…",
  "Run the agent once in a terminal and sign in.",
  "Logged in.",
  "Open a project",
  "A repository, or a folder of related ones.",
  "{count} open.",
  "Create a task",
  "Write one, or import one from your tracker; an agent runs it.",
  "{count} so far.",
  "{done} of {total} done",
];

/// The checklist from what the window has read: the tools found
/// (`diagnostics`), the setup checks with their login checks, the projects
/// and the tasks. Unread data counts as not done yet.
export function checklist({ diagnostics, setupChecks, projects, tasks } = {}) {
  const found = AGENTS.filter((agent) => (diagnostics ?? []).some((tool) => tool.name === agent && tool.found));
  const logins = found
    .filter((agent) => agent !== "gemini")
    .map((agent) => (setupChecks ?? []).find((check) => check.id === `login.${agent}`))
    .filter(Boolean);
  // Gemini has no login check of its own: found is enough.
  const loggedIn = found.includes("gemini") || logins.some((check) => check.status === "ok");
  const items = [
    {
      key: "agent",
      label: "Install an agent",
      detail: found.length ? ["Found {agents}.", { agents: found.map((agent) => NAMES[agent]).join(", ") }] : ["Claude Code, Codex or Gemini CLI, found on your PATH.", {}],
      done: found.length > 0,
      action: "setup",
    },
    {
      key: "login",
      label: "Log in to it",
      detail: !setupChecks && found.length ? ["Checking…", {}] : loggedIn ? ["Logged in.", {}] : ["Run the agent once in a terminal and sign in.", {}],
      done: found.length > 0 && loggedIn,
      action: "setup",
    },
    {
      key: "project",
      label: "Open a project",
      detail: projects?.length ? ["{count} open.", { count: projects.length }] : ["A repository, or a folder of related ones.", {}],
      done: (projects?.length ?? 0) > 0,
      action: "open-folder",
    },
    {
      key: "task",
      label: "Create a task",
      detail: tasks?.length ? ["{count} so far.", { count: tasks.length }] : ["Write one, or import one from your tracker; an agent runs it.", {}],
      done: (tasks?.length ?? 0) > 0,
      action: "first-task",
    },
  ];
  const done = items.filter((item) => item.done).length;
  return { items, done, total: items.length, complete: done === items.length };
}
