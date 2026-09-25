// Bootstrap, rendering and every action the window offers.

import { installTasks } from "./tasks.js";
import { icons } from "./icons.js";
import { listen } from "@tauri-apps/api/event";
import { open as openDialogNative, save as saveDialogNative } from "@tauri-apps/plugin-dialog";
import {
  applyTheme,
  attempt,
  call,
  changed,
  done,
  isRunning,
  loadPlanning,
  loadSessions,
  loadSettings,
  loadWorkspace,
  anySession,
  saveCollapsed,
  activeAccount,
  rememberAccount,
  observe,
  project,
  session,
  state,
  toast,
  visibleSessions,
} from "./state.js";
import { escape, plural, value } from "./ui.js";
import { SHORTCUTS, binding, capture, label, matches } from "./shortcuts.js";
import { dialogView } from "./dialogs.js";
import * as terminal from "./terminal.js";
import {
  header,
  quickMenu,
  reviewsView,
  sessionMenu,
  sessionsView,
  sidebar,
  contextMenu,
  tabBar,
  tabInfo,
  status,
  workbench,
} from "./views.js";
import { filesView, remoteMenu } from "./views-files.js";
import { settingsPage } from "./views-settings.js";
import { projectSettingsPage } from "./views-project.js";
import { forgetIcon, onIconLoaded } from "./icons.js";
import { runnable, specsView, tasksView } from "./views-planning.js";
import { dashboardView, homeView } from "./views-home.js";
import { PROJECT_DOC, docsView } from "./views-docs.js";

const app = document.querySelector("#app");
const imports = installTasks({ state, escape, icons, call, callDone: done, toast, render,
  loadWorkspace: async () => { await loadWorkspace(); await loadPlanning(); },
  start: terminal.start, launch: (id) => terminal.start(id, { background: true }) });
const isImportDialog = () => ["import", "integrations", "connection", "task-model"].includes(state.dialog?.kind);

// A thrown error used to leave a black window and no clue about it. Painting
// the message is the difference between "Convoy is broken" and a line the user
// can act on — a corrupt workspace file says so.
function fatal(detail) {
  const where = state.storage
    ? `<p>The workspace file is at <code>${escape(state.storage)}</code>.</p>`
    : "";
  app.innerHTML = `
    <div class="fatal">
      <h1>Convoy could not start</h1>
      <pre>${escape(detail)}</pre>
      ${where}
    </div>`;
}

window.addEventListener("error", (event) => fatal(event.error?.stack ?? event.message));
window.addEventListener("unhandledrejection", (event) =>
  fatal(event.reason?.stack ?? String(event.reason)),
);

// ----------------------------------------------------------------- render --

function content() {
  if (state.page === "home") return homeView();
  if (state.page === "dashboard") return dashboardView();
  if (state.files) return filesView();
  if (!project()) {
    return `<div class="empty">
      <span class="empty__mark">📁</span>
      <h2 class="empty__title">No project selected</h2>
      <p class="empty__text">Open a folder to begin.</p>
    </div>`;
  }
  if (state.page === "project" && state.projectDraft) return projectSettingsPage();
  if (state.sessionId) return workbench();
  switch (state.tab) {
    case "reviews":
      return reviewsView();
    case "specs":
      return specsView();
    case "tasks":
      return tasksView();
    case "docs":
      return docsView();
    default:
      return sessionsView();
  }
}

let shown = null;

// ------------------------------------------------------------------- tabs --

const TABS_KEY = "convoy.tabs";

try {
  state.sidebarHidden = localStorage.getItem("convoy.sidebarHidden") === "1";
} catch {
  state.sidebarHidden = false;
}

try {
  const saved = JSON.parse(localStorage.getItem(TABS_KEY) ?? "null");
  if (saved?.tabs) {
    state.tabs = saved.tabs;
    state.tabInfo = saved.info ?? {};
  }
} catch {
  /* no saved tabs */
}

function saveTabs() {
  try {
    localStorage.setItem(TABS_KEY, JSON.stringify({ tabs: state.tabs, info: state.tabInfo }));
  } catch {
    /* the tabs simply are not restored next launch */
  }
}

// ------------------------------------------------------------------ panes --

const PANES_KEY = "convoy.panes";

try {
  const saved = JSON.parse(localStorage.getItem(PANES_KEY) ?? "null");
  if (saved && [1, 2, 4].includes(saved.layout)) {
    state.layout = saved.layout;
    state.panes = saved.panes ?? [];
    state.focus = Math.min(saved.focus ?? 0, saved.layout - 1);
  }
} catch {
  /* single pane */
}

function savePanes() {
  try {
    localStorage.setItem(PANES_KEY, JSON.stringify({ layout: state.layout, panes: state.panes, focus: state.focus }));
  } catch {
    /* the layout simply is not restored */
  }
}

/// One, two or four panes. The open session stays in view, in the focused
/// pane; panes beyond the layout keep their session for when it grows back.
function setLayout(count) {
  state.menu = null;
  state.layout = count;
  if (state.sessionId) {
    const at = state.panes.indexOf(state.sessionId);
    if (at >= 0 && at < count) state.focus = at;
    else {
      state.focus = Math.min(state.focus, count - 1);
      state.panes[state.focus] = state.sessionId;
    }
  } else {
    state.focus = 0;
  }
  savePanes();
  render();
  requestAnimationFrame(terminal.refit);
}

function focusPane(index) {
  if (index === state.focus && state.panes[index] === state.sessionId) return;
  state.focus = index;
  savePanes();
  const id = state.panes[index];
  if (id) return openTab(id);
  render();
}

function closePane(index) {
  state.panes[index] = null;
  if (index === state.focus) {
    const other = state.panes.findIndex((id, at) => id && at < state.layout);
    if (other >= 0) {
      state.focus = other;
      savePanes();
      return openTab(state.panes[other]);
    }
    state.sessionId = null;
  }
  savePanes();
  render();
}

function stepPane(step) {
  if (state.layout < 2) return;
  return focusPane((state.focus + step + state.layout) % state.layout);
}

/// The open session always has a tab, and a tab remembers what it shows so
/// it can be drawn while another project is open. Archived sessions leave.
function syncTabs() {
  let touched = false;
  if (state.sessionId && !state.tabs.includes(state.sessionId)) {
    state.tabs.push(state.sessionId);
    touched = true;
  }
  for (const item of state.sessions) {
    if (!state.tabs.includes(item.id)) continue;
    if (item.archived) {
      state.tabs = state.tabs.filter((id) => id !== item.id);
      touched = true;
      continue;
    }
    const info = { title: item.title, agent: item.agent, project_id: item.project_id, started: item.started };
    if (JSON.stringify(state.tabInfo[item.id]) !== JSON.stringify(info)) {
      state.tabInfo[item.id] = info;
      touched = true;
    }
  }
  if (touched) saveTabs();
}

async function openTab(id) {
  const info = tabInfo(id);
  if (!info) return;
  state.page = null;
  if (info.project_id && info.project_id !== state.projectId) await selectProject(info.project_id);
  if (!state.sessions.some((item) => item.id === id)) {
    // The session is gone — removed or archived elsewhere.
    state.tabs = state.tabs.filter((tab) => tab !== id);
    saveTabs();
    return render();
  }
  openSession(id);
}

/// Closes a tab; a running agent is stopped first, after asking, as on macOS.
function closeTab(id) {
  if (state.running.includes(id)) {
    return openModal({
      kind: "confirm",
      title: "Stop this agent and close the tab?",
      body: "Recent output stays saved and the session can be resumed later.",
      confirmLabel: "Stop and close",
      danger: true,
      run: async () => {
        await terminal.stop(id);
        dropTab(id);
      },
    });
  }
  dropTab(id);
}

function dropTab(id) {
  const at = state.tabs.indexOf(id);
  if (at >= 0) {
    const earlier = closedTabs.indexOf(id);
    if (earlier >= 0) closedTabs.splice(earlier, 1);
    closedTabs.push(id);
  }
  state.tabs = state.tabs.filter((tab) => tab !== id);
  delete state.tabInfo[id];
  saveTabs();
  if (!state.running.includes(id)) terminal.forget(id);
  if (state.sessionId === id) {
    const next = state.tabs[Math.min(at, state.tabs.length - 1)];
    if (next) return openTab(next);
    state.sessionId = null;
  }
  render();
}

function moveTab(id, target) {
  const tabs = state.tabs.filter((tab) => tab !== id);
  tabs.splice(Math.max(0, Math.min(target, tabs.length)), 0, id);
  state.tabs = tabs;
  saveTabs();
  render();
}

function stepTabs(step) {
  if (state.tabs.length < 2) return stepSession(step);
  const at = state.tabs.indexOf(state.sessionId);
  return openTab(state.tabs[(Math.max(at, 0) + step + state.tabs.length) % state.tabs.length]);
}

function render() {
  syncTabs();
  const caret = document.activeElement?.id;
  const position = document.activeElement?.selectionStart;
  // Every change re-renders, and a scroller that jumps to the top each time
  // makes a long dialog or a long file list unusable.
  const scrolled = new Map(
    [...document.querySelectorAll("[data-scroll]")].map((node) => [
      node.dataset.scroll,
      node.scrollTop,
    ]),
  );

  app.innerHTML =
    state.page === "settings"
      ? `<div class="shell shell--page">${settingsPage()}${status()}</div>`
      : `<div class="shell${state.sidebarHidden ? " shell--nosidebar" : ""}">
          ${state.sidebarHidden ? "" : sidebar()}
          <main class="main">${tabBar()}${state.files || state.page || state.sessionId ? "" : header()}${content()}</main>
          ${status()}
        </div>`;

  if (state.menu === "session") app.insertAdjacentHTML("beforeend", sessionMenu());
  if (state.menu === "quick") app.insertAdjacentHTML("beforeend", quickMenu());
  if (state.menu === "remote") app.insertAdjacentHTML("beforeend", remoteMenu());
  if (state.menu?.kind) app.insertAdjacentHTML("beforeend", contextMenu(state.menu));
  if (state.dialog) app.insertAdjacentHTML("beforeend", isImportDialog() ? imports.dialog() : dialogView());
  if (state.dialog?.kind === "login") terminal.mount(state.dialog.id, "#login-host");

  // Every click re-renders, and a dialog rebuilt from markup replays its
  // opening animation — the whole modal blinked on each toggle. Only the
  // render that opens a dialog or menu animates it.
  const overlay = state.dialog ?? state.menu;
  if (overlay && overlay === shown) {
    for (const scrim of app.querySelectorAll(".scrim")) scrim.classList.add("scrim--settled");
  }
  shown = overlay;

  for (const [key, top] of scrolled) {
    const node = document.querySelector(`[data-scroll="${key}"]`);
    if (node) node.scrollTop = top;
  }

  // A freshly opened editor starts with the caret at the end of the file.
  if (state.docs?.focusEditor) {
    state.docs.focusEditor = false;
    const editor = document.querySelector("#doc-editor");
    if (editor) {
      editor.focus();
      editor.setSelectionRange(editor.value.length, editor.value.length);
    }
  }

  if (state.sessionId && !state.files && state.page !== "settings" && state.page !== "project") {
    if (state.layout > 1) {
      state.panes.forEach((id, index) => id && index < state.layout && terminal.mount(id, `#terminal-host-${index}`));
    } else {
      terminal.mount(state.sessionId);
    }
  }

  // Typing re-renders, so the caret goes back where it was.
  if (caret) {
    const restored = document.querySelector(`#${caret}`);
    if (restored) {
      restored.focus();
      if (position != null && restored.setSelectionRange) {
        try {
          restored.setSelectionRange(position, position);
        } catch {
          /* a number input has no selection range */
        }
      }
    }
  } else if (state.dialog) {
    document.querySelector(".modal input, .modal textarea")?.focus();
  }
}

observe(render);
onIconLoaded(() => render());

// -------------------------------------------------------------- selection --

async function selectProject(id) {
  if (state.page === "project") closeProjectSettings();
  if (state.projectId === id) return;
  state.projectId = id;
  state.sessionId = null;
  state.files = null;
  state.planning = null;
  await loadSessions();
  if (state.tab === "specs" || state.tab === "tasks") await loadPlanning();
}

/// Opens a session of any project, selecting its project first.
async function openAnywhere(id) {
  const target = anySession(id);
  if (!target) return;
  state.page = null;
  if (target.project_id !== state.projectId) await selectProject(target.project_id);
  if (!state.sessions.some((item) => item.id === id)) return render();
  openSession(id);
}

/// Session ids by most recent use, for the ⌘E switcher.
const recent = [];
/// Tabs closed this run, most recent last, for ⇧⌘T.
const closedTabs = [];

function reopenTab() {
  while (closedTabs.length) {
    const id = closedTabs.pop();
    if (anySession(id)) return openAnywhere(id);
  }
}

/// A stopped session shows its saved output in place of a live terminal.
async function showSaved(id) {
  if (isRunning(id)) return;
  const text = await call("session_output", { id });
  if (text === undefined || isRunning(id)) return;
  terminal.restore(id, text);
  if (terminal.missingConversation(id)) render();
}

/// Home or the Agent Dashboard, with what they show brought up to date.
async function openHome(page = "home") {
  if (state.page === "project") closeProjectSettings();
  state.page = page;
  state.sessionId = null;
  state.files = null;
  state.menu = null;
  render();
  await Promise.all([
    loadWorkspace(),
    loadAllTasks(),
    state.diagnostics ? null : call("diagnostics_run").then((checks) => (state.diagnostics = checks ?? [])),
  ]);
  render();
}

function openSession(id) {
  if (state.page === "home" || state.page === "dashboard") state.page = null;
  showSaved(id);
  const seen = recent.indexOf(id);
  if (seen >= 0) recent.splice(seen, 1);
  recent.unshift(id);
  if (state.find && state.sessionId !== id) closeFind();
  // A session already in a pane takes the focus there; otherwise it goes
  // into the focused pane, as on macOS.
  if (state.layout > 1) {
    const at = state.panes.indexOf(id);
    if (at >= 0 && at < state.layout) state.focus = at;
    else state.panes[state.focus] = id;
    savePanes();
  }
  state.sessionId = id;
  state.files = null;
  render();
  refreshGitStatus();
}

async function refreshGitStatus() {
  const current = session();
  if (!current || state.settings.git_status === false) {
    state.git = null;
    return;
  }
  const result = await attempt("git_status", { id: current.id });
  const next = result.ok ? { branch: result.value.branch, changed: result.value.changed_files } : null;
  if (JSON.stringify(next) !== JSON.stringify(state.git)) {
    state.git = next;
    render();
  }
}

// ------------------------------------------------------------- dialogs ----

const closeDialog = () => {
  if (state.dialog?.kind === "login") {
    const id = state.dialog.id;
    call("session_stop", { id });
    terminal.forget(id);
    loadProfiles().then(render);
  }
  // Turning the offer down once is an answer; Settings → Setup still has it.
  if (state.dialog?.kind === "macos-import") {
    try {
      localStorage.setItem(MAC_OFFERED, "1");
    } catch {}
  }
  state.dialog = null;
  state.menu = null;
  render();
};

const openModal = (dialog) => {
  state.dialog = dialog;
  state.menu = null;
  render();
};

/// Reads whatever the open dialog's fields currently hold, so a re-render
/// caused by a toggle does not lose typing.
function captureDraft() {
  const dialog = state.dialog;
  if (!dialog) return;
  const read = (id, key) => {
    const element = document.querySelector(`#${id}`);
    if (element) dialog[key] = element.value;
  };
  read("draft-title", "title");
  read("draft-model", "model");
  read("draft-prompt", "prompt");
  read("draft-notes", "notes");
  read("draft-provider", "provider_id");
  read("draft-brief", "brief");
  read("draft-branch", "branch");
  read("draft-base", "base");
  read("draft-account", "account");
  read("draft-label", "label");
  read("draft-text", "text");
  read("draft-group", "group");
  read("draft-icon", "icon");
  read("draft-shared", "shared_paths");
  read("draft-setup", "setup_command");
  read("draft-review", "review_template");
  for (const key of ["title", "problem", "requirements", "acceptance", "constraints", "plan"]) {
    read(`spec-${key}`, key);
  }
  for (const key of ["title", "details", "findings", "model"]) {
    read(`task-${key}`, key);
  }
}

// -------------------------------------------------------------- actions ---

const ACTIONS = {
  // projects
  "open-folder": openFolder,
  settings: () => (state.page === "settings" ? closeSettings() : openSettings()),
  accounts: openAccounts,
  "import-history": openTranscripts,
  "project-settings": openProjectSettings,
  "close-project-settings": closeProjectSettings,
  "layout-1": () => setLayout(1),
  "layout-2": () => setLayout(2),
  "layout-4": () => setLayout(4),
  "awake-settings": () => openSettings("General"),
  "awake-menu": () => {
    state.menu = state.menu?.kind === "awake" ? null : { kind: "awake" };
    render();
  },
  "stop-session": () => {
    const current = session();
    state.menu = null;
    return openModal({
      kind: "confirm",
      title: "Stop this agent?",
      body: "This interrupts current work. The saved conversation stays with the agent.",
      confirmLabel: "Stop",
      danger: true,
      run: () => terminal.stop(current.id),
    });
  },
  "start-session": () => {
    state.menu = null;
    return terminal.start(session().id);
  },
  "sleep-session": async () => {
    state.menu = null;
    render();
    await done("session_hibernate", { id: session().id });
  },
  "toggle-sidebar": () => {
    state.sidebarHidden = !state.sidebarHidden;
    try {
      localStorage.setItem("convoy.sidebarHidden", state.sidebarHidden ? "1" : "");
    } catch {
      /* not remembered */
    }
    render();
    terminal.refit();
  },
  // Home is the macOS app's: every project at once. The project's own page
  // is a click on the project.
  home: () => openHome(),
  dashboard: () => openHome("dashboard"),
  "remove-project": () =>
    openModal({ kind: "remove-project", id: state.projectId, path: project()?.path ?? "" }),
  "confirm-remove-project": async () => {
    const id = state.dialog.id;
    if (await done("project_remove", { id })) {
      state.projectId = null;
      closeDialog();
      await loadWorkspace();
    }
  },
  "reveal-project": () => call("path_reveal", { path: project()?.path ?? "" }),
  "copy-path": async () => {
    await navigator.clipboard.writeText(project()?.path ?? "");
    toast("Path copied");
  },
  "reconnect-project": reconnectProject,

  // sessions
  "new-session": () => openNewSession(state.projectId),
  "create-session": createSession,
  "edit-session": openEditSession,
  "save-session": saveSession,
  "pin-session": async () => {
    const current = session();
    state.menu = null;
    if (await done("session_pin", { id: current.id, pinned: !current.pinned })) {
      await loadSessions();
    }
  },
  "archive-session": async () => {
    const current = session();
    state.menu = null;
    if (await done("session_archive", { id: current.id, archived: !current.archived })) {
      if (!current.archived) state.sessionId = null;
      else state.showArchived = true;
      await loadSessions();
    }
  },
  "recover-session": async () => {
    const id = await call("session_recover", { id: session().id });
    state.menu = null;
    if (!id) return;
    await loadSessions();
    openSession(id);
  },
  "start-review": openReview,
  "create-review": createReview,
  // As on macOS: the reviewer's output, ready to trim, under a short ask.
  "send-feedback": () =>
    openModal({
      kind: "feedback",
      feedback: `Please address the review findings below, verify the changes, and report what you fixed.\n\n${terminal
        .buffer(state.sessionId)
        .slice(-18000)}`,
    }),
  "start-fresh": async () => {
    const id = await call("session_recover", { id: session().id });
    if (!id) return;
    await loadSessions();
    openSession(id);
    await terminal.start(id);
  },
  "insert-feedback": insertFeedback,
  "saved-output": openSavedOutput,
  usage: openUsage,
  "git-status": async () => {
    state.menu = null;
    render();
    await refreshGitStatus();
  },
  back: () => {
    state.sessionId = null;
    render();
  },
  refresh: () => loadWorkspace(),
  "macos-import": () => offerMacImport(false),
  "find-next": () => findStep(),
  "find-previous": () => findStep({ previous: true }),
  "find-case": () => {
    state.find.caseSensitive = !state.find.caseSensitive;
    findStep({ incremental: true });
    render();
  },
  "find-close": () => closeFind(),
  "macos-import-run": runMacImport,
  "refresh-activity": loadActivity,

  // menus
  quit: () => call("quit_now"),
  "open-split": () => setLayout(2),
  "close-split": () => setLayout(1),
  "session-menu": () => {
    state.menu = "session";
    render();
  },
  "quick-menu": () => {
    state.menu = "quick";
    render();
  },
  "manage-commands": () =>
    openModal({ kind: "commands", title: "", text: "", submit: false, scoped: true }),
  "add-command": addQuickCommand,

  // worktrees
  "create-worktree": () =>
    openModal({ kind: "worktree", branch: suggestBranch(session()?.title ?? "") }),
  "confirm-worktree": confirmWorktree,
  "confirm-setup": async () => {
    const id = state.dialog.id;
    closeDialog();
    if (await done("worktree_setup", { id })) toast("Worktree setup finished");
  },
  "remove-worktree": openRemoveWorktree,
  "confirm-remove-worktree": async () => {
    const id = state.dialog.id;
    closeDialog();
    if (await done("worktree_remove", { id })) {
      state.sessionId = null;
      await loadWorkspace();
    }
  },

  // settings
  "close-settings": closeSettings,
  "run-diagnostics": runDiagnostics,
  "reveal-worktrees": () => call("path_reveal", { path: state.worktrees }),
  "reset-shortcuts": () => savePref({ shortcuts: {} }),
  limits: () => (state.menu?.kind === "limits" ? closeDialog() : openLimits()),
  "limits-refresh": () => refreshLimits(true),
  "limits-settings": () => openSettings("AI Limits"),
  "page-add-profile": addPageProfile,
  "add-profile": addProfile,
  "scan-transcripts": scanTranscripts,

  // planning
  "new-spec": () =>
    openModal({
      kind: "spec",
      title: "",
      problem: "",
      requirements: "",
      acceptance: "",
      constraints: "",
      plan: "",
    }),
  "save-spec": saveSpec,
  "approve-spec": approveSpec,
  "new-task": () =>
    openModal({
      kind: "task",
      title: "",
      details: "",
      findings: "",
      agent: project()?.default_agent || state.settings.default_agent,
      mode: project()?.task_mode || "pr",
      model: "",
      auto_review: false,
      spec_id: "",
    }),
  "save-task": () => saveTask(false),
  "run-task-now": () => saveTask(true),
  "delete-task": () => {
    const id = state.dialog?.id;
    if (!id) return;
    closeDialog();
    state.menu = { kind: "task", id };
    return runTaskMenu("task-delete", id);
  },
  "queue-start": startQueue,
  "queue-stop": () => {
    state.queues.delete(state.projectId);
    render();
  },
  "confirm-queue": async () => {
    closeDialog();
    state.queues.add(state.projectId);
    await advanceQueue(state.projectId);
  },

  // files
  files: () => openFiles(),
  "files-close": () => {
    state.files = null;
    render();
  },
  "files-refresh": () => loadFiles(),
  "files-split": async () => {
    state.files.sideBySide = !state.files.sideBySide;
    await readSelection();
  },
  "files-remote": () => {
    state.menu = "remote";
    render();
  },
  stage: () => mutate({ action: "stage", path: selectedPath(), original: selectedOriginal() }),
  unstage: () => mutate({ action: "unstage", path: selectedPath(), original: selectedOriginal() }),
  "stage-all": () => mutate({ action: "stageAll" }),
  discard: () =>
    confirmThen("Discard changes to this file?", selectedPath(), () =>
      mutate({ action: "discard", path: selectedPath() }),
    ),
  trash: trashFile,
  "discard-hunk": openHunkChoice,
  "confirm-hunk": confirmHunk,
  commit: () => commit(false),
  amend: () =>
    confirmThen(
      "Amend the latest commit?",
      "The previous commit is replaced. Do not amend something already pushed.",
      () => commit(true),
    ),
  generate: generateMessage,
  fetch: () => mutate({ action: "fetch" }),
  pull: () => mutate({ action: "pull" }),
  push: () => mutate({ action: "push" }),
  pr: createPullRequest,
  revert: () =>
    confirmThen("Revert this commit?", selectedCommit(), () =>
      mutate({ action: "revert", commit: selectedCommit() }),
    ),
  "reset-soft": () =>
    confirmThen("Reset to this commit, keeping the index?", selectedCommit(), () =>
      mutate({ action: "resetSoft", commit: selectedCommit() }),
    ),
  "reset-mixed": () =>
    confirmThen("Reset to this commit and clear the index?", selectedCommit(), () =>
      mutate({ action: "resetMixed", commit: selectedCommit() }),
    ),
  "switch-branch": () =>
    state.files.branch ? mutate({ action: "switch", branch: state.files.branch }) : null,
  "new-branch": () => openModal({ kind: "branch", branch: "" }),
  "confirm-branch": async () => {
    const branch = value("draft-branch");
    closeDialog();
    await mutate({ action: "branch", branch });
  },
  "confirm-generic": async () => {
    const run = state.dialog.run;
    closeDialog();
    await run();
  },
  "copy-markdown": async () => {
    await navigator.clipboard.writeText(state.dialog.text);
    toast("Copied");
  },
};

/// What each bound key does. Kept beside the bindings rather than inside the
/// key handler, so adding an action is one entry in two places and not a
/// branch in a growing chain of ifs.
const SHORTCUT_ACTIONS = {
  palette: () => openPalette(),
  newSession: () => ACTIONS["new-session"](),
  files: () => openFiles(),
  sidebar: () => ACTIONS["toggle-sidebar"](),
  next: () => stepTabs(1),
  previous: () => stepTabs(-1),
  closeTab: () => state.sessionId && closeTab(state.sessionId),
  settings: () => (state.page === "settings" ? closeSettings() : openSettings()),
  // ⌘F finds in the terminal on screen, and in the sidebar otherwise.
  search: () => (terminalShown() ? openFind() : document.querySelector("#project-search")?.focus()),
  switcher: () => openPalette("terminals"),
  home: () => openHome(),
  dashboard: () => (state.page === "dashboard" ? openHome() : openHome("dashboard")),
  reopenTab: () => reopenTab(),

  // sessions — each acts on the open session, and does nothing without one
  resume: () => withSession((current) => !current.running && terminal.start(current.id)),
  stop: () =>
    withSession((current) =>
      current.running &&
      openModal({
        kind: "confirm",
        title: "Stop this agent?",
        body: "This interrupts current work. The saved conversation stays with the agent.",
        confirmLabel: "Stop",
        danger: true,
        run: () => terminal.stop(current.id),
      }),
    ),
  sleep: () => withSession((current) => current.running && done("session_hibernate", { id: current.id })),
  pin: () => withSession(() => ACTIONS["pin-session"]()),
  edit: () => withSession(() => openEditSession()),
  review: () => withSession(() => openReview()),
  feedback: () => withSession((current) => current.review_of && ACTIONS["send-feedback"]()),
  quick: () => withSession(() => ACTIONS["quick-menu"]()),
  split: () => setLayout(state.layout === 2 ? 1 : 2),
  layout1: () => setLayout(1),
  layout2: () => setLayout(2),
  layout4: () => setLayout(4),
  paneNext: () => stepPane(1),
  panePrevious: () => stepPane(-1),
  paneClose: () => state.layout > 1 && closePane(state.focus),

  // project
  openFolder: () => openFolder(),
  newTask: () => withProject(async () => {
    await showTab("tasks");
    ACTIONS["new-task"]();
  }),
  newSpec: () => withProject(async () => {
    await showTab("specs");
    ACTIONS["new-spec"]();
  }),
  importIssues: () => withProject(async () => {
    await showTab("tasks");
    await imports.loadTasks();
    await imports.onClick({ dataset: { action: "import" } });
  }),
  sessionsTab: () => withProject(() => showTab("sessions")),
  reviewsTab: () => withProject(() => showTab("reviews")),
  specsTab: () => withProject(() => showTab("specs")),
  tasksTab: () => withProject(() => showTab("tasks")),
  docsTab: () => withProject(() => showTab("docs")),
  activity: () => ACTIONS.activity(),
  nextTab: () => withProject(() => stepTab(1)),
  previousTab: () => withProject(() => stepTab(-1)),
  reveal: () => withProject(() => ACTIONS["reveal-project"]()),
  copyPath: () => withProject(() => ACTIONS["copy-path"]()),
  gitRefresh: () => withSession(() => ACTIONS["git-status"]()),

  // general
  theme: () =>
    savePref({ theme: { system: "light", light: "dark", dark: "system" }[state.settings.theme] ?? "system" }),
  wake: () => {
    const next = state.settings.keep_awake === "off" ? "always" : "off";
    toast(next === "off" ? "Sleep allowed" : "Keeping the computer awake");
    return savePref({ keep_awake: next });
  },
  limits: () => openLimits(),
  limitsRefresh: () => refreshLimits(true),
};

const TAB_ORDER = ["sessions", "reviews", "specs", "tasks", "docs"];

const withSession = (run) => (session() ? run(session()) : undefined);
const withProject = (run) => (project() ? run(project()) : undefined);

/// Shows a project tab, leaving a session, Files or the settings page.
async function showTab(tab) {
  state.page = null;
  state.tab = tab;
  state.sessionId = null;
  state.files = null;
  render();
  if (tab === "specs" || tab === "tasks") await loadPlanning();
  if (tab === "docs") await loadDocs();
}

function stepTab(step) {
  const at = TAB_ORDER.indexOf(state.tab);
  return showTab(TAB_ORDER[(Math.max(at, 0) + step + TAB_ORDER.length) % TAB_ORDER.length]);
}

/// The next or previous session of the selected project, wrapping round. Does
/// nothing when the list is empty, and opens the first when none is chosen.
function stepSession(step) {
  const sessions = visibleSessions();
  if (!sessions.length) return;
  const at = sessions.findIndex((item) => item.id === state.sessionId);
  const next = at < 0 ? 0 : (at + step + sessions.length) % sessions.length;
  return openSession(sessions[next].id);
}

// --------------------------------------------------------------- handlers --

app.addEventListener("click", async (event) => {
  const target = event.target.closest(
    "[data-action],[data-project],[data-tab],[data-start],[data-stop],[data-open]," +
      "[data-set],[data-toggle],[data-dismiss],[data-quick],[data-files-view]," +
      "[data-change],[data-file],[data-commit],[data-branch],[data-task],[data-spec]," +
      "[data-approve],[data-export],[data-prepare],[data-status],[data-remove-profile]," +
      "[data-remove-command],[data-import],[data-task-view],[data-palette],[data-split],"
      + "[data-capture],[data-expand],[data-project-more],[data-menu-act],[data-settings-section]," +
      "[data-pref-set],[data-pref-toggle],[data-icon-tab],[data-proj-color],[data-proj-icon]," +
      "[data-proj-toggle],[data-proj-clear],[data-icon-source],[data-doc],[data-activity-open]," +
      "[data-tab-close],[data-tab-open],[data-tab-act],[data-awake],[data-pane-pick],[data-pane-close]," +
      "[data-layout],[data-needs-you],[data-settings-open],[data-home-tasks],[data-task-scope],[data-task-group],[data-task-run],[data-task-pr],[data-task-more],[data-use-account],[data-login-account],[data-issue],[data-conn-add],[data-conn-edit],[data-conn-remove],[data-conn-auth],[data-issue-source]",
  );
  if (!target) return;
  const data = target.dataset;
  if (data.issueSource) return done("issue_open", { id: data.issueSource });
  if (data.action === "edit-task-model") {
    await imports.loadTasks();
    return imports.editModel(state.dialog.id);
  }
  if (data.action === "import" || data.action === "integrations") await imports.loadTasks();
  if (data.menuAct) return runMenu(data.menuAct);
  if (data.awake) {
    state.menu = null;
    return savePref({ keep_awake: data.awake });
  }
  if (data.needsYou) return openTab(data.needsYou);
  if (data.tabClose) return closeTab(data.tabClose);
  if (data.tabOpen) return openTab(data.tabOpen);
  if (data.tabAct) {
    const id = state.menu?.id;
    state.menu = null;
    const at = state.tabs.indexOf(id);
    if (data.tabAct.startsWith("pane-")) {
      const pane = Number(data.tabAct.slice(5));
      state.panes[pane] = id;
      state.focus = pane;
      savePanes();
      return openTab(id);
    }
    if (data.tabAct === "left") return moveTab(id, at - 1);
    if (data.tabAct === "right") return moveTab(id, at + 1);
    if (data.tabAct === "close") return closeTab(id);
    if (data.tabAct === "others") {
      for (const other of [...state.tabs]) if (other !== id && !state.running.includes(other)) dropTab(other);
      return openTab(id);
    }
  }
  if (data.doc) return openDoc(data.doc);
  if (data.activityOpen) {
    state.menu = null;
    if (!data.activityProject) return openAnywhere(data.activityOpen);
    if (data.activityProject && data.activityProject !== state.projectId) await selectProject(data.activityProject);
    await loadSessions();
    if (state.sessions.some((item) => item.id === data.activityOpen)) return openSession(data.activityOpen);
    return render();
  }
  if (state.page === "project" && !state.dialog) {
    if (data.iconTab) {
      state.iconTab = data.iconTab;
      return render();
    }
    if (data.projColor !== undefined) return saveProjectField({ color: data.projColor });
    if (data.projIcon !== undefined) return saveProjectField({ icon: data.projIcon });
    if (data.projToggle) return saveProjectField({ [data.projToggle]: !state.projectDraft[data.projToggle] });
    if (data.projClear) return saveProjectField({ [data.projClear]: "" });
    if (data.iconSource) return setProjectImage(data.iconSource);
  }
  if (data.projectMore) {
    const box = target.getBoundingClientRect();
    return openContextMenu("project", data.projectMore, box.left, box.bottom + 4);
  }
  if (data.expand) {
    if (state.collapsed.has(data.expand)) state.collapsed.delete(data.expand);
    else state.collapsed.add(data.expand);
    saveCollapsed();
    return render();
  }
  if (state.page === "settings" && !state.dialog) {
    if (data.settingsSection) {
      state.settingsSection = data.settingsSection;
      state.capturing = null;
      render();
      if (data.settingsSection === "Setup" && !state.diagnostics) await runDiagnostics();
      if (data.settingsSection === "Setup") checkMacImport().then(render);
      return;
    }
    if (data.prefSet === "profile_agent") {
      state.profileAgent = data.value;
      return render();
    }
    if (data.prefSet) return savePref({ [data.prefSet]: data.value });
    if (data.prefToggle) return savePref({ [data.prefToggle]: !state.settings[data.prefToggle] });
    if (data.capture) {
      state.capturing = state.capturing === data.capture ? null : data.capture;
      return render();
    }
  }
  if (data.connAdd || data.connEdit || data.connRemove) {
    await imports.onClick(target);
    return;
  }
  if (isImportDialog() || ["import", "integrations"].includes(data.action)) {
    if (await imports.onClick(target)) return;
  }

  if (data.dismiss) {
    if (event.target.closest(".modal, .menu") && !target.classList.contains("button")) return;
    return closeDialog();
  }
  if (data.action) {
    // Menus open under the control that asked for them.
    const box = target.getBoundingClientRect();
    state.anchor = { top: box.bottom + 4, right: Math.max(8, window.innerWidth - box.right) };
    const handler = ACTIONS[data.action];
    if (handler) return handler();
    return;
  }
  if (data.project) {
    if (state.page === "home" || state.page === "dashboard") state.page = null;
    await selectProject(data.project);
    return render();
  }
  if (data.settingsOpen) return openSettings(data.settingsOpen);
  if (data.homeTasks) {
    state.page = null;
    state.taskScope = "all";
    state.tab = "tasks";
    await loadAllTasks();
    await loadPlanning();
    return render();
  }
  if (data.tab) {
    if (state.page === "project") state.page = null;
    state.tab = data.tab;
    state.sessionId = null;
    state.files = null;
    render();
    if (data.tab === "specs" || data.tab === "tasks") await loadPlanning();
    if (data.tab === "docs") await loadDocs();
    return;
  }
  if (data.start) {
    // From Home or the Dashboard it may be another project's session.
    if (!state.sessions.some((item) => item.id === data.start)) await openAnywhere(data.start);
    return terminal.start(data.start);
  }
  if (data.stop) return terminal.stop(data.stop);
  if (data.open) return openAnywhere(data.open);
  if (data.split) {
    const pane = state.dialog?.pane ?? state.focus;
    state.dialog = null;
    state.panes[pane] = data.split;
    state.focus = pane;
    savePanes();
    return openTab(data.split);
  }
  if (data.panePick !== undefined) {
    state.focus = Number(data.panePick);
    return openModal({ kind: "split", pane: Number(data.panePick) });
  }
  if (data.paneClose !== undefined) return closePane(Number(data.paneClose));
  if (data.layout) return setLayout(Number(data.layout));
  if (data.quick) {
    state.menu = null;
    render();
    return done("quick_command_send", {
      sessionId: state.sessionId,
      commandId: data.quick,
    });
  }
  if (data.set) {
    captureDraft();
    state.dialog[fieldFor(data.set)] = data.value;
    if (state.dialog.kind === "session" && data.set === "agent") state.dialog.account = activeAccount(data.value);
    return render();
  }
  if (data.toggle) {
    captureDraft();
    state.dialog[data.toggle] = !state.dialog[data.toggle];
    return render();
  }
  if (data.filesView) {
    state.files.view = data.filesView;
    state.files.selection = null;
    state.files.preview = null;
    return render();
  }
  if (data.change || data.file) {
    const path = data.change ?? data.file;
    const change = state.files.snapshot?.changes.find((item) => item.path === path);
    state.files.selection = data.file
      ? { kind: "file", path }
      : change?.untracked
        ? { kind: "untracked", path }
        : change?.staged
          ? { kind: "staged", path }
          : { kind: "unstaged", path };
    return readSelection();
  }
  if (data.commit) {
    state.files.selection = { kind: "commit", commit: data.commit };
    return readSelection();
  }
  if (data.branch) {
    state.files.branch = data.branch;
    return render();
  }
  if (data.taskScope) {
    state.taskScope = data.taskScope;
    if (data.taskScope === "all") await loadAllTasks();
    return render();
  }
  if (data.taskGroup) {
    state.foldedStatuses ??= new Set();
    if (state.foldedStatuses.has(data.taskGroup)) state.foldedStatuses.delete(data.taskGroup);
    else state.foldedStatuses.add(data.taskGroup);
    return render();
  }
  if (data.taskRun) return runTask(data.taskRun);
  if (data.taskPr) return done("task_pr_open", { id: data.taskPr });
  if (data.taskMore) {
    const box = target.getBoundingClientRect();
    return openContextMenu("task", data.taskMore, box.left - 180, box.bottom + 4);
  }
  if (data.taskView) {
    state.taskView = data.taskView;
    return render();
  }
  if (data.task) return openTask(data.task);
  if (data.spec) return openSpec(data.spec);
  if (data.approve) {
    if (await done("spec_approve", { id: data.approve, revision: Number(data.revision) })) {
      await loadPlanning();
    }
    return;
  }
  if (data.export) return exportSpec(data.export);
  if (data.prepare) return prepareTask(data.prepare);
  if (data.status) return setTaskStatus(data.status);
  if (data.useAccount) {
    const [agent, id] = data.useAccount.split(":");
    rememberAccount(agent, id);
    return render();
  }
  if (data.loginAccount) return openLogin(data.loginAccount);
  if (data.removeProfile) {
    if (await done("profile_remove", { id: data.removeProfile })) await loadProfiles();
    return render();
  }
  if (data.removeCommand) {
    if (await done("quick_command_remove", { id: data.removeCommand })) await loadSessions();
    return render();
  }
  if (data.import) return importTranscript(data.import, data.title);
  if (data.palette) return runPalette(Number(data.palette));
});

const fieldFor = (key) =>
  ({ agent: "agent", spec: "spec_id", mode: "mode", profile: "profile", hunk: "hunk" })[key] ?? key;

app.addEventListener("input", (event) => {
  const field = event.target;
  if (isImportDialog() && imports.onInput(field)) return;
  if (field.id === "session-filter") {
    state.filter = field.value;
    return render();
  }
  if (field.id === "doc-editor") {
    const first = !state.docs.dirty;
    state.docs.text = field.value;
    state.docs.dirty = true;
    if (first) render();
    return;
  }
  if (field.id === "activity-filter") {
    state.activityFilter = field.value;
    return render();
  }
  if (field.id === "settings-search") {
    state.settingsQuery = field.value;
    return render();
  }
  if (field.id === "project-search") {
    state.search = field.value;
    return render();
  }
  if (field.id === "commit-message") {
    state.files.message = field.value;
    return;
  }
  if (state.dialog?.kind === "session" && (field.id === "draft-title" || field.id === "draft-branch")) {
    if (field.id === "draft-branch") state.dialog.branchEdited = true;
    else if (!state.dialog.branchEdited) {
      state.dialog.branch = suggestBranch(field.value || "session", state.dialog.projectId);
      const branch = document.querySelector("#draft-branch");
      if (branch) branch.value = state.dialog.branch;
    }
    return;
  }
  if (field.id === "dashboard-filter") {
    state.dashboardFilter = field.value;
    return render();
  }
  if (field.id === "find-term") {
    state.find.term = field.value;
    findStep({ incremental: true });
    return render();
  }
  if (field.id === "palette-input") {
    state.dialog.query = field.value;
    state.dialog.results = paletteResults(field.value, state.dialog.mode);
    state.dialog.index = 0;
    return render();
  }
});

app.addEventListener("change", async (event) => {
  if (state.dialog?.kind === "session" && event.target.id === "draft-project") {
    captureDraft();
    return sessionProject(state.dialog, event.target.value);
  }
  if (state.dialog?.kind === "session" && event.target.id === "draft-account") {
    state.dialog.account = event.target.value;
    return;
  }
  const projectKey = event.target.dataset?.projText ?? event.target.dataset?.projSelect;
  if (projectKey) return saveProjectField({ [projectKey]: event.target.value });
  const choiceKey = event.target.dataset?.prefSelect;
  if (choiceKey) return savePref({ [choiceKey]: event.target.value });
  const textKey = event.target.dataset?.prefText;
  if (textKey) return savePref({ [textKey]: event.target.value });
  const key = event.target.dataset?.prefNumber;
  if (key) {
    const parsed = Number(event.target.value);
    if (Number.isFinite(parsed)) return savePref({ [key]: Math.round(parsed) });
    return render();
  }
  if (isImportDialog() && imports.onChange(event.target)) return;
  if (event.target.dataset.toggleState) {
    state[event.target.dataset.toggleState] = event.target.checked;
    await loadSessions();
  }
});

addEventListener("keydown", async (event) => {
  // The shortcut editor on the settings page takes the next press whole.
  if (state.page === "settings" && state.capturing && !state.dialog) {
    event.preventDefault();
    if (event.key === "Escape") {
      state.capturing = null;
      return render();
    }
    const pressed = capture(event);
    if (!pressed) return;
    const action = state.capturing;
    state.capturing = null;
    return savePref({ shortcuts: { ...(state.settings.shortcuts ?? {}), [action]: pressed } });
  }
  if (event.target.id === "find-term" && (event.key === "Enter" || event.key === "Escape")) {
    event.preventDefault();
    if (event.key === "Escape") return closeFind();
    return findStep({ previous: event.shiftKey });
  }
  if (event.key === "Escape") {
    if (state.menu || state.dialog) return closeDialog();
    if (state.page === "settings") return closeSettings();
    return;
  }
  if (state.dialog?.kind === "palette") {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      const count = state.dialog.results.length || 1;
      state.dialog.index = (state.dialog.index + step + count) % count;
      return render();
    }
    if (event.key === "Enter") {
      event.preventDefault();
      return runPalette(state.dialog.index);
    }
    return;
  }
  if (event.key === "Enter" && state.dialog && event.target.tagName === "INPUT") {
    if (isImportDialog() && imports.onEnter(event.target)) { event.preventDefault(); return; }
    const confirmAction = document
      .querySelector(".modal__foot .button--primary, .modal__foot .button--danger-solid")
      ?.getAttribute("data-action");
    if (confirmAction && ACTIONS[confirmAction]) {
      event.preventDefault();
      ACTIONS[confirmAction]();
    }
  }
});

// Bindings are matched while the event is still on its way down, before the
// terminal sees it: xterm consumes keys like Tab and stops them there, so a
// bubbling listener never learned of ⌃Tab or ⌘. pressed inside a session.
// Keys nothing is bound to pass through untouched.
addEventListener(
  "keydown",
  (event) => {
    if (state.capturing || state.dialog?.kind === "palette") return;
    // ⌘1–⌘9 (Ctrl elsewhere) select a tab, as in the macOS app.
    const primary = /mac/i.test(navigator.platform) ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
    // ⌘+ / ⌘− / ⌘0 zoom the focused terminal only.
    if (primary && !event.altKey && state.sessionId && !state.dialog && ["Equal", "NumpadAdd", "Minus", "NumpadSubtract", "Digit0", "Numpad0"].includes(event.code)) {
      event.preventDefault();
      event.stopPropagation();
      const step = /Equal|Add/.test(event.code) ? 1 : /Minus|Subtract/.test(event.code) ? -1 : 0;
      const size = terminal.zoom(state.sessionId, step);
      if (size) toast(`Terminal text ${size} pt`);
      return;
    }
    const digit = /^Digit([1-9])$/.exec(event.code ?? "");
    if (primary && digit && !event.altKey && !event.shiftKey && state.tabs[Number(digit[1]) - 1]) {
      event.preventDefault();
      event.stopPropagation();
      return openTab(state.tabs[Number(digit[1]) - 1]);
    }
    const bound = SHORTCUTS.find(([action]) => matches(event, binding(action)));
    if (!bound) return;
    event.preventDefault();
    event.stopPropagation();
    SHORTCUT_ACTIONS[bound[0]]();
  },
  true,
);

addEventListener("resize", terminal.refit);

matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
  if (state.settings.theme === "system") terminal.applySettings();
});

// ------------------------------------------------------------ operations --

async function openFolder() {
  const chosen = await openDialogNative({ directory: true, multiple: false });
  if (!chosen) return;
  const added = await call("project_open", { path: chosen });
  if (added === undefined) return;
  if (added > 1) toast(`Added ${added} projects`);
  await loadWorkspace();
  // The folder just opened is what to show next.
  if (state.page === "home" || state.page === "dashboard") state.page = null;
  render();
}

async function reconnectProject() {
  const chosen = await openDialogNative({ directory: true, multiple: false });
  if (!chosen) return;
  if (await done("project_reconnect", { id: state.projectId, path: chosen })) {
    await loadWorkspace();
  }
}

/// Project settings is a page in the main area, as on macOS.
async function openProjectSettings() {
  if (!(await reloadProjectDraft())) return;
  const icon = state.projectDraft.icon ?? "";
  state.iconTab = icon.startsWith("sf:") ? "symbol" : /^(gh|img):/.test(icon) ? "image" : "emoji";
  state.page = "project";
  state.sessionId = null;
  state.files = null;
  state.menu = null;
  state.dialog = null;
  render();
}

async function reloadProjectDraft() {
  const detail = await call("project_detail", { id: state.projectId });
  if (!detail) return false;
  state.projectDraft = { ...project(), ...detail };
  return true;
}

function closeProjectSettings() {
  state.page = null;
  state.projectDraft = null;
  render();
}

/// Saves one change. The page then shows what was stored — a rejected value
/// is reported and put back.
async function saveProjectField(input) {
  const id = state.projectDraft?.id;
  if (!id) return;
  const saved = await done("project_edit", { id, input });
  await loadWorkspace();
  await reloadProjectDraft();
  render();
  return saved;
}

async function setProjectImage(source) {
  const id = state.projectDraft?.id;
  let value = "";
  if (source === "upload") {
    const chosen = await openDialogNative({
      multiple: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
    });
    if (!chosen) return;
    value = chosen;
  }
  if (source === "favicon") {
    value = document.querySelector("#proj-favicon")?.value.trim() ?? "";
    if (!value) return toast("Enter a domain such as example.com.");
  }
  state.iconBusy = true;
  render();
  const icon = await call("project_icon_set", { id, source, value });
  state.iconBusy = false;
  if (icon) forgetIcon(icon.replace(/^(gh|img):/, ""));
  await loadWorkspace();
  await reloadProjectDraft();
  render();
}

// ---------------------------------------------------------- new session ---

/// A session's name from its first message, else the agent and the time —
/// the macOS app's rule.
function autoTitle(agent, prompt) {
  const first = (prompt ?? "")
    .split("\n")
    .map((line) => line.trim())
    .find(Boolean)
    ?.replace(/^[#*>\-\s]+/, "");
  if (first) return first.length > 60 ? `${first.slice(0, 57).trim()}…` : first;
  const when = new Date().toLocaleString("en-US", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });
  return `${agent === "claude" ? "Claude Code" : "Codex"} · ${when}`;
}

async function openNewSession(projectId) {
  if (!projectId) return toast("Open a folder first.");
  await loadProfiles();
  const draft = {
    kind: "session",
    projectId: null,
    title: "",
    model: "",
    prompt: "",
    notes: "",
    setup: "run",
    keepOpen: false,
    advanced: false,
  };
  openModal(draft);
  await sessionProject(draft, projectId);
}

/// Points the sheet at a project: its default agent, its refs, and whether
/// it is a repository at all.
async function sessionProject(draft, projectId) {
  const target = state.projects.find((item) => item.id === projectId);
  draft.projectId = projectId;
  draft.agent = target?.default_agent || state.settings.default_agent;
  draft.account = activeAccount(draft.agent);
  draft.worktree = !!state.settings.worktree_by_default;
  draft.branchEdited = false;
  draft.branch = suggestBranch(draft.title || "session", projectId);
  draft.refs = [];
  draft.repo = false;
  draft.base = "";
  render();
  const [refs, detail] = await Promise.all([
    call("git_refs", { projectId }),
    call("project_detail", { id: projectId }),
  ]);
  if (state.dialog !== draft || draft.projectId !== projectId) return;
  draft.refs = refs ?? [];
  draft.repo = draft.refs.length > 0;
  draft.defaultBase = detail?.base_ref || draft.refs.find((ref) => !ref.startsWith("origin/") && !ref.startsWith("tag:")) || "";
  draft.setupPreview = [state.settings.worktree_setup, detail?.setup_command]
    .filter((part) => part && part.trim())
    .join("\n");
  render();
}

async function createSession() {
  captureDraft();
  const draft = state.dialog;
  if (draft.busy) return;
  const title = draft.title.trim() || autoTitle(draft.agent, draft.prompt);
  const target = draft.projectId;
  draft.busy = true;
  render();
  const id = await call("session_create", {
    projectId: target,
    agent: draft.agent,
    title,
    prompt: draft.prompt,
    model: draft.model,
    profileId: draft.account || null,
    notes: draft.notes,
  });
  draft.busy = false;
  if (!id) return render();
  rememberAccount(draft.agent, draft.account);
  const wantsWorktree = draft.repo && draft.worktree;
  const branch = (draft.branch ?? "").trim() || suggestBranch(title, target);
  const base = (draft.base ?? "").trim();
  const setup = draft.setup;
  if (draft.keepOpen) {
    Object.assign(draft, { title: "", prompt: "", notes: "", branchEdited: false, branch: suggestBranch("session", target) });
    toast(`Started ${title}`);
  } else {
    closeDialog();
  }
  await loadWorkspace();
  if (target !== state.projectId) await selectProject(target);
  if (!draft.keepOpen) openSession(id);

  if (wantsWorktree) {
    const made = await attempt("worktree_create", { id, branch, base: base || null });
    if (!made.ok) {
      toast(`Started in the project folder: ${made.error}`);
    } else {
      await loadSessions();
      const plan = made.value;
      if (setup !== "skip" && (plan.shared_paths.length || plan.setup_command)) {
        toast("Running worktree setup…");
        if (!(await done("worktree_setup", { id }))) return;
      }
    }
  }
  // Creating is starting, as on macOS; the first message goes with it.
  await terminal.start(id, { background: draft.keepOpen });
}

async function openEditSession() {
  const current = session();
  const detail = await call("session_detail", { id: current.id });
  if (!detail) return;
  openModal({ kind: "edit-session", ...detail });
}

async function saveSession() {
  captureDraft();
  const dialog = state.dialog;
  const input = { title: dialog.title, notes: dialog.notes };
  if (!dialog.running) input.provider_id = dialog.provider_id;
  if (!(await done("session_edit", { id: dialog.id, input }))) return;
  closeDialog();
  await loadSessions();
}

async function openReview() {
  const current = session();
  state.menu = null;
  const brief = await call("review_brief", { id: current.id });
  if (brief === undefined) return;
  openModal({
    kind: "review",
    id: current.id,
    brief,
    reviewer: current.agent === "claude" ? "codex" : "claude",
  });
}

async function createReview() {
  captureDraft();
  const dialog = state.dialog;
  const id = await call("review_create", { id: dialog.id, prompt: dialog.brief });
  if (!id) return;
  closeDialog();
  await loadSessions();
  openSession(id);
}

async function insertFeedback() {
  captureDraft();
  const text = value("draft-feedback") || state.dialog.feedback || "";
  const builder = await call("review_builder", { id: state.sessionId });
  if (!builder) return;
  if (await terminal.paste(builder, text, false)) {
    closeDialog();
    toast("Feedback inserted; press Enter in the builder to send it");
  }
}

async function openSavedOutput() {
  const current = session();
  state.menu = null;
  const text = await call("session_output", { id: current.id });
  if (text === undefined) return;
  openModal({ kind: "output", title: current.title, text: text || "Nothing saved yet." });
}

async function openUsage() {
  const current = session();
  openModal({ kind: "usage", loading: true });
  const usage = await call("usage_read", { id: current.id });
  if (!state.dialog || state.dialog.kind !== "usage") return;
  state.dialog = { kind: "usage", loading: false, ...(usage ?? { windows: [], note: "" }) };
  render();
}

/// A branch name from a title, under the project's branch prefix, else the
/// one set in Settings.
const suggestBranch = (title, projectId = state.projectId) =>
  `${state.projects.find((item) => item.id === projectId)?.branch_prefix || state.settings.branch_prefix || "convoy"}/${title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "work"}`;

async function confirmWorktree() {
  captureDraft();
  const branch = state.dialog.branch;
  const id = state.sessionId;
  const plan = await call("worktree_create", { id, branch });
  if (!plan) return;
  await loadSessions();
  if (plan.shared_paths.length || plan.setup_command) {
    openModal({
      kind: "worktree-setup",
      id,
      shared: plan.shared_paths,
      command: plan.setup_command ?? "",
      directory: plan.directory,
    });
  } else {
    closeDialog();
    toast(`Worktree created on ${plan.branch}`);
  }
}

async function openRemoveWorktree() {
  state.menu = null;
  const plan = await call("worktree_plan_remove", { id: state.sessionId });
  if (!plan) return;
  openModal({ kind: "remove-worktree", id: state.sessionId, ...plan });
}

/// Settings is a page, as on macOS, and every change is saved as it is made.
async function openSettings(section) {
  state.page = "settings";
  if (section) state.settingsSection = section;
  state.capturing = null;
  state.menu = null;
  state.dialog = null;
  state.profileAgent ??= state.settings.default_agent;
  render();
  await Promise.all([
    loadProfiles(),
    call("integrations_list").then((list) => {
      if (list) state.integrations = list;
    }),
    call("worktrees_path").then((path) => {
      if (path) state.worktrees = path;
    }),
  ]);
  render();
  if (state.settingsSection === "Setup") runDiagnostics();
  checkMacImport().then(render);
}

/// The macOS app's workspace, when this Mac has one. Offered once on its own
/// when this workspace is still empty; Settings → Setup offers it any time.
const MAC_OFFERED = "convoy.macImportOffered";

async function checkMacImport() {
  state.macImport = (await call("macos_import_preview", { settings: false })) ?? null;
  return state.macImport;
}

async function offerMacImport(offer) {
  const settings = offer;
  const report = await call("macos_import_preview", { settings });
  if (!report) return toast("There is no macOS app workspace on this Mac.");
  openModal({ kind: "macos-import", report, settings, offer });
}

async function runMacImport() {
  const dialog = state.dialog;
  if (!dialog || dialog.busy) return;
  dialog.busy = true;
  render();
  const report = await call("macos_import_run", { settings: !!dialog.settings });
  try {
    localStorage.setItem(MAC_OFFERED, "1");
  } catch {}
  if (!report) {
    dialog.busy = false;
    return render();
  }
  closeDialog();
  if (dialog.settings) {
    await loadSettings();
    applyTheme();
  }
  await loadWorkspace();
  await loadPlanning();
  loadActivity();
  checkMacImport();
  toast(`Imported ${plural(report.projects, "project")} and ${plural(report.sessions, "session")}`);
  if (report.warnings.length) openModal({ kind: "macos-import-done", warnings: report.warnings });
}

async function runDiagnostics() {
  state.diagnostics = null;
  render();
  state.diagnostics = (await call("diagnostics_run")) ?? [];
  render();
}

function closeSettings() {
  state.page = null;
  state.capturing = null;
  render();
}

/// Saves one change. The core validates the whole set, so a rejected value
/// is reported and the page shows what is actually stored.
async function savePref(patch) {
  const input = { ...state.settings, shortcuts: { ...(state.settings.shortcuts ?? {}) }, ...patch };
  const saved = await done("settings_save", { input });
  await loadSettings();
  if (saved) {
    terminal.applySettings();
    await applyKeepAwake();
    if (["git_status", "git_poll_seconds", "compact_sidebar"].some((key) => key in patch)) scheduleGitPolling();
  }
  render();
}

async function addPageProfile() {
  const label = value("pref-profile-label").trim();
  if (!label) return toast("Give the account a label.");
  const before = new Set((state.profiles ?? []).map((item) => item.id));
  if (!(await done("profile_add", { label, agent: state.profileAgent }))) return;
  await loadProfiles();
  render();
  // As on macOS: a new account opens its login straight away.
  const added = state.profiles.find((item) => !before.has(item.id));
  if (added) await openLogin(added.id);
}

/// The provider's own login for an account, in a terminal of its own. It is
/// not a session: closing the dialog ends it, and nothing is saved.
async function openLogin(profileId) {
  const profile = (state.profiles ?? []).find((item) => item.id === profileId);
  if (!profile) return;
  const id = `login:${profileId}`;
  openModal({ kind: "login", id, label: profile.label, agent: profile.agent, finished: false });
  terminal.mount(id, "#login-host");
  const { terminal: xterm } = terminal.terminalFor(id);
  const started = await call("account_login", { profileId, cols: xterm.cols || 100, rows: xterm.rows || 28 });
  if (started === undefined && state.dialog?.id === id) closeDialog();
  xterm.focus();
}

/// Right-click and "…" menus in the sidebar.
function openContextMenu(kind, id, x, y) {
  state.dialog = null;
  state.menu = { kind, id, x, y };
  render();
}

async function runMenu(act) {
  const menu = state.menu;
  state.menu = null;
  if (!menu?.kind) return render();
  if (menu.kind === "project") {
    const target = state.projects.find((item) => item.id === menu.id);
    if (!target) return render();
    if (state.projectId !== target.id) await selectProject(target.id);
    switch (act) {
      case "new-session":
        return ACTIONS["new-session"]();
      case "import-history":
        return openTranscripts();
      case "reveal":
        render();
        return call("path_reveal", { path: target.path });
      case "copy-path":
        await navigator.clipboard.writeText(target.path);
        render();
        return toast("Path copied");
      case "reconnect":
        render();
        return reconnectProject();
      case "project-settings":
        return openProjectSettings();
      case "remove":
        return ACTIONS["remove-project"]();
    }
    return render();
  }
  if (menu.kind === "task") return runTaskMenu(act, menu.id);
  // A pinned session may belong to another project.
  const owner = anySession(menu.id)?.project_id;
  if (owner && owner !== state.projectId) await selectProject(owner);
  const target = state.sessions.find((item) => item.id === menu.id);
  if (!target) return render();
  switch (act) {
    case "open":
      return openSession(target.id);
    case "pin":
      if (await done("session_pin", { id: target.id, pinned: !target.pinned })) await loadSessions();
      return render();
    case "start":
      return terminal.start(target.id);
    case "sleep":
      await done("session_hibernate", { id: target.id });
      return render();
    case "stop":
      return openModal({
        kind: "confirm",
        title: "Stop this agent?",
        body: "This interrupts current work. The saved conversation stays with the agent.",
        confirmLabel: "Stop",
        danger: true,
        run: () => terminal.stop(target.id),
      });
    case "edit":
      state.sessionId = target.id;
      return openEditSession();
    case "review":
      state.sessionId = target.id;
      return openReview();
    case "archive":
      if (await done("session_archive", { id: target.id, archived: !target.archived })) {
        if (!target.archived && state.sessionId === target.id) state.sessionId = null;
        await loadSessions();
      }
      return render();
    case "reveal-worktree":
      render();
      return call("path_reveal", { path: target.working_directory });
    case "copy-worktree":
      await navigator.clipboard.writeText(target.working_directory);
      render();
      return toast("Path copied");
    case "remove-worktree":
      state.sessionId = target.id;
      return openRemoveWorktree();
  }
  return render();
}

async function loadProfiles() {
  state.profiles = (await call("profiles_read")) ?? [];
}

async function openAccounts() {
  await openSettings("Accounts");
}

async function addProfile() {
  captureDraft();
  const dialog = state.dialog;
  if (!(await done("profile_add", { label: dialog.label, agent: dialog.agent }))) return;
  await loadProfiles();
  state.dialog.label = "";
  render();
}

async function openTranscripts() {
  await loadProfiles();
  openModal({
    kind: "transcripts",
    agent: state.settings.default_agent,
    profile: "",
    found: null,
    scanning: false,
  });
}

async function scanTranscripts() {
  const dialog = state.dialog;
  dialog.scanning = true;
  render();
  const found = await call("transcripts_scan", {
    projectId: state.projectId,
    agent: dialog.agent,
    profileId: dialog.profile || null,
  });
  dialog.scanning = false;
  dialog.found = found ?? [];
  render();
}

async function importTranscript(providerId, title) {
  const dialog = state.dialog;
  const id = await call("transcripts_import", {
    projectId: state.projectId,
    agent: dialog.agent,
    providerId,
    title: title ?? "",
    profileId: dialog.profile || null,
  });
  if (!id) return;
  await loadSessions();
  toast("Imported");
}

async function addQuickCommand() {
  captureDraft();
  const dialog = state.dialog;
  const saved = await done("quick_command_save", {
    id: "",
    title: dialog.title,
    text: dialog.text,
    submit: dialog.submit,
    projectId: dialog.scoped ? state.projectId : null,
  });
  if (!saved) return;
  await loadSessions();
  state.dialog.title = "";
  state.dialog.text = "";
  render();
}

/// Reads the feed; re-renders only when something new arrived, since this
/// runs every few seconds for the bell's badge.
async function loadActivity() {
  const events = (await attempt("activity_read")).value ?? state.activity ?? [];
  const changed = events.length !== (state.activity?.length ?? -1) || events[0]?.id !== state.activity?.[0]?.id;
  state.activity = events;
  if (changed) render();
}

// The last event seen in the feed, kept per machine like the macOS badge.
try {
  state.activitySeen = localStorage.getItem("convoy.activitySeen") ?? null;
} catch {
  state.activitySeen = null;
}

function markActivitySeen() {
  state.activitySeen = state.activity?.[0]?.at ?? state.activitySeen;
  try {
    if (state.activitySeen) localStorage.setItem("convoy.activitySeen", state.activitySeen);
  } catch {
    /* private storage unavailable: the badge simply resets next launch */
  }
}

async function openActivity() {
  state.dialog = null;
  state.menu = { kind: "activity" };
  render();
  await loadActivity();
  markActivitySeen();
  render();
}

// ------------------------------------------------------------------ docs ---

async function loadDocs() {
  const id = state.projectId;
  if (!id) return;
  const files = (await call("docs_list", { projectId: id })) ?? [];
  const previous = state.docs?.projectId === id ? state.docs : null;
  state.docs = {
    projectId: id,
    files,
    selected: previous?.selected && files.includes(previous.selected) ? previous.selected : null,
    text: "",
    editing: false,
    dirty: false,
  };
  if (state.docs.selected) await openDoc(state.docs.selected);
  else render();
}

async function openDoc(path) {
  // As on macOS, opening another file leaves unsaved edits behind.
  state.docs = { ...state.docs, selected: path, text: "", editing: false, dirty: false, loading: true };
  render();
  const text = await call("doc_read", { projectId: state.docs.projectId, path });
  state.docs = { ...state.docs, text: text ?? "", loading: false };
  render();
}

async function saveDoc() {
  const docs = state.docs;
  if (!(await done("doc_write", { projectId: docs.projectId, path: docs.selected, text: docs.text }))) return;
  state.docs = { ...docs, editing: false, dirty: false };
  render();
  toast("Saved");
}

/// Starts Claude with one of the Docs prompts, as the macOS app does.
async function startDocSession(title, prompt) {
  const id = await call("session_create", {
    projectId: state.projectId,
    agent: "claude",
    title,
    prompt,
    model: "",
  });
  if (!id) return;
  await loadSessions();
  await terminal.start(id);
}

const docTitle = () => {
  const heading = (state.docs?.text ?? "").split("\n").find((line) => line.startsWith("# "));
  return heading ? heading.slice(2).trim() : state.docs?.selected ?? "";
};

Object.assign(ACTIONS, {
  activity: () => (state.menu?.kind === "activity" ? closeDialog() : openActivity()),
  "activity-clear": async () => {
    if (await done("activity_clear")) {
      state.activity = [];
      markActivitySeen();
      render();
    }
  },
  "doc-reload": () => loadDocs(),
  "doc-edit": () => {
    state.docs.editing = true;
    state.docs.focusEditor = true;
    render();
  },
  "doc-done": () => {
    state.docs.editing = false;
    render();
  },
  "doc-save": () => saveDoc(),
  "doc-copy-path": async () => {
    await navigator.clipboard.writeText(`${project()?.path ?? ""}/${state.docs.selected}`);
    toast("Path copied");
  },
  "doc-new-spec": () => openModal({ kind: "doc-spec", title: "" }),
  "doc-create-spec": async () => {
    captureDraft();
    const title = state.dialog.title?.trim();
    if (!title) return toast("Give the spec a title.");
    const path = await call("spec_create", { projectId: state.projectId, title });
    if (!path) return;
    closeDialog();
    await loadDocs();
    await openDoc(path);
    state.docs.editing = true;
    state.docs.focusEditor = true;
    render();
  },
  "doc-write-project": async () => {
    const prompts = await call("doc_prompts", { path: PROJECT_DOC, title: "" });
    if (prompts) await startDocSession("Write project doc", prompts.project_doc);
  },
  "doc-draft-spec": async () => {
    const title = docTitle();
    const prompts = await call("doc_prompts", { path: state.docs.selected, title });
    if (prompts) await startDocSession(`Spec: ${title}`, prompts.spec);
  },
  "doc-task": async () => {
    const path = state.docs.selected;
    const title = docTitle();
    await showTab("tasks");
    openModal({
      kind: "task",
      title,
      details: `Implement the specification in ${path}. Read it first and treat its acceptance criteria as the definition of done.`,
      findings: "",
      agent: project()?.default_agent || state.settings.default_agent,
      mode: project()?.task_mode || "none",
      auto_review: false,
      spec_id: "",
    });
  },
});

// ------------------------------------------------------------- planning ---

function openSpec(id) {
  const spec = state.planning?.specs.find((item) => item.id === id);
  if (!spec) return;
  openModal({ kind: "spec", ...spec });
}

async function saveSpec() {
  captureDraft();
  const dialog = state.dialog;
  const saved = await done("spec_save", {
    input: {
      id: dialog.id ?? null,
      project_id: state.projectId,
      title: dialog.title,
      problem: dialog.problem,
      requirements: dialog.requirements,
      acceptance: dialog.acceptance,
      constraints: dialog.constraints,
      plan: dialog.plan,
    },
  });
  if (!saved) return;
  closeDialog();
  await loadPlanning();
}

async function approveSpec() {
  const dialog = state.dialog;
  if (!(await done("spec_approve", { id: dialog.id, revision: dialog.revision }))) return;
  closeDialog();
  await loadPlanning();
}

async function exportSpec(id) {
  const text = await call("spec_markdown", { id });
  if (text === undefined) return;
  const path = await saveDialogNative({
    defaultPath: "specification.md",
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (!path) {
    return openModal({ kind: "markdown", title: "Specification", text });
  }
  const { writeTextFile } = await import("@tauri-apps/plugin-fs");
  try {
    await writeTextFile(path, text);
    toast(`Exported to ${path}`);
  } catch (error) {
    toast(error, "error");
  }
}

function openTask(id) {
  const task = state.planning?.tasks.find((item) => item.id === id);
  if (!task) return;
  openModal({ kind: "task", ...task });
}

/// Saves the form; with `run`, starts the task straight away, as the macOS
/// sheet's "Run now" does.
async function saveTask(run = false) {
  captureDraft();
  const dialog = state.dialog;
  const id = await call("task_save", {
    input: {
      id: dialog.id ?? null,
      project_id: dialog.project_id ?? state.projectId,
      spec_id: dialog.spec_id || null,
      title: dialog.title,
      details: dialog.details,
      findings: dialog.findings,
      agent: dialog.agent,
      mode: dialog.mode,
      auto_review: dialog.auto_review,
    },
  });
  if (!id) return;
  const model = (dialog.model ?? "").trim();
  const before = findTask(id);
  if (model !== (before?.model ?? "") || (before && before.agent !== dialog.agent)) {
    if (!(await done("task_agent", { id, agent: dialog.agent, model }))) return;
  }
  closeDialog();
  await reloadTasks();
  if (run) await runTask(id);
}

async function setTaskStatus(status) {
  const id = state.dialog.id;
  if (!(await done("task_status", { id, status }))) return;
  closeDialog();
  await reloadTasks();
}

const findTask = (id) =>
  state.planning?.tasks.find((item) => item.id === id) ?? state.allTasks?.find((item) => item.id === id);

async function loadAllTasks() {
  state.allTasks = (await call("tasks_all")) ?? [];
}

/// Reloads whichever task lists are on screen.
async function reloadTasks() {
  await loadPlanning();
  if (state.taskScope === "all") await loadAllTasks();
  render();
}

/// Prepares the task's session and starts it, as Run does on macOS.
async function runTask(id) {
  const sessionId = await call("task_prepare", { id, profileId: null });
  if (!sessionId) return;
  await loadWorkspace();
  await reloadTasks();
  await openAnywhere(sessionId);
  await terminal.start(sessionId);
  await reloadTasks();
}

async function moveTask(id, status) {
  const task = findTask(id);
  if (!task || task.status === status) return;
  if (status === "building") return runnable(task) ? runTask(id) : toast("Only a queued, failed or returned task can run.");
  if (await done("task_status", { id, status })) await reloadTasks();
}

async function runTaskMenu(act, id) {
  const task = findTask(id);
  if (!task) return render();
  if (act.startsWith("task-status:")) return moveTask(id, act.slice("task-status:".length));
  switch (act) {
    case "task-run":
      return runTask(id);
    case "task-session":
      return openAnywhere(task.session_id);
    case "task-reviewer":
      return openAnywhere(task.review_session_id);
    case "task-pr":
      render();
      return done("task_pr_open", { id });
    case "task-issue":
      render();
      return done("issue_open", { id });
    case "task-edit":
      return openModal({ kind: "task", ...task });
    case "task-delete":
      return openModal({
        kind: "confirm",
        title: `Delete "${task.title}"?`,
        body: "The task goes; its session and anything the agent changed stay.",
        confirmLabel: "Delete",
        danger: true,
        run: async () => {
          if (await done("task_delete", { id })) await reloadTasks();
        },
      });
  }
  return render();
}

async function prepareTask(id) {
  const sessionId = await call("task_prepare", { id, profileId: null });
  if (!sessionId) return;
  await loadSessions();
  await loadPlanning();
  openSession(sessionId);
}

async function startQueue() {
  const summary = await call("queue_summary", { projectId: state.projectId });
  if (!summary?.length) return toast("No queued tasks.");
  openModal({ kind: "queue", summary });
}

/// Starts the next queued task. The rules are the core's; this supplies the
/// clock and the starting.
async function advanceQueue(projectId) {
  if (!state.queues.has(projectId)) return;
  await loadPlanning();
  const next = state.planning?.tasks.find((task) => task.status === "queued");
  if (!next) {
    state.queues.delete(projectId);
    return render();
  }
  const sessionId = await call("task_prepare", { id: next.id, profileId: null });
  if (!sessionId) {
    state.queues.delete(projectId);
    return render();
  }
  await loadSessions();
  await terminal.start(sessionId);
}

// ---------------------------------------------------------------- files ---

async function openFiles() {
  const id = state.sessionId ?? state.projectId;
  if (!id) return;
  state.files = {
    id,
    view: "changes",
    snapshot: null,
    selection: null,
    preview: null,
    hash: "",
    sideBySide: false,
    message: "",
    branch: null,
  };
  render();
  await loadFiles();
}

async function loadFiles() {
  if (!state.files) return;
  const snapshot = await call("files_snapshot", { id: state.files.id });
  if (!snapshot || !state.files) return;
  state.files.snapshot = snapshot;
  render();
  if (state.files.selection) await readSelection();
}

/// A generation counter makes a slow read harmless: if the selection has moved
/// on by the time it returns, the result is dropped.
let generation = 0;
async function readSelection() {
  const files = state.files;
  if (!files?.selection) return render();
  generation += 1;
  const mine = generation;
  render();
  const preview = await call("files_read", {
    id: files.id,
    selection: files.selection,
    sideBySide: files.sideBySide,
  });
  if (mine !== generation || !state.files) return;
  state.files.preview = preview ?? null;
  state.files.hash = preview?.hash ?? "";
  render();
}

const selectedPath = () => state.files?.selection?.path ?? "";
const selectedCommit = () => state.files?.selection?.commit ?? "";
const selectedOriginal = () =>
  state.files?.snapshot?.changes.find((change) => change.path === selectedPath())?.original ?? null;

function confirmThen(title, body, run) {
  openModal({ kind: "confirm", title, body, confirmLabel: "Continue", danger: true, run });
}

async function mutate(mutation) {
  if (!state.files) return;
  if (mutation.path === "" && "path" in mutation) return toast("Select a file first.");
  if (mutation.commit === "" && "commit" in mutation) return toast("Select a commit first.");
  state.menu = null;
  const output = await call("files_mutate", { id: state.files.id, mutation });
  if (output === undefined) return;
  if (output.trim()) toast(output.trim().slice(0, 160));
  await loadFiles();
}

async function trashFile() {
  const path = selectedPath();
  if (!path) return toast("Select a file first.");
  confirmThen("Move untracked file to Trash?", path, async () => {
    if (await done("files_trash", { id: state.files.id, path })) await loadFiles();
  });
}

async function openHunkChoice() {
  const path = selectedPath();
  if (!path || state.files.selection.kind !== "unstaged") {
    return toast("Hunks can only be discarded from unstaged changes.");
  }
  const hunks = await call("files_hunks", { id: state.files.id, path });
  if (!hunks?.length) return toast("This hunk cannot be discarded separately.");
  openModal({ kind: "hunk", path, hunks, hunk: 0 });
}

async function confirmHunk() {
  const dialog = state.dialog;
  const hunk = Number(dialog.hunk ?? 0);
  const path = dialog.path;
  const hash = state.files.hash;
  closeDialog();
  await mutate({ action: "discardHunk", path, hunk, hash });
}

async function commit(amend) {
  const message = document.querySelector("#commit-message")?.value ?? state.files.message;
  state.files.message = message;
  await mutate({ action: "commit", message, amend });
  state.files.message = "";
  render();
}

async function generateMessage() {
  const existing = document.querySelector("#commit-message")?.value ?? "";
  const run = async () => {
    toast("Asking Claude for a commit message…");
    const message = await call("commit_generate", { id: state.files.id });
    if (message === undefined) return;
    state.files.message = message;
    render();
  };
  if (existing.trim()) {
    return confirmThen(
      "Replace the message you have written?",
      "Generating asks Claude for a new message for the staged diff.",
      run,
    );
  }
  await run();
}

async function createPullRequest() {
  state.menu = null;
  const url = await call("pr_create", { id: state.files.id });
  if (url) toast(url);
}

// ------------------------------------------------------------------ find --

/// Whether a session's terminal is what the window shows.
const terminalShown = () =>
  !!state.sessionId && !state.files && state.page !== "settings" && state.page !== "project" && !state.dialog;

function openFind() {
  if (!state.find) state.find = { term: "", caseSensitive: false, index: -1, count: 0 };
  render();
  const field = document.querySelector("#find-term");
  field?.focus();
  field?.select();
}

function closeFind() {
  const id = state.sessionId;
  state.find = null;
  if (id) terminal.endFind(id);
  render();
}

function findStep({ previous = false, incremental = false } = {}) {
  const find = state.find;
  if (!find || !state.sessionId) return;
  terminal.find(state.sessionId, find.term, { previous, incremental, caseSensitive: find.caseSensitive });
}

terminal.onFindResults((id, index, count) => {
  if (!state.find || id !== state.sessionId) return;
  if (state.find.index === index && state.find.count === count) return;
  state.find.index = index;
  state.find.count = count;
  render();
});

// -------------------------------------------------------------- palette ---

/// Everything the palette can reach: every project's sessions, the projects,
/// and the macOS app's list of actions, each with its shortcut.
function paletteEntries() {
  const entries = [];
  for (const item of state.allSessions) {
    const owner = state.projects.find((entry) => entry.id === item.project_id);
    const running = isRunning(item.id);
    const reported = running ? state.agentState.get(item.id) : null;
    entries.push({
      kind: "Session",
      title: item.title,
      subtitle: [
        owner?.title,
        item.agent === "claude" ? "Claude Code" : "Codex",
        item.branch ? `⎇ ${item.branch}` : "",
        item.review_of ? "review" : "",
        reported ?? "",
      ]
        .filter(Boolean)
        .join(" · "),
      running,
      needsYou: reported === "waiting",
      run: () => openAnywhere(item.id),
    });
  }
  for (const item of state.projects) {
    entries.push({ kind: "Project", title: item.title, subtitle: item.path, run: () => selectProject(item.id).then(render) });
  }
  const bound = (action) => label(binding(action));
  const inFiles = (then) => async () => {
    if (!state.files) await openFiles();
    await then();
  };
  const actions = [
    ["Home", "Every project at once", "home"],
    ["Agent Dashboard", "Every session by what its agent is doing", "dashboard"],
    ["New session", "Creates a Claude Code or Codex session", "newSession"],
    ["Open folder…", "Add a repository or a folder of projects", "openFolder"],
    ["Resume current session", "Reconnect to the selected agent", "resume"],
    ["Stop current session", "Interrupts the running agent", "stop"],
    ["Sleep current session", "Stops it; opening it resumes the conversation", "sleep"],
    ["Close tab", "Closes the selected terminal tab", "closeTab"],
    ["Reopen closed tab", "", "reopenTab"],
    ["Switch terminal", "Open terminals, most recent first", "switcher"],
    ["Find in terminal", "", "search"],
    ["Toggle sidebar", "Show or hide the project list", "sidebar"],
    ["Edit session name & notes", "", "edit"],
    ["Pin or unpin session", "", "pin"],
    ["Start review of current session", "Hands the work to the other agent", "review"],
    ["Send feedback to builder", "From a review session", "feedback"],
    ["Quick commands", "Send a saved command to this terminal", "quick"],
    ["One pane", "", "layout1"],
    ["Two panes", "", "layout2"],
    ["Four panes", "", "layout4"],
    ["AI Limits", "Account usage for Codex and Claude", "limits"],
    ["Refresh AI limits", "Reads the current usage from your login", "limitsRefresh"],
    ["Go to Sessions", "", "sessionsTab"],
    ["Go to Reviews", "", "reviewsTab"],
    ["Go to Specs", "", "specsTab"],
    ["Go to Tasks", "", "tasksTab"],
    ["Go to Docs & Specs", "", "docsTab"],
    ["Next section", "", "nextTab"],
    ["Previous section", "", "previousTab"],
    ["New specification", "", "newSpec"],
    ["New task", "Queue work for an agent in the current project", "newTask"],
    ["Import Linear or Jira issues", "As tasks in the current project", "importIssues"],
    ["Show project folder", "", "reveal"],
    ["Copy project path", "", "copyPath"],
    ["Toggle keep awake", "", "wake"],
    ["Activity", "What the agents did, newest first", "activity"],
    ["Settings…", "Appearance, terminal, agents, git, notifications", "settings"],
    ["Files & Changes", "Browse files, stage, commit, push, log", "files"],
  ].map(([title, subtitle, action]) => ({ kind: "Action", title, subtitle, shortcut: bound(action), run: () => SHORTCUT_ACTIONS[action]() }));
  const more = [
    ["Theme: System", "Follow the system appearance", () => savePref({ theme: "system" })],
    ["Theme: Light", "", () => savePref({ theme: "light" })],
    ["Theme: Dark", "", () => savePref({ theme: "dark" })],
    ["Setup check", "Find missing CLIs and configuration problems", () => openSettings("Setup")],
    ["Accounts", "Sign-ins for Claude Code and Codex", openAccounts],
    ["Project settings", "", openProjectSettings],
    ["Import provider history", "Conversations the agent already has for this folder", openTranscripts],
    ["Git: Commit…", "The commit composer in Files & Changes", inFiles(() => showFilesView("changes"))],
    ["Git: Push", "", inFiles(() => ACTIONS.push())],
    ["Git: Pull (fast-forward)", "", inFiles(() => ACTIONS.pull())],
    ["Git: Fetch", "", inFiles(() => ACTIONS.fetch())],
    ["Git: Log", "Recent commits with revert and reset", inFiles(() => showFilesView("log"))],
    ["Git: Files", "Browse the project tree", inFiles(() => showFilesView("files"))],
    ["Git: Branches", "", inFiles(() => showFilesView("branches"))],
  ].map(([title, subtitle, run]) => ({ kind: "Action", title, subtitle, run }));
  return [...entries, ...actions, ...more];
}

function showFilesView(view) {
  if (!state.files) return;
  state.files.view = view;
  render();
}

/// The macOS app's ranking: prefix, then a word's start, then anywhere in the
/// title, then the subtitle, then the letters in order.
function paletteScore(query, title, subtitle = "") {
  const q = query.toLowerCase();
  const t = title.toLowerCase();
  if (t.startsWith(q)) return 100;
  if (t.split(" ").some((word) => word.startsWith(q))) return 80;
  if (t.includes(q)) return 60;
  if (subtitle.toLowerCase().includes(q)) return 30;
  let at = 0;
  for (const letter of q) {
    at = t.indexOf(letter, at) + 1;
    if (!at) return 0;
  }
  return 15;
}

function terminalEntries() {
  const open = state.tabs.filter((id) => anySession(id));
  const current = state.sessionId;
  const ordered = [
    ...recent.filter((id) => open.includes(id) && id !== current),
    ...open.filter((id) => !recent.includes(id) && id !== current),
    ...(current && open.includes(current) ? [current] : []),
  ];
  return ordered.map((id) => {
    const item = anySession(id);
    const owner = state.projects.find((entry) => entry.id === item.project_id);
    const index = state.tabs.indexOf(id);
    return {
      kind: "Session",
      title: item.title,
      subtitle: [owner?.title, isRunning(id) ? (state.agentState.get(id) ?? "running") : "stopped", id === current ? "current" : ""]
        .filter(Boolean)
        .join(" · "),
      shortcut: index < 9 ? label(`mod+${index + 1}`) : "",
      run: () => openAnywhere(id),
    };
  });
}

function paletteResults(query, mode = "all") {
  const needle = query.trim();
  if (mode === "terminals") {
    const items = terminalEntries();
    return needle ? items.filter((item) => paletteScore(needle, item.title, item.subtitle) > 0) : items;
  }
  const items = paletteEntries();
  if (!needle) {
    // As on macOS: who needs you, what runs, a few recent, a few actions.
    const sessions = items.filter((item) => item.kind === "Session");
    return [
      ...sessions.filter((item) => item.needsYou),
      ...sessions.filter((item) => item.running && !item.needsYou),
      ...sessions.filter((item) => !item.running).slice(0, 6),
      ...items.filter((item) => item.kind === "Action").slice(0, 6),
    ];
  }
  return items
    .map((item) => {
      const score = paletteScore(needle, item.title, item.subtitle);
      return [item, score && score + (item.needsYou ? 10 : 0) + (item.running ? 5 : 0) + (item.kind === "Session" ? 2 : 0)];
    })
    .filter(([, score]) => score > 0)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 40)
    .map(([item]) => item);
}

/// "all" is the command palette; "terminals" is the ⌘E switcher: open tabs,
/// most recently used first and the one on screen last, as on macOS.
function openPalette(mode = "all") {
  if (state.dialog?.kind === "palette" && state.dialog.mode === mode && mode === "terminals") {
    // ⌘E again moves down the list, like ⌘Tab.
    const count = state.dialog.results.length || 1;
    state.dialog.index = (state.dialog.index + 1) % count;
    return render();
  }
  openModal({ kind: "palette", mode, query: "", results: paletteResults("", mode), index: 0 });
  document.querySelector("#palette-input")?.focus();
}

function runPalette(index) {
  const entry = state.dialog?.results?.[index];
  closeDialog();
  entry?.run();
}

// -------------------------------------------------------------- monitor ---

/// The two-second check of what each running agent reports through its hooks.
/// Terminal text is never used to guess whether an agent has finished.
async function monitorTick() {
  if (!state.running.length) return;
  const report = await call("monitor_tick");
  if (!report) return;
  let touched = false;
  for (const entry of report.states) {
    if (state.agentState.get(entry.id) !== entry.state) {
      state.agentState.set(entry.id, entry.state);
      touched = true;
      if (entry.notable) notify(entry);
    }
  }
  for (const id of report.hibernate) {
    await terminal.hibernate(id);
  }
  if (report.changed) await loadSessions();
  else if (touched) render();
  updateBadge();
}

async function notify(entry) {
  const prefs = state.settings;
  if (!prefs.notifications) return;
  if (entry.state === "done" ? prefs.notify_done === false : prefs.notify_waiting === false) return;
  // The session you are looking at never interrupts you; the rest only
  // while Convoy is in the background, unless asked otherwise.
  if (document.hasFocus() && (!prefs.notify_when_focused || state.sessionId === entry.id)) return;
  const { isPermissionGranted, requestPermission, sendNotification } = await import(
    "@tauri-apps/plugin-notification"
  );
  let granted = await isPermissionGranted();
  if (!granted) granted = (await requestPermission()) === "granted";
  if (!granted) return;
  sendNotification({
    title: entry.state === "done" ? "Agent finished a turn" : "Agent needs your attention",
    body: entry.title,
    ...(prefs.notification_sound === "none" ? {} : { sound: prefs.notification_sound || "default" }),
  });
  notified = { id: entry.id, state: entry.state };
}

/// The session the last notification was about. Desktop notifications give
/// no click callback, but clicking one brings the window forward: coming back
/// while that session still waits opens it, as clicking does on macOS.
let notified = null;
addEventListener("focus", () => {
  const last = notified;
  notified = null;
  if (last && isRunning(last.id) && state.agentState.get(last.id) === last.state && state.sessionId !== last.id) {
    openAnywhere(last.id);
  }
});

/// The Dock badge counts the sessions waiting for you. Windows has no badge;
/// there the call fails and nothing is shown.
let badge = 0;
async function updateBadge() {
  const waiting = state.running.filter((id) => state.agentState.get(id) === "waiting").length;
  if (waiting === badge) return;
  badge = waiting;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setBadgeCount(waiting || undefined);
  } catch {
    /* unsupported here */
  }
}

// ------------------------------------------------------------ git polling --

let gitTimer = null;

/// Refreshes Git at the interval Settings asks for: the open session's branch
/// line, and every project's branch and count when sidebar rows show them.
function scheduleGitPolling() {
  clearInterval(gitTimer);
  gitTimer = null;
  const seconds = Number(state.settings.git_poll_seconds ?? 10);
  if (state.settings.git_status === false) {
    state.projectGit = new Map();
    return;
  }
  pollProjects();
  if (seconds > 0) {
    gitTimer = setInterval(() => {
      refreshGitStatus();
      pollProjects();
    }, seconds * 1000);
  }
}

async function pollProjects() {
  if (state.settings.compact_sidebar !== false || state.settings.git_status === false) return;
  const next = new Map();
  await Promise.all(
    state.projects.slice(0, 40).map(async (item) => {
      const reading = await attempt("project_git", { id: item.id });
      if (reading.ok && reading.value) next.set(item.id, reading.value);
    }),
  );
  state.projectGit = next;
  render();
}

// ------------------------------------------------------------- AI limits --

/// Reads the limits. Codex starts its CLI to answer, so it is asked only when
/// wanted; Claude's reading is a file its status line left behind.
async function refreshLimits(codex = false) {
  if (codex) {
    state.limits = { ...(state.limits ?? {}), loading: true };
    render();
  }
  const reading = await attempt("limits_read", { codex });
  const previous = state.limits ?? {};
  state.limits = reading.ok
    ? {
        claude: reading.value.claude,
        codex: reading.value.codex ?? previous.codex ?? null,
        codexAt: reading.value.codex ? Date.now() : previous.codexAt,
        loading: false,
      }
    : { ...previous, loading: false, error: reading.error };
  render();
}

function openLimits() {
  state.dialog = null;
  state.menu = { kind: "limits" };
  render();
  // A Codex reading older than five minutes is refreshed on opening.
  if (!state.limits?.codexAt || Date.now() - state.limits.codexAt > 5 * 60 * 1000) refreshLimits(true);
}

async function applyKeepAwake() {
  await call("keep_awake", {
    wanted:
      state.settings.keep_awake === "always" ||
      (state.settings.keep_awake === "sessions" && state.running.length > 0),
  });
}

terminal.onExit(async (exit) => {
  updateBadge();
  if (exit.id.startsWith("login:")) {
    if (state.dialog?.id === exit.id) {
      state.dialog.finished = true;
      render();
    }
    return;
  }
  await applyKeepAwake();
  const projectId =
    state.sessions.find((item) => item.id === exit.id)?.project_id ?? state.projectId;
  // "Auto-run task queue": the queue carries on by itself in this project.
  if (state.projects.find((item) => item.id === projectId)?.auto_run_tasks) state.queues.add(projectId);
  const queued = state.queues.has(projectId);

  // A failure or a stop pauses the queue, and nothing resumes it but the user
  // asking again — including a restart. Hibernation is neither: the agent was
  // idle, and the queue carries on when its own task next finishes.
  if (!exit.clean) {
    if (queued && exit.cause !== "hibernated") {
      state.queues.delete(projectId);
      toast("The queue is paused: the last task did not finish cleanly.", "bad");
      render();
    }
    return;
  }

  // What a clean exit means is decided in the core, and it is asked on every
  // exit: a single task with automatic review set is handed on whether or not
  // a queue is running.
  const step = await call("queue_after_exit", { id: exit.id, clean: true });
  if (!step) return;
  if (step.kind === "superseded") {
    state.queues.delete(projectId);
    toast("The specification changed while the task ran. It is back in changes.", "bad");
    await loadPlanning();
    return render();
  }
  if (step.kind === "review") {
    await loadSessions();
    await terminal.start(step.session);
  }
  if (queued && step.kind !== "nothing") await advanceQueue(projectId);
});

// ------------------------------------------------------------- file drop --

/// A path as a shell reads it back: bare when it is plainly safe, quoted
/// otherwise — single quotes on macOS and Linux, double quotes on Windows.
function shellQuote(path) {
  if (/^[\w@%+=:,./-]+$/.test(path)) return path;
  if (/Win/.test(navigator.platform)) return `"${path.replaceAll('"', '""')}"`;
  return `'${path.replaceAll("'", "'\\''")}'`;
}

/// The session whose terminal is under a point of the window, if any.
function terminalAt(x, y) {
  const spot = document.elementFromPoint(x, y);
  const pane = spot?.closest("[data-pane]");
  if (pane) return state.panes[Number(pane.dataset.pane)] ?? null;
  return spot?.closest("#terminal-host") ? state.sessionId : null;
}

/// Files dropped onto a terminal are typed into it as quoted paths, as the
/// macOS app does, without pressing Enter.
function dropPaths(paths, x, y) {
  const id = terminalAt(x, y);
  if (!id || !paths?.length) return false;
  if (!isRunning(id)) {
    toast("Start the session to drop files into it.");
    return false;
  }
  terminal.insert(id, `${paths.map(shellQuote).join(" ")} `);
  return true;
}
window.convoyDrop = dropPaths;

async function wireFileDrop() {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type !== "drop") return;
      const scale = window.devicePixelRatio || 1;
      dropPaths(payload.paths, payload.position.x / scale, payload.position.y / scale);
    });
  } catch {
    /* no native window, as under the UI tests */
  }
}

// ------------------------------------------------------------- bootstrap --

// The window asks before it takes the agents with it.
await listen("window:closing", ({ payload }) =>
  openModal({ kind: "quit", running: Number(payload) || 1 }),
);

try {
  await terminal.wire();
  wireFileDrop();
  await loadSettings();
  applyTheme();
  await loadWorkspace({ required: true });
  // The window opens on Home, as the macOS app's does.
  if (state.projects.length) openHome();
  await applyKeepAwake();
  setInterval(monitorTick, 2000);
  setInterval(applyKeepAwake, 10000);
  refreshLimits(false);
  setInterval(() => refreshLimits(false), 60000);
  scheduleGitPolling();
  loadActivity();
  setInterval(loadActivity, 5000);
  // What each running terminal shows is kept every few seconds.
  setInterval(terminal.snapshotRunning, 4000);
  // The backend found a Codex id or a pull request in an agent's output.
  await listen("session:changed", async () => {
    await loadSessions();
    if (state.tab === "tasks") await loadPlanning();
  });
  let offered = false;
  try {
    offered = localStorage.getItem(MAC_OFFERED) === "1";
  } catch {}
  if (!offered && !state.projects.length && (await checkMacImport())?.projects && !state.dialog) {
    await offerMacImport(true);
  }
} catch (error) {
  fatal(error?.stack ?? String(error));
}

// Task cards move between board columns by dragging.
let draggedTask = null;
app.addEventListener("dragstart", (event) => {
  const card = event.target.closest?.("[data-task-card]");
  if (!card) return;
  draggedTask = card.dataset.taskCard;
  event.dataTransfer.effectAllowed = "move";
  event.dataTransfer.setData("text/plain", draggedTask);
});
app.addEventListener("dragover", (event) => {
  const column = event.target.closest?.("[data-task-column]");
  if (!column || !draggedTask) return;
  event.preventDefault();
  for (const other of app.querySelectorAll(".board__column--drop")) other.classList.remove("board__column--drop");
  column.classList.add("board__column--drop");
});
app.addEventListener("drop", (event) => {
  const column = event.target.closest?.("[data-task-column]");
  if (!column || !draggedTask) return;
  event.preventDefault();
  const id = draggedTask;
  draggedTask = null;
  moveTask(id, column.dataset.taskColumn);
});
app.addEventListener("dragend", () => {
  draggedTask = null;
  for (const other of app.querySelectorAll(".board__column--drop")) other.classList.remove("board__column--drop");
});

// Tabs are reordered by dragging one onto another.
let draggedTab = null;
app.addEventListener("dragstart", (event) => {
  const tab = event.target.closest?.("[data-tab-open]");
  if (!tab) return;
  draggedTab = tab.dataset.tabOpen;
  event.dataTransfer.effectAllowed = "move";
  event.dataTransfer.setData("text/plain", draggedTab);
});
app.addEventListener("dragover", (event) => {
  const tab = event.target.closest?.("[data-tab-open]");
  if (!tab || !draggedTab) return;
  event.preventDefault();
  for (const other of app.querySelectorAll(".tab-item--drop")) other.classList.remove("tab-item--drop");
  if (tab.dataset.tabOpen !== draggedTab) tab.classList.add("tab-item--drop");
});
app.addEventListener("drop", (event) => {
  const tab = event.target.closest?.("[data-tab-open]");
  if (!tab || !draggedTab) return;
  event.preventDefault();
  const id = draggedTab;
  draggedTab = null;
  if (tab.dataset.tabOpen !== id) moveTab(id, state.tabs.indexOf(tab.dataset.tabOpen));
});
app.addEventListener("dragend", () => {
  draggedTab = null;
  for (const other of app.querySelectorAll(".tab-item--drop")) other.classList.remove("tab-item--drop");
});

// Right-click on a project or a session in the sidebar opens its menu.
app.addEventListener("contextmenu", (event) => {
  const tab = event.target.closest("[data-tab-open]");
  if (tab) {
    event.preventDefault();
    state.menu = { kind: "tab", id: tab.dataset.tabOpen, x: event.clientX, y: event.clientY };
    return render();
  }
  const task = event.target.closest("[data-task-menu]");
  if (task) {
    event.preventDefault();
    return openContextMenu("task", task.dataset.taskMenu, event.clientX, event.clientY);
  }
  const session = event.target.closest("[data-menu-session]");
  const owner = event.target.closest("[data-menu-project]");
  if (!session && !owner) return;
  event.preventDefault();
  if (session) return openContextMenu("session", session.dataset.menuSession, event.clientX, event.clientY);
  return openContextMenu("project", owner.dataset.menuProject, event.clientX, event.clientY);
});

// Clicking inside a pane focuses it — its session becomes the one the bars
// and tabs describe. Buttons in the pane do their own thing.
app.addEventListener("mousedown", (event) => {
  const pane = event.target.closest?.("[data-pane]");
  if (!pane || event.target.closest("button")) return;
  focusPane(Number(pane.dataset.pane));
});
