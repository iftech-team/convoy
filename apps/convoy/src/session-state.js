// One word for where a session stands, used by every place that shows one:
// the sidebar, tabs, Home, the dashboard, helper cards, the palette. Before,
// each drew its own from the process and the agent's last report, and the
// same session read "Running", "working" and "Working" side by side.
//
// Pure: no imports. Labels are English keys; the views put them through t().

/// key → [label, dot tone]. The tone names the `dot--…` class.
export const SESSION_STATES = {
  not_started: ["Not started", "idle"],
  working: ["Working", "running"],
  // "Needs you" names the task phase and the inbox; a session is waiting.
  needs_you: ["Waiting for you", "waiting"],
  // A finished turn and an idle agent look the same from outside: the agent
  // is there, doing nothing until it is given more.
  finished: ["Idle", "done"],
  idle: ["Idle", "done"],
  // Closed, not stopped: Resume opens it again where it left off.
  stopped: ["Closed", "stopped"],
  asleep: ["Asleep", "asleep"],
};

/// Where a session stands. `running` is whether its process runs, `agent`
/// what its agent last reported (idle, working, waiting, done, ended, or
/// nothing — Codex in a terminal reports nothing), `asleep` whether Convoy
/// put it to sleep. A running agent that reports nothing is working.
export function sessionStateOf(item, { running, agent, asleep } = {}) {
  if (!running) {
    if (!item?.started) return "not_started";
    return asleep ? "asleep" : "stopped";
  }
  if (agent === "waiting") return "needs_you";
  if (agent === "done") return "finished";
  if (agent === "idle" || agent === "ended") return "idle";
  return "working";
}

export const sessionLabel = (key) => (SESSION_STATES[key] ?? SESSION_STATES.working)[0];
export const sessionTone = (key) => (SESSION_STATES[key] ?? SESSION_STATES.working)[1];

/// Whether an activity event says the session was put to sleep: Convoy's own
/// idle sleep records `hibernated`; a sleep from the menu ends the process
/// with this detail.
export const sleptBy = (event) =>
  event?.kind === "hibernated" || (event?.kind === "exited" && String(event.detail ?? "").startsWith("Hibernated"));
