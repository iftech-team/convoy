// The store, and the one place that talks to the backend.
//
// No framework: the app re-renders from state on every change, and the pieces
// that cannot survive that — the terminals — live outside the rendered tree.

import { invoke } from "@tauri-apps/api/core";

const COLLAPSED = "convoy.collapsedProjects";

function remembered(key) {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

export function saveCollapsed() {
  try {
    localStorage.setItem(COLLAPSED, JSON.stringify([...state.collapsed]));
  } catch {
    /* not remembered */
  }
}

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
  /// Every project's sessions that are not archived. `sessions` is the
  /// selected project's, archived ones included when they are shown.
  allSessions: [],
  quickCommands: [],
  storage: "",
  running: [],
  projectId: null,
  sessionId: null,
  /// A second session shown beside the first. In memory only: which two
  /// terminals someone had open is not worth writing to the workspace.
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
  /// When each terminal session started waiting, for the approvals inbox.
  waitingSince: new Map(),
  /// Inbox rows showing their full detail.
  inboxExpanded: new Set(),
  // Projects the user folded shut in the sidebar, kept across launches.
  collapsed: new Set(remembered(COLLAPSED)),
  // Groups as folding rows (on) or a flat list (off); per machine.
  groupHierarchy: (() => {
    try {
      return localStorage.getItem("convoy.groupHierarchy") !== "0";
    } catch {
      return true;
    }
  })(),
  // Sessions picked with ⌘-click (Ctrl-click elsewhere) in the sidebar.
  selected: new Set(),
  /// Rows picked to delete on the Sessions list (`s:<id>`, `c:<agent>:<id>`);
  /// null when not picking.
  picked: null,
  // Open terminal tabs, in the order shown, and what each one displays.
  tabs: [],
  tabInfo: {},
  // 1, 2 or 4 panes; what each shows; which one has focus.
  layout: 1,
  panes: [],
  focus: 0,
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

/// The models of an agent for an account, read once and kept: the sheets'
/// pickers, and a chat's before its agent has listed its own.
const modelsLoading = new Set();
export function ensureModels(agent, profileId = "") {
  const key = `${agent}:${profileId ?? ""}`;
  state.models ??= {};
  if (state.models[key] || modelsLoading.has(key)) return;
  modelsLoading.add(key);
  call("models_list", { agent, profileId: profileId || null }).then((models) => {
    modelsLoading.delete(key);
    state.models[key] = models ?? [];
    changed();
  });
}

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
export const anySession = (id) =>
  state.sessions.find((item) => item.id === id) ??
  state.allSessions.find((item) => item.id === id);

/// Sessions of the selected project, filtered the way the list shows them.
export function visibleSessions() {
  const needle = state.filter.toLowerCase();
  return state.sessions.filter(
    (item) =>
      !needle ||
      `${item.title} ${item.provider_id}`.toLowerCase().includes(needle),
  );
}

/// The account last used per agent, which new sessions start with, as the
/// macOS app's "active" account. Per machine; an empty string is the system
/// login.
const ACCOUNT_KEY = "convoy.activeAccount";

export function activeAccount(agent) {
  try {
    const id = JSON.parse(localStorage.getItem(ACCOUNT_KEY) ?? "{}")?.[agent] ?? "";
    return (state.profiles ?? []).some((item) => item.id === id && item.agent === agent) ? id : "";
  } catch {
    return "";
  }
}

export function rememberAccount(agent, id) {
  try {
    const saved = JSON.parse(localStorage.getItem(ACCOUNT_KEY) ?? "{}") ?? {};
    saved[agent] = id || "";
    localStorage.setItem(ACCOUNT_KEY, JSON.stringify(saved));
  } catch {
    /* not remembered */
  }
}

/// The model and effort last picked per agent, as Claude Code and Codex keep
/// the last /model: every new session and task starts with them, and what is
/// picked in one dialog belongs to that session or task alone. Per machine.
const CHOICE_KEY = "convoy.lastModel";

export function lastChoice(agent) {
  try {
    const saved = JSON.parse(localStorage.getItem(CHOICE_KEY) ?? "{}")?.[agent] ?? {};
    return {
      model: typeof saved.model === "string" && saved.model !== "__loading" ? saved.model : "",
      effort: typeof saved.effort === "string" ? saved.effort : "",
    };
  } catch {
    return { model: "", effort: "" };
  }
}

export function rememberChoice(agent, model, effort) {
  try {
    const saved = JSON.parse(localStorage.getItem(CHOICE_KEY) ?? "{}") ?? {};
    saved[agent] = { model: model && model !== "__loading" ? model : "", effort: effort || "" };
    localStorage.setItem(CHOICE_KEY, JSON.stringify(saved));
  } catch {
    /* not remembered */
  }
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
  state.allSessions = view.sessions ?? [];
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
  // The selected project's list is the fresher one.
  state.allSessions = [
    ...state.allSessions.filter((item) => item.project_id !== state.projectId),
    ...state.sessions.filter((item) => !item.archived),
  ];
  // A pane showing a session of this project that has since been archived
  // or removed would draw something that is not there.
  state.panes = state.panes.map((id) => {
    const item = state.sessions.find((entry) => entry.id === id);
    return item && item.archived ? null : id;
  });
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
