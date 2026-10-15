// The four things a first run needs, as a checklist: an agent installed, it
// logged in, a project, and a first task. Shown on the welcome screen, on
// Home and atop Settings → Get started until it is done or put away.
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
  "Sign in to {agent} here, in its own login.",
  "Log in to {agent}",
  "Logged in.",
  "Open a project",
  "A repository, or a folder of related ones.",
  "{count} open.",
  "Create a task",
  "One sentence is enough; it starts right away.",
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
  // The agent to sign in from the window: the first found one whose login
  // check failed. Gemini signs in on its own first run.
  const signIn = loggedIn ? null : found.find((agent) => agent !== "gemini" && logins.some((check) => check.id === `login.${agent}`));
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
      detail: !setupChecks && found.length
        ? ["Checking…", {}]
        : loggedIn
          ? ["Logged in.", {}]
          : signIn
            ? ["Sign in to {agent} here, in its own login.", { agent: NAMES[signIn] }]
            : ["Run the agent once in a terminal and sign in.", {}],
      done: found.length > 0 && loggedIn,
      action: signIn ? "login" : "setup",
      // What the step's button says and signs in to, when it can.
      agent: signIn,
      button: signIn ? ["Log in to {agent}", { agent: NAMES[signIn] }] : null,
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
      detail: tasks?.length ? ["{count} so far.", { count: tasks.length }] : ["One sentence is enough; it starts right away.", {}],
      done: (tasks?.length ?? 0) > 0,
      action: "first-task",
    },
  ];
  const done = items.filter((item) => item.done).length;
  return { items, done, total: items.length, complete: done === items.length };
}

// The welcome screen's words: what Convoy is, and how a task moves through it.

/// What Convoy does, in one sentence. Home repeats it.
export const PITCH =
  "Convoy runs coding agents on your tasks: they plan, build, review and test the work and open a pull request, and you approve and merge.";

/// How one task moves through Convoy, in three steps.
export const FLOW = [
  ["edit", "You write a task", "One sentence is enough: fix a bug, add a page, update the docs."],
  ["sparkles", "Agents do the work", "Claude Code, Codex or Gemini CLI plan, build, review each other and test, each in a session you can open."],
  ["checkCircle", "You approve and merge", "Look at the change, then merge the pull request or send it back with notes."],
];

/// Every label this screen shows, for the translation check.
export const WELCOME_LABELS = [
  PITCH,
  ...FLOW.flatMap(([, title, text]) => [title, text]),
  "Welcome to Convoy",
  "How it works",
  "Open a project…",
  "A repository, or a folder of related ones. Nothing in it changes until you run a task.",
  "Used Convoy for macOS? Import its workspace…",
];
