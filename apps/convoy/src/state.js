// The store, and the one place that talks to the backend.
//
// No framework: the app re-renders from state on every change, and the pieces
// that cannot survive that — the terminals — live outside the rendered tree.

import { invoke } from "@tauri-apps/api/core";

export const state = {
  settings: {
    theme: "system",
    default_agent: "claude",
    font_size: 13,
    scrollback: 10000,
    claude_usage: false,
    notifications: false,
    keep_awake: "off",
    hibernate_minutes: 0,
    shortcuts: {},
  },
  projects: [],
  sessions: [],
  quickCommands: [],
  storage: "",
  running: [],
  projectId: null,
  sessionId: null,
  /// A second session shown beside the first. In memory only: which two
  /// terminals someone had open is not worth writing to the workspace.
  split: null,
  tab: "sessions",
  filter: "",
  search: "",
  showArchived: false,
  dialog: null,
  /// The Files & Changes view, when it is open.
  files: null,
  /// Specs and tasks for the selected project.
  planning: null,
  /// Projects whose queue is running. In memory only: a restart never resumes
  /// a queue on its own.
  queues: new Set(),
  /// What each running agent last reported, from its hooks.
  agentState: new Map(),
  // Projects the user folded shut in the sidebar.
  collapsed: new Set(),
  // Open terminal tabs, in the order shown, and what each one displays.
  tabs: [],
  tabInfo: {},
  // Branch and changed files per project, for detailed sidebar rows.
  projectGit: new Map(),
  // "settings" while the settings page replaces the window.
  page: null,
  settingsSection: "General",
  settingsQuery: "",
  capturing: null,
};

let onChange = () => {};
export const observe = (handler) => {
  onChange = handler;
};
export const changed = () => onChange();

// ------------------------------------------------------------------ toast --

const toastHost = () => document.querySelector("#toasts");

export function toast(message, kind = "info") {
  const host = toastHost();
  if (!host) return;
  const element = document.createElement("div");
  element.className = `toast${kind === "error" ? " toast--error" : ""}`;
  element.textContent = String(message);
  host.append(element);
  setTimeout(() => element.remove(), kind === "error" ? 8000 : 4000);
}

// ------------------------------------------------------------------- call --

// `undefined` means the call failed, never `null`: a command returning unit
// resolves to `null`, and using that as the failure signal would make a
// successful save look rejected.
export async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    toast(error, "error");
    return undefined;
  }
}

/// True when the call went through, for commands that return nothing.
export async function done(command, args) {
  return (await call(command, args)) !== undefined;
}

/// Like `call`, but the caller handles the failure itself.
export async function attempt(command, args) {
  try {
    return { ok: true, value: await invoke(command, args) };
  } catch (error) {
    return { ok: false, error: String(error) };
  }
}

// ----------------------------------------------------------------- lookups --

export const project = () =>
  state.projects.find((item) => item.id === state.projectId);
export const session = () =>
  state.sessions.find((item) => item.id === state.sessionId);
export const isRunning = (id) => state.running.includes(id);

/// Sessions of the selected project, filtered the way the list shows them.
export function visibleSessions() {
  const needle = state.filter.toLowerCase();
  return state.sessions.filter(
    (item) =>
      !needle ||
      `${item.title} ${item.provider_id}`.toLowerCase().includes(needle),
  );
}

// -------------------------------------------------------------- loading ---

export async function loadSettings() {
  const settings = await call("settings_read");
  if (!settings) return;
  state.settings = settings;
  state.storage = settings.storage;
  applyTheme();
}

export function applyTheme() {
  const root = document.documentElement;
  if (state.settings.theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", state.settings.theme);
}

/// `required` is for the first load: a workspace that cannot be read at all is
/// not a passing failure to toast, it is the reason the window is empty, and
/// the caller paints it. Later loads stay quiet so a transient failure does not
/// replace a working window.
export async function loadWorkspace({ required = false } = {}) {
  const result = await attempt("workspace_read");
  if (!result.ok) {
    if (required) throw new Error(result.error);
    toast(result.error, "bad");
    return;
  }
  const view = result.value;
  state.projects = view.projects;
  state.running = view.running;
  state.storage = view.storage;
  if (!state.projectId && view.projects.length) {
    state.projectId = view.projects[0].id;
  }
  if (state.projectId && !view.projects.some((p) => p.id === state.projectId)) {
    state.projectId = view.projects[0]?.id ?? null;
    state.sessionId = null;
  }
  await loadSessions();
}

export async function loadSessions() {
  if (!state.projectId) {
    state.sessions = [];
    return changed();
  }
  const sessions = await call("sessions_for", {
    projectId: state.projectId,
    archived: state.showArchived,
  });
  state.sessions = sessions ?? [];
  // A split pointing at a session that has been archived or removed would
  // render a pane for something that is not there.
  if (state.split && !state.sessions.some((item) => item.id === state.split)) {
    state.split = null;
  }
  if (state.split === state.sessionId) state.split = null;
  if (state.sessionId && !state.sessions.some((s) => s.id === state.sessionId)) {
    state.sessionId = null;
  }
  state.quickCommands =
    (await call("quick_commands_read", { projectId: state.projectId })) ?? [];
  changed();
}

export async function loadPlanning() {
  if (!state.projectId) return;
  state.planning = await call("planning_read", { projectId: state.projectId });
  changed();
}
