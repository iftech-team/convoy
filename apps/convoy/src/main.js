// Convoy's front end.
//
// Deliberately without a framework for now: this is a prototype whose job is
// to answer two questions — does it look right, and does the terminal feel
// right on Linux. A framework would confound the second.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { icons } from "./icons.js";
import { installTasks } from "./tasks.js";

const state = {
  settings: { theme: "system", default_agent: "claude" },
  dialog: null,
  projects: [],
  storage: "",
  running: [],
  projectId: null,
  sessions: [],
  sessionId: null,
  filter: "",
  search: "",
  tab: "sessions",
  showArchived: false,
};

/** One xterm per session, kept across navigation so output is not lost. */
const terminals = new Map();

const app = document.querySelector("#app");
const toasts = document.querySelector("#toasts");

// ---------------------------------------------------------------- helpers --

const escape = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );

function toast(message, kind = "info") {
  const element = document.createElement("div");
  element.className = `toast${kind === "error" ? " toast--error" : ""}`;
  element.textContent = message;
  toasts.append(element);
  setTimeout(() => element.remove(), kind === "error" ? 7000 : 4000);
}

// `undefined` means the call failed, never `null`: a command returning unit
// resolves to `null`, and using that as the failure signal made a successful
// save look like a rejected one.
async function call(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    toast(String(error), "error");
    return undefined;
  }
}

// True when the call went through, for commands that return nothing.
async function callDone(command, args) {
  return (await call(command, args)) !== undefined;
}

const project = () => state.projects.find((item) => item.id === state.projectId);
const session = () => state.sessions.find((item) => item.id === state.sessionId);

// ------------------------------------------------------------------ views --

function sidebar() {
  const groups = new Map();
  const loose = [];
  const needle = state.search.toLowerCase();
  for (const item of state.projects) {
    if (needle && !`${item.title} ${item.path}`.toLowerCase().includes(needle)) {
      continue;
    }
    if (item.group) {
      if (!groups.has(item.group)) groups.set(item.group, []);
      groups.get(item.group).push(item);
    } else {
      loose.push(item);
    }
  }

  const sessionLinks = (item) =>
    item.id !== state.projectId
      ? ""
      : state.sessions
          .map(
            (entry) => `
        <button class="session-link${entry.running ? " session-link--running" : ""}"
                data-open="${escape(entry.id)}"
                aria-current="${entry.id === state.sessionId}">
          <span class="session-link__dot"></span>
          <span class="session-link__title">${escape(entry.title)}</span>
        </button>`,
          )
          .join("");

  const row = (item) => `
    <button class="project" data-project="${escape(item.id)}"
            aria-current="${item.id === state.projectId}">
      <span class="project__icon">${escape(item.icon || "")}</span>
      <span class="project__title">${escape(item.title)}</span>
      ${item.running ? '<span class="project__running"></span>' : ""}
      <span class="project__count">${item.sessions || ""}</span>
    </button>${sessionLinks(item)}`;

  const sections = [
    ...[...groups].map(
      ([name, items]) =>
        `<div class="sidebar__group">${escape(name)}</div>${items.map(row).join("")}`,
    ),
    ...loose.map(row),
  ].join("");

  return `
    <aside class="sidebar">
      <div class="sidebar__brand">
        <span class="sidebar__mark">${icons.terminal}</span>
        <span class="sidebar__name">Convoy</span>
      </div>
      <div class="sidebar__search">
        ${icons.search}
        <input id="project-search" type="search" placeholder="Find a project"
               value="${escape(state.search)}" spellcheck="false" />
      </div>
      <div class="sidebar__label">Projects</div>
      <div class="sidebar__list">${sections || emptySidebar()}</div>
      <div class="sidebar__footer">
        <button class="button" style="width:100%">${icons.folder} Open folder…</button>
      </div>
    </aside>`;
}

const emptySidebar = () =>
  `<div style="padding:10px 8px;color:var(--text-faint);font-size:12.5px">
     ${state.search ? "Nothing matches." : "No projects yet."}
   </div>`;

function header() {
  const current = project();
  if (!current) return "";
  const tabs = ["Sessions", "Reviews", "Specs", "Tasks", "Docs"];
  return `
    <div class="header">
      <div class="header__top">
        <div class="header__titles">
          <h1 class="header__name">
            ${escape(current.title)}
            ${current.group ? `<span class="badge">${escape(current.group)}</span>` : ""}
          </h1>
          <div class="header__path">${escape(current.path)}</div>
        </div>
        <div class="header__actions">
          <button class="button button--icon" data-action="settings"
                  title="Settings">${icons.gear}</button>
          <button class="button button--primary" data-action="new-session">
            ${icons.plus} New session
          </button>
        </div>
      </div>
      <nav class="tabs">
        ${tabs
          .map(
            (name, index) =>
              `${index > 1 ? '<span class="tab__divider"></span>' : ""}
               <button class="tab" data-tab="${name.toLowerCase()}"
                       aria-selected="${state.tab === name.toLowerCase()}">${name}</button>`,
          )
          .join("")}
      </nav>
    </div>`;
}

function sessionList() {
  const filtered = state.sessions.filter(
    (item) =>
      !state.filter ||
      `${item.title} ${item.provider_id}`
        .toLowerCase()
        .includes(state.filter.toLowerCase()),
  );

  if (!state.sessions.length) {
    return empty(
      icons.review,
      "No sessions yet",
      "Start one to run Claude Code or Codex in this folder.",
    );
  }

  const rows = filtered
    .map((item) => {
      const mark = item.agent === "claude" ? "✳" : "◉";
      const meta = [
        item.agent === "claude" ? "Claude Code" : "Codex",
        item.running ? "running" : item.started ? "stopped" : "never started",
        item.provider_id ? `<code>${escape(item.provider_id.slice(0, 8))}</code>` : null,
      ]
        .filter(Boolean)
        .join(" · ");
      return `
        <div class="row" data-session="${escape(item.id)}">
          <span class="row__mark">${mark}</span>
          <div class="row__body">
            <div class="row__title">${escape(item.title)}</div>
            <div class="row__meta">${meta}</div>
          </div>
          <div class="row__actions">
            ${item.pinned ? '<span class="badge--muted badge">Pinned</span>' : ""}
            ${
              item.running
                ? `<button class="button button--danger" data-stop="${escape(item.id)}">${icons.stop} Stop</button>`
                : `<button class="button" data-start="${escape(item.id)}">${icons.play} ${item.started ? "Resume" : "Start"}</button>`
            }
            <button class="button button--quiet" data-open="${escape(item.id)}"
                    title="Open the terminal">${icons.open}</button>
          </div>
        </div>`;
    })
    .join("");

  return `
    <div class="section">
      <h2 class="section__title">Saved conversations
        <span class="section__count">${filtered.length}</span>
      </h2>
      <span class="section__spacer"></span>
      <div class="filter">
        ${icons.search}
        <input id="session-filter" type="search" placeholder="Filter by name or ID"
               value="${escape(state.filter)}" spellcheck="false" />
      </div>
      <button class="button button--icon" data-action="refresh" title="Refresh">
        ${icons.refresh}
      </button>
      <button class="button button--primary" data-action="new-session">
        ${icons.plus} New session
      </button>
    </div>
    <p class="section__hint">
      Pick a session, start a new one, or resume any conversation Claude Code or
      Codex saved for this folder — even ones started in a plain terminal.
    </p>
    <div class="list">${rows || emptyFilter()}</div>`;
}

const emptyFilter = () =>
  `<div style="padding:28px;color:var(--text-muted);text-align:center">
     Nothing matches “${escape(state.filter)}”.
   </div>`;

const empty = (mark, title, text) => `
  <div class="empty">
    <span class="empty__mark">${mark}</span>
    <h2 class="empty__title">${escape(title)}</h2>
    <p class="empty__text">${escape(text)}</p>
  </div>`;

function workbench() {
  const current = session();
  if (!current) return "";
  const stateClass = current.running ? "state--running" : "state";
  return `
    <div class="workbench">
      <div class="workbench__bar">
        <span class="crumb">
          ${escape(project()?.title ?? "")}
          <span class="crumb__sep">/</span>
          <span class="crumb__muted">${escape(current.title)}</span>
        </span>
        <span class="section__spacer"></span>
        <span class="state ${stateClass}">
          <span class="state__dot"></span>
          ${current.running ? "Running" : current.started ? "Stopped" : "Not started"}
        </span>
        ${
          current.running
            ? `<button class="button button--danger" data-stop="${escape(current.id)}">${icons.stop} Stop</button>`
            : `<button class="button" data-start="${escape(current.id)}">${icons.play} ${current.started ? "Resume" : "Start"}</button>`
        }
        <button class="button button--quiet" data-action="back" title="Back to the list">
          ${icons.chevron}
        </button>
      </div>
      <div class="terminal" id="terminal-host"></div>
    </div>`;
}

function newSessionDialog() {
  const draft = state.dialog;
  const agent = (name, label) => `
    <button data-agent="${name}" aria-pressed="${draft.agent === name}">${label}</button>`;
  return `
    <div class="scrim" data-dismiss="1">
      <div class="modal" role="dialog" aria-modal="true" aria-label="New session">
        <div class="modal__head">
          <div class="modal__title">New session</div>
          <div class="modal__hint">In ${escape(project()?.title ?? "")}</div>
        </div>
        <div class="modal__body">
          <label class="field">
            <span class="field__label">Name</span>
            <input id="draft-title" value="${escape(draft.title)}" spellcheck="false" />
          </label>
          <div class="field">
            <span class="field__label">Agent</span>
            <div class="choice">
              ${agent("claude", "Claude Code")}
              ${agent("codex", "Codex")}
            </div>
          </div>
          <label class="field">
            <span class="field__label">Model</span>
            <input id="draft-model" value="${escape(draft.model)}"
                   placeholder="Leave empty for the CLI default" spellcheck="false" />
          </label>
          <label class="field">
            <span class="field__label">First message</span>
            <textarea id="draft-prompt" spellcheck="false"
                      placeholder="Optional. Sent once, on the first launch only.">${escape(draft.prompt)}</textarea>
            <span class="field__note">Resuming never replays it.</span>
          </label>
        </div>
        <div class="modal__foot">
          <button class="button" data-dismiss="1">Cancel</button>
          <button class="button button--primary" data-action="create">Create</button>
        </div>
      </div>
    </div>`;
}

function settingsDialog() {
  const draft = state.dialog.settings;
  const segment = (key, options) => `
    <div class="segmented">
      ${options
        .map(
          ([value, label]) =>
            `<button data-set="${key}" data-value="${value}"
                     aria-pressed="${draft[key] === value}">${label}</button>`,
        )
        .join("")}
    </div>`;

  const setting = (title, hint, control) => `
    <div class="setting">
      <div class="setting__text">
        <div class="setting__title">${escape(title)}</div>
        ${hint ? `<div class="setting__hint">${escape(hint)}</div>` : ""}
      </div>
      <div class="setting__control">${control}</div>
    </div>`;

  return `
    <div class="scrim" data-dismiss="1">
      <div class="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <div class="modal__head">
          <div class="modal__title">Settings</div>
          <div class="modal__hint">Shared with the other builds through the workspace file.</div>
        </div>
        <div class="modal__body">
          <div class="group">
            <div class="group__label">Appearance</div>
            ${setting(
              "Theme",
              "System follows the desktop.",
              segment("theme", [
                ["system", "System"],
                ["light", "Light"],
                ["dark", "Dark"],
              ]),
            )}
            ${setting(
              "Font size",
              "Terminal text, 10 to 24.",
              `<input type="number" id="set-font" min="10" max="24" step="1"
                      value="${draft.font_size}" />`,
            )}
            ${setting(
              "Scrollback",
              "Lines kept per terminal, 1000 to 50000.",
              `<input type="number" id="set-scrollback" min="1000" max="50000" step="1000"
                      value="${draft.scrollback}" />`,
            )}
          </div>
          <div class="group">
            <div class="group__label">Agents</div>
            ${setting(
              "Default agent",
              "Preselected for a new session.",
              segment("default_agent", [
                ["claude", "Claude Code"],
                ["codex", "Codex"],
              ]),
            )}
            ${setting(
              "Claude usage status line",
              "Replaces that launch's own status line. Takes effect next launch.",
              `<button class="switch" data-toggle="claude_usage"
                       aria-pressed="${draft.claude_usage}"
                       aria-label="Claude usage status line"></button>`,
            )}
          </div>
          <div class="group">
            <div class="group__label">Integrations</div>
            ${setting(
              "Linear & Jira",
              "Import issues as tasks with an API key, a Jira login, or the agent’s MCP server.",
              `<button class="button" data-action="integrations">Manage…</button>`,
            )}
          </div>
        </div>
        <p class="modal__note">
          State lives in <code>${escape(state.storage)}</code>.
          Notifications, hibernation and keep-awake are not in this build yet.
        </p>
        <div class="modal__foot">
          <button class="button" data-dismiss="1">Cancel</button>
          <button class="button button--primary" data-action="save-settings">Save</button>
        </div>
      </div>
    </div>`;
}

function status() {
  const running = state.running.length;
  return `
    <footer class="status">
      <span class="status__item">${icons.limits} <strong>AI Limits</strong></span>
      <span class="status__sep"></span>
      <span class="status__item">${escape(state.storage)}</span>
      <span class="status__spacer"></span>
      <span class="status__item">${icons.terminal} ${running} running</span>
      <span class="status__sep"></span>
      <span class="status__item">${icons.awake} Awake</span>
    </footer>`;
}

// ----------------------------------------------------------------- render --

function render() {
  // Dialogs re-render while typing; put the caret back where it was.
  const active = document.activeElement;
  const focus = active?.id ? { id: active.id, start: active.selectionStart, end: active.selectionEnd } : null;
  const current = project();
  const body = !current
    ? empty(
        icons.folder,
        "No project selected",
        "Choose a project on the left to see its sessions.",
      )
    : state.sessionId
      ? workbench()
      : state.tab === "sessions"
        ? sessionList()
        : state.tab === "tasks"
          ? tasks.taskList()
          : empty(
            icons.review,
            `${state.tab[0].toUpperCase()}${state.tab.slice(1)}`,
            "Not in this prototype yet — it exists to check the design and the terminal.",
          );

  app.innerHTML = `
    <div class="shell">
      ${sidebar()}
      <main class="main">${header()}${body}</main>
      ${status()}
    </div>`;

  if (state.dialog?.kind === "session") {
    app.insertAdjacentHTML("beforeend", newSessionDialog());
    document.querySelector("#draft-title")?.focus();
  }
  if (state.dialog?.kind === "settings") {
    app.insertAdjacentHTML("beforeend", settingsDialog());
  }
  const extra = tasks.dialog();
  if (extra) app.insertAdjacentHTML("beforeend", extra);
  if (focus && state.dialog) {
    const field = document.getElementById(focus.id);
    if (field) {
      field.focus();
      try {
        field.setSelectionRange(focus.start, focus.end);
      } catch {
        // Selects and checkboxes have no caret.
      }
    }
  }
  if (state.sessionId) mountTerminal(state.sessionId);
}

// --------------------------------------------------------------- terminal --

function terminalFor(id) {
  if (terminals.has(id)) return terminals.get(id);

  // The element lives outside the re-rendered tree and is moved into place.
  // Rebuilding it would detach xterm from its canvas and lose the scrollback,
  // which in a session that has been running for an hour is the whole point.
  const host = document.createElement("div");
  host.className = "terminal__surface";

  const style = getComputedStyle(document.documentElement);
  const terminal = new Terminal({
    fontFamily: style.getPropertyValue("--font-mono").trim(),
    fontSize: 13,
    lineHeight: 1.35,
    cursorBlink: true,
    allowProposedApi: true,
    scrollback: 10000,
    // Matching the window rather than the usual black rectangle: an agent's
    // output is prose to read, not a console to admire.
    theme: {
      background: style.getPropertyValue("--term-bg").trim(),
      foreground: style.getPropertyValue("--term-fg").trim(),
      cursor: style.getPropertyValue("--term-cursor").trim(),
      selectionBackground: style.getPropertyValue("--term-selection").trim(),
    },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.loadAddon(new WebLinksAddon());

  terminal.onData((data) => invoke("terminal_write", { id, data }).catch(() => {}));
  terminal.onResize(({ cols, rows }) =>
    invoke("terminal_resize", { id, cols, rows }).catch(() => {}),
  );

  const entry = { terminal, fit, host, opened: false };
  terminals.set(id, entry);
  return entry;
}

function mountTerminal(id) {
  const slot = document.querySelector("#terminal-host");
  if (!slot) return;
  const entry = terminalFor(id);
  slot.append(entry.host);
  if (!entry.opened) {
    entry.terminal.open(entry.host);
    entry.opened = true;
  }
  requestAnimationFrame(() => {
    entry.fit.fit();
    entry.terminal.focus();
  });
}

const refit = () => {
  if (!state.sessionId) return;
  terminals.get(state.sessionId)?.fit.fit();
};

// ------------------------------------------------------------------ data --

async function loadSettings() {
  const settings = await call("settings_read");
  if (!settings) return;
  state.settings = settings;
  // "system" means whatever the desktop says; the other two are explicit.
  const root = document.documentElement;
  if (settings.theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", settings.theme);
}

async function loadWorkspace() {
  const view = await call("workspace_read");
  if (!view) return;
  state.projects = view.projects;
  state.running = view.running;
  state.storage = view.storage;
  if (!state.projectId && view.projects.length) {
    state.projectId = view.projects[0].id;
  }
  await loadSessions();
}

async function loadSessions() {
  if (!state.projectId) return render();
  const sessions = await call("sessions_for", {
    projectId: state.projectId,
    archived: state.showArchived,
  });
  state.sessions = sessions ?? [];
  await tasks.loadTasks();
  render();
}

function openDialog() {
  if (!state.projectId) return;
  state.dialog = {
    kind: "session",
    title: project()?.title ?? "Session",
    agent: state.settings.default_agent || "claude",
    model: "",
    prompt: "",
  };
  render();
}

function openSettings() {
  state.dialog = { kind: "settings", settings: { ...state.settings } };
  render();
}

// Numbers come back out of their inputs before anything else reads them.
function readSettings() {
  const number = (id, fallback) => {
    const value = Number(document.querySelector(id)?.value);
    return Number.isFinite(value) ? Math.round(value) : fallback;
  };
  state.dialog.settings = {
    ...state.dialog.settings,
    font_size: number("#set-font", state.settings.font_size),
    scrollback: number("#set-scrollback", state.settings.scrollback),
  };
}

async function saveSettings() {
  readSettings();
  const input = state.dialog.settings;
  if (!(await callDone("settings_save", { input }))) return;
  state.dialog = null;
  await loadSettings();
  applyTerminalSettings();
  render();
  toast("Settings saved");
}

// Font size and scrollback reach terminals that are already open, so the
// change is visible without restarting a session.
function applyTerminalSettings() {
  const style = getComputedStyle(document.documentElement);
  for (const entry of terminals.values()) {
    entry.terminal.options.fontSize = state.settings.font_size;
    entry.terminal.options.scrollback = state.settings.scrollback;
    entry.terminal.options.theme = {
      background: style.getPropertyValue("--term-bg").trim(),
      foreground: style.getPropertyValue("--term-fg").trim(),
      cursor: style.getPropertyValue("--term-cursor").trim(),
      selectionBackground: style.getPropertyValue("--term-selection").trim(),
    };
    entry.fit.fit();
  }
}

function readDraft() {
  const value = (id) => document.querySelector(id)?.value ?? "";
  state.dialog = {
    ...state.dialog,
    title: value("#draft-title"),
    model: value("#draft-model"),
    prompt: value("#draft-prompt"),
  };
}

async function createSession() {
  readDraft();
  const draft = state.dialog;
  const id = await call("session_create", {
    projectId: state.projectId,
    agent: draft.agent,
    title: draft.title,
    prompt: draft.prompt,
    model: draft.model,
  });
  if (!id) return;
  state.dialog = null;
  await loadWorkspace();
  // Selected but not started. Launching an agent is always an explicit act —
  // a first message is passed on the command line and acted on at once.
  state.sessionId = id;
  render();
}

async function start(id) {
  const { terminal, fit } = terminalFor(id);
  state.sessionId = id;
  render();
  fit.fit();
  await callDone("session_start", {
    id,
    cols: terminal.cols,
    rows: terminal.rows,
  });
  await loadWorkspace();
}

// Starts a session without opening it: used to run several imported tasks
// at once. The terminal is sized again when it is first shown.
async function launch(id) {
  const { terminal } = terminalFor(id);
  return callDone("session_start", { id, cols: terminal.cols, rows: terminal.rows });
}

const tasks = installTasks({
  state,
  escape,
  icons,
  call,
  callDone,
  toast,
  render,
  loadWorkspace,
  start,
  launch,
});

// ---------------------------------------------------------------- events --

app.addEventListener("click", async (event) => {
  const target = event.target.closest(
    "[data-project],[data-tab],[data-action],[data-start],[data-stop],[data-open]," +
      "[data-session],[data-agent],[data-dismiss],[data-set],[data-toggle]," +
      "[data-task-run],[data-task-done],[data-task-edit],[data-issue],[data-conn-add]," +
      "[data-conn-edit],[data-conn-remove],[data-conn-auth]",
  );
  if (!target) return;
  if (target.tagName === "SELECT" || (target.tagName === "INPUT" && !target.dataset.issue)) return;
  if (await tasks.onClick(target)) return;
  // Clicking the panel itself must not dismiss it; only the scrim behind.
  if (target.dataset.dismiss && event.target.closest(".modal") && !target.classList.contains("button")) {
    return;
  }

  if (target.dataset.project) {
    state.projectId = target.dataset.project;
    state.sessionId = null;
    return loadSessions();
  }
  if (target.dataset.tab) {
    state.tab = target.dataset.tab;
    state.sessionId = null;
    return render();
  }
  if (target.dataset.start) return start(target.dataset.start);
  if (target.dataset.stop) {
    await callDone("session_stop", { id: target.dataset.stop });
    return;
  }
  if (target.dataset.open || target.dataset.session) {
    state.sessionId = target.dataset.open || target.dataset.session;
    return render();
  }

  switch (target.dataset.action) {
    case "refresh":
      return loadWorkspace();
    case "back":
      state.sessionId = null;
      return render();
    case "new-session":
      return openDialog();
    case "create":
      return createSession();
    case "settings":
      return openSettings();
    case "save-settings":
      return saveSettings();
  }

  if (target.dataset.set) {
    readSettings();
    state.dialog.settings[target.dataset.set] = target.dataset.value;
    return render();
  }
  if (target.dataset.toggle) {
    readSettings();
    const key = target.dataset.toggle;
    state.dialog.settings[key] = !state.dialog.settings[key];
    return render();
  }

  if (target.dataset.agent) {
    readDraft();
    state.dialog = { ...state.dialog, agent: target.dataset.agent };
    return render();
  }
  if (target.dataset.dismiss) {
    state.dialog = null;
    return render();
  }
});

// Escape closes the dialog, Enter in a single-line field submits it.
addEventListener("keydown", (event) => {
  if (!state.dialog) return;
  if (event.key === "Escape") {
    state.dialog = null;
    render();
  }
  if (event.key === "Enter" && event.target.tagName === "INPUT") {
    event.preventDefault();
    if (tasks.onEnter(event.target)) return;
    if (state.dialog.kind === "settings") saveSettings();
    else if (state.dialog.kind === "session") createSession();
  }
});

// Typing re-renders, so the caret has to be put back where it was. A
// framework would do this; here it is four lines and no dependency.
app.addEventListener("input", (event) => {
  const field = event.target;
  if (tasks.onInput(field)) return;
  const keys = { "session-filter": "filter", "project-search": "search" };
  const key = keys[field.id];
  if (!key) return;
  state[key] = field.value;
  const caret = field.selectionStart;
  render();
  const restored = document.querySelector(`#${field.id}`);
  if (restored) {
    restored.focus();
    restored.setSelectionRange(caret, caret);
  }
});

app.addEventListener("change", (event) => {
  const field = event.target;
  if (field.tagName === "SELECT" || field.type === "checkbox") {
    if (!field.dataset.issue) tasks.onChange(field);
  }
});

window.addEventListener("resize", refit);

// The desktop can change its colour scheme while the app is open.
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (state.settings.theme === "system") applyTerminalSettings();
});

await listen("terminal:data", ({ payload }) => {
  terminalFor(payload.id).terminal.write(payload.data);
});

await listen("terminal:exit", async ({ payload }) => {
  const entry = terminals.get(payload.id);
  if (entry) {
    const how = payload.stopped ? "stopped" : `exited with code ${payload.code}`;
    entry.terminal.write(`\r\n\x1b[2m── session ${how} ──\x1b[0m\r\n`);
  }
  await loadWorkspace();
});

await loadSettings();
await loadWorkspace();
