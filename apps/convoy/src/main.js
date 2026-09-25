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
  observe,
  project,
  session,
  state,
  toast,
  visibleSessions,
} from "./state.js";
import { escape, value } from "./ui.js";
import { SHORTCUTS, binding, capture, matches } from "./shortcuts.js";
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
import { specsView, tasksView } from "./views-planning.js";
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
          <main class="main">${tabBar()}${state.files || state.page === "project" || state.sessionId ? "" : header()}${content()}</main>
          ${status()}
        </div>`;

  if (state.menu === "session") app.insertAdjacentHTML("beforeend", sessionMenu());
  if (state.menu === "quick") app.insertAdjacentHTML("beforeend", quickMenu());
  if (state.menu === "remote") app.insertAdjacentHTML("beforeend", remoteMenu());
  if (state.menu?.kind) app.insertAdjacentHTML("beforeend", contextMenu(state.menu));
  if (state.dialog) app.insertAdjacentHTML("beforeend", isImportDialog() ? imports.dialog() : dialogView());

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

  if (state.sessionId && !state.files && state.page !== "settings") {
    terminal.mount(state.sessionId);
    if (state.split) terminal.mount(state.split, "#terminal-host-split");
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

function openSession(id) {
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
  for (const key of ["title", "details", "findings"]) {
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
  "layout-one": () => {
    state.split = null;
    render();
  },
  "layout-two": () => (state.sessionId && !state.split ? ACTIONS["open-split"]() : undefined),
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
  home: () => {
    state.sessionId = null;
    state.files = null;
    state.page = null;
    render();
  },
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
  "new-session": () =>
    openModal({
      kind: "session",
      title: project()?.title ?? "Session",
      agent: project()?.default_agent || state.settings.default_agent,
      model: "",
      prompt: "",
    }),
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
  "send-feedback": () => openModal({ kind: "feedback" }),
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
  "refresh-activity": loadActivity,

  // menus
  quit: () => call("quit_now"),
  "open-split": () => openModal({ kind: "split" }),
  "close-split": () => {
    state.split = null;
    changed();
  },
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
      mode: project()?.task_mode || "none",
      auto_review: false,
      spec_id: "",
    }),
  "save-task": saveTask,
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
  search: () => document.querySelector("#project-search")?.focus(),

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
  split: () => withSession(() => state.sessions.length > 1 && ACTIONS["open-split"]()),

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
      "[data-tab-close],[data-tab-open],[data-tab-act],[data-awake],[data-issue],[data-conn-add],[data-conn-edit],[data-conn-remove],[data-conn-auth],[data-issue-source]",
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
  if (data.tabClose) return closeTab(data.tabClose);
  if (data.tabOpen) return openTab(data.tabOpen);
  if (data.tabAct) {
    const id = state.menu?.id;
    state.menu = null;
    const at = state.tabs.indexOf(id);
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
    if (state.projectId !== data.expand) {
      state.collapsed.delete(data.expand);
      return selectProject(data.expand);
    }
    if (state.collapsed.has(data.expand)) state.collapsed.delete(data.expand);
    else state.collapsed.add(data.expand);
    return render();
  }
  if (state.page === "settings" && !state.dialog) {
    if (data.settingsSection) {
      state.settingsSection = data.settingsSection;
      state.capturing = null;
      render();
      if (data.settingsSection === "Setup" && !state.diagnostics) await runDiagnostics();
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
  if (data.project) return selectProject(data.project);
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
  if (data.start) return terminal.start(data.start);
  if (data.stop) return terminal.stop(data.stop);
  if (data.open) return openSession(data.open);
  if (data.split) {
    state.split = data.split;
    return closeDialog();
  }
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
  if (field.id === "palette-input") {
    state.dialog.query = field.value;
    state.dialog.results = paletteResults(field.value);
    state.dialog.index = 0;
    return render();
  }
});

app.addEventListener("change", async (event) => {
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

async function createSession() {
  captureDraft();
  const draft = state.dialog;
  const id = await call("session_create", {
    projectId: state.projectId,
    agent: draft.agent,
    title: draft.title,
    prompt: draft.prompt,
    model: draft.model,
  });
  if (!id) return;
  closeDialog();
  await loadWorkspace();
  // Selected but not started: launching an agent is always an explicit act,
  // and a first message is acted on the moment it runs.
  openSession(id);
  if (state.settings.worktree_by_default) await worktreeByDefault(id, draft.title);
}

/// "Run new sessions in a git worktree by default". A folder that is not a
/// repository simply keeps the session in the project folder.
async function worktreeByDefault(id, title) {
  const made = await attempt("worktree_create", { id, branch: suggestBranch(title || "session") });
  if (!made.ok) return toast(`Started in the project folder: ${made.error}`);
  await loadSessions();
  const plan = made.value;
  if (plan.shared_paths.length || plan.setup_command) {
    openModal({
      kind: "worktree-setup",
      id,
      shared: plan.shared_paths,
      command: plan.setup_command ?? "",
      directory: plan.directory,
    });
  } else {
    toast(`Worktree created on ${plan.branch}`);
  }
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
const suggestBranch = (title) =>
  `${project()?.branch_prefix || state.settings.branch_prefix || "convoy"}/${title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "work"}`;

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
  if (!(await done("profile_add", { label, agent: state.profileAgent }))) return;
  await loadProfiles();
  render();
  toast(`Added ${label}`);
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
  await loadProfiles();
  openModal({ kind: "accounts", label: "", agent: state.settings.default_agent });
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

async function saveTask() {
  captureDraft();
  const dialog = state.dialog;
  const saved = await done("task_save", {
    input: {
      id: dialog.id ?? null,
      project_id: state.projectId,
      spec_id: dialog.spec_id || null,
      title: dialog.title,
      details: dialog.details,
      findings: dialog.findings,
      agent: dialog.agent,
      mode: dialog.mode,
      auto_review: dialog.auto_review,
    },
  });
  if (!saved) return;
  closeDialog();
  await loadPlanning();
}

async function setTaskStatus(status) {
  const id = state.dialog.id;
  if (!(await done("task_status", { id, status }))) return;
  closeDialog();
  await loadPlanning();
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

// -------------------------------------------------------------- palette ---

function paletteEntries() {
  const entries = [
    ["New session", "Action", () => ACTIONS["new-session"]()],
    ["Open folder", "Action", openFolder],
    ["Files & Changes", "Action", openFiles],
    ["Project settings", "Action", openProjectSettings],
    ["Import provider history", "Action", openTranscripts],
    ["Accounts", "Action", openAccounts],
    ["Settings", "Action", openSettings],
    ["Activity", "Action", () => openActivity()],
    ["Docs & specs", "Action", () => withProject(() => showTab("docs"))],
  ];
  for (const item of state.projects) {
    entries.push([item.title, "Project", () => selectProject(item.id)]);
  }
  for (const item of visibleSessions()) {
    entries.push([item.title, "Session", () => openSession(item.id)]);
  }
  return entries.map(([title, kind, run]) => ({ title, kind, run }));
}

const paletteResults = (query) => {
  const needle = query.toLowerCase();
  return paletteEntries().filter(
    (entry) =>
      !needle ||
      entry.title.toLowerCase().includes(needle) ||
      entry.kind.toLowerCase().includes(needle),
  );
};

function openPalette() {
  openModal({ kind: "palette", query: "", results: paletteResults(""), index: 0 });
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

// ------------------------------------------------------------- bootstrap --

// The window asks before it takes the agents with it.
await listen("window:closing", ({ payload }) =>
  openModal({ kind: "quit", running: Number(payload) || 1 }),
);

try {
  await terminal.wire();
  await loadSettings();
  applyTheme();
  await loadWorkspace({ required: true });
  await applyKeepAwake();
  setInterval(monitorTick, 2000);
  setInterval(applyKeepAwake, 10000);
  refreshLimits(false);
  setInterval(() => refreshLimits(false), 60000);
  scheduleGitPolling();
  loadActivity();
  setInterval(loadActivity, 5000);
} catch (error) {
  fatal(error?.stack ?? String(error));
}

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
  const session = event.target.closest("[data-menu-session]");
  const owner = event.target.closest("[data-menu-project]");
  if (!session && !owner) return;
  event.preventDefault();
  if (session) return openContextMenu("session", session.dataset.menuSession, event.clientX, event.clientY);
  return openContextMenu("project", owner.dataset.menuProject, event.clientX, event.clientY);
});
