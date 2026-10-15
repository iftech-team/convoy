// Bootstrap, rendering and every action the window offers.

import { installTasks } from "./tasks.js";
import { agentName, newTaskDefaults } from "./task-defaults.js";
import { dropStatus, movable } from "./phase.js";
import { presetPatch } from "./presets.js";
import { sessionLabel } from "./session-state.js";
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
  noteSleep,
  phaseOf,
  sessionState,
  loadPlanning,
  loadSessions,
  loadSettings,
  loadWorkspace,
  anySession,
  saveCollapsed,
  activeAccount,
  rememberAccount,
  lastChoice,
  rememberChoice,
  observe,
  project,
  session,
  state,
  toast,
  visibleSessions,
  ensureModels,
  PROJECT_TAB,
  projectTab,
  isProjectTab,
} from "./state.js";
import { escape, plural, value } from "./ui.js";
import { setLanguage, t } from "./i18n.js";
import { SHORTCUTS, binding, capture, label, matches } from "./shortcuts.js";
import { dialogView, effortLevels, sweepChoices, sweepGroups, sweepProject, sweepVisible } from "./dialogs.js";
import * as terminal from "./terminal.js";
import { adoptApprovals, answer, chatEvent, chatExited, chatSnapshot, chatText, historyChanged, insertDraft, mountChat, sendText, shownAsTranscript, viewOf, waitingFor } from "./views-chat.js";
import { inboxItems } from "./views-inbox.js";
import {
  header,
  quickMenu,
  reviewsView,
  taskOfSession,
  sessionMenu,
  pickableKeys,
  sessionsView,
  sidebar,
  contextMenu,
  tabBar,
  tabInfo,
  status,
  workbench,
} from "./views.js";
import { filesView, remoteMenu } from "./views-files.js";
import { pageTask, taskPage } from "./views-task.js";
import { settingsPage } from "./views-settings.js";
import { projectSettingsPage } from "./views-project.js";
import { forgetIcon, onIconLoaded } from "./icons.js";
import { runnable, specsView, tasksView } from "./views-planning.js";
import { dashboardView, homeView } from "./views-home.js";
import { toggleHistoryCollapsed } from "./views-history.js";
import { PROJECT_DOC, docsView } from "./views-docs.js";
import { installAutomations } from "./automations.js";
import { automationsView } from "./views-automations.js";
import { fitSkills, installSkills } from "./skills.js";
import { installUsage } from "./usage.js";
import { usageView } from "./views-usage.js";

const app = document.querySelector("#app");
const imports = installTasks({ state, escape, icons, call, callDone: done, toast, render,
  loadWorkspace: async () => { await loadWorkspace(); await loadPlanning(); },
  start: (id) => startSession(id), launch: (id) => startSession(id, { background: true }) });
// Wrapped: the dialog helpers are defined further down.
const automations = installAutomations({
  render: () => render(),
  openModal: (dialog) => openModal(dialog),
  closeDialog: () => closeDialog(),
  selectProject: (id) => selectProject(id),
  loadWorkspace: () => loadWorkspace(),
});
const skills = installSkills({
  render: () => render(),
  openModal: (dialog) => openModal(dialog),
  closeDialog: () => closeDialog(),
  openSettings: (section) => openSettings(section),
  loadWorkspace: () => loadWorkspace(),
});
const usage = installUsage({ render: () => render() });
const isImportDialog = () => ["import", "integrations", "connection", "task-model"].includes(state.dialog?.kind);

// A thrown error used to leave a black window and no clue about it. Painting
// the message is the difference between "Convoy is broken" and a line the user
// can act on — a corrupt workspace file says so.
function fatal(detail) {
  const where = state.storage
    ? `<p>${escape(t("The workspace file is at {path}.", { path: "\u0000" })).replace("\u0000", `<code>${escape(state.storage)}</code>`)}</p>`
    : "";
  app.innerHTML = `
    <div class="fatal">
      <h1>${escape(t("Convoy could not start"))}</h1>
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
  if (state.page === "usage") return usageView();
  if (state.files) return filesView();
  if (!project()) {
    return `<div class="empty">
      <span class="empty__mark">📁</span>
      <h2 class="empty__title">${escape(t("No project selected"))}</h2>
      <p class="empty__text">${escape(t("Open a folder to begin."))}</p>
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
      return pageTask() ? taskPage() : tasksView();
    case "docs":
      return docsView();
    case "automations":
      return automationsView();
    default:
      return sessionsView();
  }
}

let shown = null;

// ------------------------------------------------------------------- tabs --

const TABS_KEY = "convoy.tabs";

/// Every project's tasks are being fetched for a task form's related picker.
let allTasksLoading = false;

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

/// The tab of what the main area shows: the open session, or the page of
/// the selected project. `null` on Home, Files, Settings and the like.
function currentTab() {
  if (state.files || (state.page && state.page !== "project")) return null;
  if (state.sessionId) return state.sessionId;
  return state.projectId ? projectTab(state.projectId) : null;
}

/// Gives the selected project's page a tab. Only going to the page does
/// this: opening one of its sessions passes through it without a tab.
function keepProjectTab() {
  if (!state.projectId) return;
  const id = projectTab(state.projectId);
  if (!state.tabs.includes(id)) state.tabs.push(id);
}

/// The open session always has a tab, as does a project page once gone to,
/// and a tab remembers what it shows so it can be drawn while another
/// project is open. Archived sessions and removed projects leave.
function syncTabs() {
  let touched = false;
  const current = currentTab();
  if (current && !isProjectTab(current) && !state.tabs.includes(current)) {
    state.tabs.push(current);
    touched = true;
  }
  if (current && isProjectTab(current) && !state.page && state.tabs.includes(current)) {
    const info = { kind: "project", project_id: state.projectId, view: state.tab };
    if (JSON.stringify(state.tabInfo[current]) !== JSON.stringify(info)) {
      state.tabInfo[current] = info;
      touched = true;
    }
  }
  if (state.projects.length) {
    const gone = state.tabs.filter((id) => isProjectTab(id) && !state.projects.some((item) => projectTab(item.id) === id));
    if (gone.length) {
      state.tabs = state.tabs.filter((id) => !gone.includes(id));
      for (const id of gone) delete state.tabInfo[id];
      touched = true;
    }
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
  if (isProjectTab(id)) {
    if (state.page === "project") closeProjectSettings();
    state.page = null;
    state.capturing = null;
    await selectProject(info.project_id);
    return showTab(info.view ?? state.tab);
  }
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
      title: t("Stop this agent and close the tab?"),
      body: t("Recent output stays saved and the session can be resumed later."),
      confirmLabel: t("Stop and close"),
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
  const shown = currentTab() === id;
  state.tabs = state.tabs.filter((tab) => tab !== id);
  delete state.tabInfo[id];
  saveTabs();
  if (!isProjectTab(id) && !state.running.includes(id)) terminal.forget(id);
  if (shown) {
    const next = state.tabs[Math.min(at, state.tabs.length - 1)];
    if (next) return openTab(next);
    // A project page left on screen would take its tab straight back.
    if (isProjectTab(id)) return openHome();
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
  const at = state.tabs.indexOf(currentTab());
  return openTab(state.tabs[(Math.max(at, 0) + step + state.tabs.length) % state.tabs.length]);
}

function render() {
  syncTabs();
  // The sessions list ends with the conversations no session holds yet.
  if (state.tab === "sessions") ensureHistory();
  // A sheet choosing a model asks for the agent's list the first time.
  if (state.dialog?.kind === "task") ensureModels(state.dialog.agent, state.dialog.profile_id ?? "");
  // Related tasks are picked from every project.
  if (state.dialog?.kind === "task" && !state.allTasks && !allTasksLoading) {
    allTasksLoading = true;
    loadAllTasks().then(() => {
      allTasksLoading = false;
      render();
    });
  }
  if (["session", "review"].includes(state.dialog?.kind) && state.dialog.agent) ensureModels(state.dialog.agent, state.dialog.account ?? "");
  if (state.dialog && isImportDialog()) {
    ensureModels("claude");
    ensureModels("codex");
    ensureModels("gemini");
  }
  const caret = document.activeElement?.id;
  const position = document.activeElement?.selectionStart;
  // Every change re-renders, and a scroller that jumps to the top each time
  // makes a long dialog or a long file list unusable.
  // Both ways: the tab strip scrolls sideways.
  const scrolled = new Map(
    [...document.querySelectorAll("[data-scroll]")].map((node) => [
      node.dataset.scroll,
      [node.scrollTop, node.scrollLeft],
    ]),
  );
  const selectedTab = document.querySelector(".tab-item--selected")?.dataset.tabOpen;

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

  for (const [key, [top, left]] of scrolled) {
    const node = document.querySelector(`[data-scroll="${key}"]`);
    if (!node) continue;
    node.scrollTop = top;
    node.scrollLeft = left;
  }
  // A tab selected from elsewhere (the sidebar, ⌘1–9) is brought into view;
  // one clicked in the strip is already there, so nothing moves.
  const tab = document.querySelector(".tab-item--selected");
  if (tab && tab.dataset.tabOpen !== selectedTab) tab.scrollIntoView({ block: "nearest", inline: "nearest" });

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
    // A pane shows its session as a chat or as a terminal.
    const place = (id, suffix) =>
      viewOf(anySession(id)) === "chat" || shownAsTranscript(anySession(id))
        ? mountChat(id, `#chat-host${suffix}`)
        : terminal.mount(id, `#terminal-host${suffix}`);
    if (state.layout > 1) {
      state.panes.forEach((id, index) => id && index < state.layout && place(id, `-${index}`));
    } else {
      place(state.sessionId, "");
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
  } else if (state.menu?.kind === "approvals") {
    // A redraw — a row answered elsewhere, a new one arriving — keeps the
    // keyboard in the inbox.
    const active = document.activeElement;
    if (!active || active === document.body) document.querySelector(".inbox")?.focus();
  }
  usage.afterRender();
}

observe(render);
onIconLoaded(() => render());

// -------------------------------------------------------------- selection --

async function selectProject(id) {
  if (state.page === "project") closeProjectSettings();
  if (state.projectId === id) return;
  state.projectId = id;
  state.sessionId = null;
  state.picked = null;
  state.files = null;
  state.planning = null;
  await loadSessions();
  if (state.tab === "specs" || state.tab === "tasks") await loadPlanning();
}

/// Shows a project's own page, from the sidebar or the palette: off Home,
/// the Dashboard, Usage or Settings, and, as on macOS, out of its session or
/// files when it is already the selected one.
async function showProject(id) {
  if (state.page && state.page !== "project") {
    state.page = null;
    state.capturing = null;
  }
  if (state.projectId === id) {
    state.sessionId = null;
    state.files = null;
  }
  await selectProject(id);
  if (!state.sessionId && !state.files) keepProjectTab();
  render();
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
    if (isProjectTab(id) && state.projects.some((item) => projectTab(item.id) === id)) {
      state.tabInfo[id] = { kind: "project", project_id: id.slice(PROJECT_TAB.length), view: "sessions" };
      return openTab(id);
    }
    if (anySession(id)) return openAnywhere(id);
  }
}

/// A stopped session shows its saved output in place of a live terminal.
async function showSaved(id) {
  if (isRunning(id)) return;
  const text = await call("session_output", { id });
  if (text === undefined || isRunning(id)) return;
  terminal.restore(id, text);
  // Read only now: it may turn out too thin to show, or a missing conversation.
  if (terminal.missingConversation(id) || terminal.thinOutput(id)) render();
}

/// Home or the Agent Dashboard, with what they show brought up to date.
async function openHome(page = "home") {
  if (state.page === "project") closeProjectSettings();
  state.page = page;
  state.sessionId = null;
  state.files = null;
  state.menu = null;
  render();
  if (page === "usage") usage.load();
  await Promise.all([
    loadWorkspace(),
    loadAllTasks(),
    state.diagnostics && state.setupChecks ? null : loadProfiles().then(loadSetup),
  ]);
  render();
}

function openSession(id) {
  if (["home", "dashboard", "usage"].includes(state.page)) state.page = null;
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
/// A dialog's text fields and the draft key each one fills.
const DRAFT_FIELDS = {
  "draft-title": "title",
  "rename-title": "title",
  "draft-model": "model",
  "draft-effort": "effort",
  "draft-prompt": "prompt",
  "draft-notes": "notes",
  "draft-provider": "provider_id",
  "draft-brief": "brief",
  "draft-branch": "branch",
  "draft-feedback": "feedback",
  "draft-base": "base",
  "draft-account": "account",
  "draft-label": "label",
  "draft-text": "text",
  "draft-group": "group",
  "draft-icon": "icon",
  "draft-shared": "shared_paths",
  "draft-setup": "setup_command",
  "draft-review": "review_template",
  "task-spec": "spec_id",
  "plan-comment": "plan_comment",
  "task-account": "profile_id",
  ...Object.fromEntries(["title", "problem", "requirements", "acceptance", "constraints", "plan"].map((key) => [`spec-${key}`, key])),
  ...Object.fromEntries(["title", "details", "findings", "model", "effort"].map((key) => [`task-${key}`, key])),
};

function captureDraft() {
  const dialog = state.dialog;
  if (!dialog) return;
  for (const [id, key] of Object.entries(DRAFT_FIELDS)) {
    const element = document.querySelector(`#${id}`);
    if (element) dialog[key] = element.value;
  }
}

// -------------------------------------------------------------- actions ---

const ACTIONS = {
  // projects
  "open-folder": openFolder,
  settings: () => (state.page === "settings" ? closeSettings() : openSettings()),
  accounts: openAccounts,
  "import-history": openTranscripts,
  "import-all-history": openHistorySweep,
  "history-sweep-run": runHistorySweep,
  "project-settings": () => openProjectSettings(),
  "project-tracker": () => openProjectSettings("tracker"),
  "sync-suggest": () => {
    const link = state.projectDraft?.link;
    if (link?.sync && state.syncChoices?.suggested) return saveProjectLink({ ...link, sync: { ...link.sync, map: state.syncChoices.suggested } });
  },
  "task-back": () => {
    state.taskId = null;
    render();
  },
  "task-page-edit": () => editTask(findTask(state.taskId)),
  "task-qa-open": () => {
    const session = findTask(panelTaskId())?.qa?.session_id;
    if (!session) return;
    closeDialog();
    return openAnywhere(session);
  },
  "task-qa-run": () => {
    const id = panelTaskId();
    if (id) return startQa(id);
  },
  "plan-approve": () => approvePlan(panelTaskId()),
  "plan-changes": async () => {
    captureDraft();
    const id = panelTaskId();
    const box = document.querySelector("#plan-comment");
    const comment = (box?.value ?? state.dialog?.plan_comment ?? "").trim();
    if (!id) return;
    if (!comment) return toast(t("Say what should change in the plan."));
    const actions = await call("plan_request_changes", { taskId: id, comment });
    if (!actions) return;
    if (state.dialog) state.dialog.plan_comment = "";
    if (box) box.value = "";
    toast(t("Sent to the planner. The new plan shows here when it is written."));
    await applyPipeline(actions);
    refreshTaskDialog(id);
    render();
  },
  "plan-open": () => {
    const session = findTask(panelTaskId())?.plan?.session_id;
    if (!session) return;
    closeDialog();
    return openAnywhere(session);
  },
  "review-open": () => {
    const task = findTask(panelTaskId());
    const session = task?.review?.session_id ?? task?.review_session_id;
    if (!session) return;
    closeDialog();
    return openAnywhere(session);
  },
  "review-skip": async () => {
    const id = panelTaskId();
    if (!id) return;
    const actions = await call("review_skip", { taskId: id });
    if (actions) await applyPipeline(actions);
    refreshTaskDialog(id);
    render();
  },
  "task-docs-write": () => writeTaskDoc(panelTaskId()),
  "task-publish": async () => {
    const id = panelTaskId();
    if (!id) return;
    const actions = await call("task_publish_start", { taskId: id });
    if (actions) await applyPipeline(actions);
  },

  "task-mark-done": async () => {
    const id = panelTaskId();
    if (id && (await done("task_status", { id, status: "done" }))) {
      if (state.dialog) closeDialog();
      await reloadTasks();
      render();
    }
  },
  "qa-detect": async () => {
    const id = state.projectDraft?.id;
    const found = id ? await call("qa_suggest_commands", { projectId: id }) : null;
    if (!found) return;
    if (!found.length) return toast(t("No test commands found in this project's files."));
    await saveProjectField({ test_commands: found.join("\n") });
    toast(t("Found {commands}", { commands: found.join(", ") }));
  },
  "task-sync-retry": async () => {
    const id = panelTaskId();
    if (id && (await done("task_sync_retry", { id }))) {
      toast(t("Sending to the tracker again…"));
      await reloadTasks();
      if (state.dialog?.id === id) Object.assign(state.dialog, { sync: findTask(id)?.sync });
      render();
    }
  },
  "close-project-settings": closeProjectSettings,
  "layout-1": () => setLayout(1),
  "layout-2": () => setLayout(2),
  "layout-4": () => setLayout(4),
  "awake-settings": () => openSettings("General"),
  "workspace-menu": () => {
    state.menu = state.menu?.kind === "workspace" ? null : { kind: "workspace" };
    render();
  },
  "account-menu": async () => {
    const agent = state.actionData?.agent ?? "claude";
    await loadProfiles();
    state.menu = { kind: "account", agent };
    render();
  },
  "selection-archive": async () => {
    const ids = [...state.selected];
    const running = ids.filter((id) => isRunning(id));
    for (const id of ids.filter((id) => !isRunning(id))) await done("session_archive", { id, archived: true });
    state.selected = new Set(running);
    await loadWorkspace();
    if (running.length)
      toast(
        running.length === 1
          ? t("{count} kept — stop it first.", { count: plural(running.length, "running session") })
          : t("{count} kept — stop them first.", { count: plural(running.length, "running session") }),
      );
    render();
  },
  "selection-close": () => {
    for (const id of state.selected) if (state.tabs.includes(id)) closeTab(id);
    state.selected = new Set();
    render();
  },
  "selection-clear": () => {
    state.selected = new Set();
    render();
  },
  "awake-menu": () => {
    state.menu = state.menu?.kind === "awake" ? null : { kind: "awake" };
    render();
  },
  "stop-session": () => {
    const current = session();
    state.menu = null;
    return openModal({
      kind: "confirm",
      title: t("Stop this agent?"),
      body: t("This interrupts current work. The saved conversation stays with the agent."),
      confirmLabel: t("Stop"),
      danger: true,
      run: () => terminal.stop(current.id),
    });
  },
  "start-session": () => {
    state.menu = null;
    return startSession(session().id);
  },
  "session-view": () => {
    const current = session();
    const view = state.actionData?.view;
    if (!current || !["chat", "terminal"].includes(view) || viewOf(current) === view) return;
    if (!current.running) return switchView(current.id, view, false);
    return openModal({
      kind: "confirm",
      title: t("Switch to {view}?", { view: t(view === "chat" ? "Chat" : "Terminal") }),
      body: t("The agent stops and resumes in {view}. The conversation carries over.", { view: t(view === "chat" ? "Chat" : "Terminal") }),
      confirmLabel: t("Switch"),
      danger: false,
      run: () => switchView(current.id, view, true),
    });
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
  "usage-page": () => openHome("usage"),
  "remove-project": () =>
    openModal({
      kind: "remove-project",
      id: state.projectId,
      path: project()?.path ?? "",
      children: state.projects.some((item) => item.parent_id === state.projectId),
    }),
  "confirm-remove-project": async () => {
    const id = state.dialog.id;
    if (await done("project_remove", { id })) {
      state.projectId = null;
      closeDialog();
      await loadWorkspace();
    }
  },
  "remove-all-projects": () => {
    state.menu = null;
    const running = state.allSessions.filter((item) => isRunning(item.id)).length;
    openModal({ kind: "remove-all-projects", projects: state.projects.length, sessions: state.allSessions.length, running });
  },
  "confirm-remove-all-projects": async () => {
    const count = state.dialog.projects;
    if (!(await done("projects_remove_all"))) return;
    state.projectId = null;
    state.sessionId = null;
    closeDialog();
    await loadWorkspace();
    openHome();
    toast(t("Removed {count}", { count: plural(count, "project") }));
  },
  "reveal-project": () => call("path_reveal", { path: project()?.path ?? "" }),
  "copy-path": async () => {
    await navigator.clipboard.writeText(project()?.path ?? "");
    toast(t("Path copied"));
  },
  "reconnect-project": reconnectProject,

  // sessions
  "new-session": () => openNewSession(state.projectId),
  "create-session": createSession,
  "edit-session": openEditSession,
  "rename-current": () => withSession((current) => openRename(current.id)),
  "save-session": saveSession,
  "rename-session": renameSessionNow,
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
      feedback: `Please address the review findings below, verify the changes, and report what you fixed.\n\n${(viewOf(session()) === "chat"
        ? chatText(state.sessionId)
        : terminal.buffer(state.sessionId)
      ).slice(-18000)}`,
    }),
  "start-fresh": async () => {
    const id = await call("session_recover", { id: session().id });
    if (!id) return;
    await loadSessions();
    openSession(id);
    await startSession(id);
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
  "pick-start": () => {
    state.picked = new Set();
    render();
  },
  "pick-all": () => {
    for (const key of pickableKeys()) state.picked.add(key);
    render();
  },
  "pick-none": () => {
    state.picked = new Set();
    render();
  },
  "pick-done": () => {
    state.picked = null;
    render();
  },
  "pick-delete": () => confirmDelete([...state.picked]),
  "pick-remove": () =>
    confirmRemove([...state.picked].filter((key) => key.startsWith("s:")).map((key) => key.slice(2))),
  "history-fold": () => {
    toggleHistoryCollapsed();
    render();
  },
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
  "skills-menu": () => {
    const id = state.actionData?.skillsSession ?? state.sessionId;
    state.menu = state.menu?.kind === "skills" && state.menu.id === id ? null : { kind: "skills", id };
    render();
  },
  "helpers-menu": () => {
    state.menu = state.menu?.kind === "helpers" ? null : { kind: "helpers", id: state.sessionId };
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
    if (await done("worktree_setup", { id })) toast(t("Worktree setup finished"));
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
      ...newTask(),
      spec_id: "",
    }),
  "save-task": () => saveTask(false),
  "spec-new-task": () => {
    const spec = state.dialog;
    openModal({
      kind: "task",
      title: spec.title,
      details: "",
      findings: "",
      ...newTask(),
      spec_id: spec.id,
    });
  },
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
  "unstage-all": () => mutate({ action: "unstageAll" }),
  // Resolving a conflict is staging the file once its markers are gone.
  resolve: () => mutate({ action: "stage", path: selectedPath(), original: selectedOriginal() }),
  "commit-push": async () => {
    if (await commit(false)) await mutate({ action: "push" });
  },
  "file-open": () => fileAct(false),
  "file-reveal": () => fileAct(true),
  "file-copy": async () => {
    const path = selectedPath();
    if (!path) return toast(t("Select a file first."));
    const root = state.files.snapshot?.directory ?? "";
    await navigator.clipboard.writeText(`${root}${/[\\/]$/.test(root) ? "" : "/"}${path}`);
    toast(t("Path copied"));
  },
  discard: () =>
    confirmThen(t("Discard changes to this file?"), selectedPath(), () =>
      mutate({ action: "discard", path: selectedPath() }),
    ),
  trash: trashFile,
  "discard-hunk": openHunkChoice,
  "confirm-hunk": confirmHunk,
  commit: () => commit(false),
  amend: () =>
    confirmThen(
      t("Amend the latest commit?"),
      t("The previous commit is replaced. Do not amend something already pushed."),
      () => commit(true),
    ),
  generate: generateMessage,
  fetch: () => mutate({ action: "fetch" }),
  pull: () => mutate({ action: "pull" }),
  push: () => mutate({ action: "push" }),
  pr: createPullRequest,
  revert: () =>
    confirmThen(t("Revert this commit?"), selectedCommit(), () =>
      mutate({ action: "revert", commit: selectedCommit() }),
    ),
  "reset-soft": () =>
    confirmThen(t("Reset to this commit, keeping the index?"), selectedCommit(), () =>
      mutate({ action: "resetSoft", commit: selectedCommit() }),
    ),
  "reset-mixed": () =>
    confirmThen(t("Reset to this commit and clear the index?"), selectedCommit(), () =>
      mutate({ action: "resetMixed", commit: selectedCommit() }),
    ),
  "switch-branch": () =>
    state.files.branch ? mutate({ action: "switch", branch: state.files.branch }) : null,
  "new-branch": () => openModal({ kind: "branch", branch: "" }),
  "confirm-branch": async () => {
    const branch = value("draft-branch");
    const base = value("draft-base").trim();
    closeDialog();
    await mutate(base ? { action: "branchFrom", branch, base } : { action: "branch", branch });
  },
  "copy-sha": () => copyCommit("id"),
  "copy-subject": () => copyCommit("subject"),
  "files-wrap": () => {
    state.files.wrap = !state.files.wrap;
    render();
  },
  "files-raw": () => {
    state.files.raw = !state.files.raw;
    render();
  },
  "files-load-all": () => {
    state.files.loadAll = true;
    render();
  },
  // Enter in the automation editor's fields.
  "automation-save": () => automations.onClick({ dataset: { action: "automation-save" } }),
  "confirm-generic": async () => {
    const run = state.dialog.run;
    closeDialog();
    await run();
  },
  "copy-markdown": async () => {
    await navigator.clipboard.writeText(state.dialog.text);
    toast(t("Copied"));
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
  searchAll: () => openPalette("search"),
  moveTabLeft: () => state.sessionId && moveTab(state.sessionId, state.tabs.indexOf(state.sessionId) - 1),
  moveTabRight: () => state.sessionId && moveTab(state.sessionId, state.tabs.indexOf(state.sessionId) + 1),
  claudeUsage: () => withProject(openClaudeUsage),
  projectRefresh: () => withProject(() => refreshProjects(state.projectId)),
  home: () => openHome(),
  dashboard: () => (state.page === "dashboard" ? openHome() : openHome("dashboard")),
  usagePage: () => openHome("usage"),
  reopenTab: () => reopenTab(),

  // sessions — each acts on the open session, and does nothing without one
  resume: () => withSession((current) => !current.running && startSession(current.id)),
  stop: () =>
    withSession((current) =>
      current.running &&
      openModal({
        kind: "confirm",
        title: t("Stop this agent?"),
        body: t("This interrupts current work. The saved conversation stays with the agent."),
        confirmLabel: t("Stop"),
        danger: true,
        run: () => terminal.stop(current.id),
      }),
    ),
  sleep: () => withSession((current) => current.running && done("session_hibernate", { id: current.id })),
  pin: () => withSession(() => ACTIONS["pin-session"]()),
  rename: () => withSession((current) => openRename(current.id)),
  edit: () => withSession(() => openEditSession()),
  review: () => withSession(() => openReview()),
  feedback: () => withSession((current) => current.review_of && ACTIONS["send-feedback"]()),
  // ⌘/ as on macOS: the palette over this project's quick commands.
  quick: () => withSession(() => openPalette("quick")),
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
  approvals: () => ACTIONS.approvals(),
  nextTab: () => withProject(() => stepTab(1)),
  previousTab: () => withProject(() => stepTab(-1)),
  reveal: () => withProject(() => ACTIONS["reveal-project"]()),
  copyPath: () => withProject(() => ACTIONS["copy-path"]()),
  gitRefresh: () => withSession(() => ACTIONS["git-status"]()),

  // general
  theme: () =>
    savePref({ theme: { system: "light", light: "dark", dark: "system" }[state.settings.theme] ?? "system" }),
  wake: () => {
    // As on macOS: on means "while agents run", the setting most people want.
    const next = state.settings.keep_awake === "off" ? "sessions" : "off";
    toast(next === "off" ? t("Sleep allowed") : t("Keeping the computer awake while agents run"));
    return savePref({ keep_awake: next });
  },
  limits: () => openLimits(),
  limitsRefresh: () => refreshLimits(true),
};

const TAB_ORDER = ["sessions", "specs", "tasks", "docs", "automations"];

const withSession = (run) => (session() ? run(session()) : undefined);
const withProject = (run) => (project() ? run(project()) : undefined);

/// Shows a project tab, leaving a session, Files or the settings page.
async function showTab(tab) {
  state.page = null;
  state.tab = tab;
  state.taskId = null;
  state.sessionId = null;
  state.files = null;
  keepProjectTab();
  render();
  if (tab === "specs" || tab === "tasks") await loadPlanning();
  if (tab === "docs") await loadDocs();
  if (tab === "automations") await automations.reload();
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
      "[data-approve],[data-export],[data-prepare],[data-remove-profile]," +
      "[data-remove-command],[data-import],[data-task-view],[data-palette],[data-split],"
      + "[data-capture],[data-expand],[data-project-more],[data-menu-act],[data-settings-section]," +
      "[data-pref-set],[data-pref-toggle],[data-icon-tab],[data-proj-color],[data-proj-icon],[data-sync-toggle],[data-intake-toggle]," +
      "[data-proj-toggle],[data-proj-clear],[data-icon-source],[data-doc],[data-activity-open]," +
      "[data-inbox-answer],[data-inbox-open],[data-inbox-expand],[data-tab-close],[data-tab-open],[data-related-remove],[data-tab-act],[data-awake],[data-pane-pick],[data-pane-close]," +
      "[data-layout],[data-needs-you],[data-spec-brief],[data-group-toggle],[data-workspace-option],[data-account-pick],[data-split-into],[data-pane-new],[data-pane-max],[data-pane-quick],[data-command-edit],[data-shortcut-reset],[data-history-copy],[data-history-resume],[data-history-delete],[data-knowledge-remove],[data-pick-row],[data-sweep-group],[data-sweep-all],[data-settings-open],[data-sessions-show],[data-project-tab],[data-preset],[data-home-tasks],[data-task-scope],[data-task-group],[data-task-run],[data-task-pr],[data-task-more],[data-use-account],[data-login-account],[data-issue],[data-conn-add],[data-conn-edit],[data-conn-remove],[data-conn-auth],[data-issue-source],[data-auto],[data-auto-day],[data-skill],[data-skill-pick],[data-skill-toggle],[data-usage-days],[data-usage-agent],[data-usage-table],[data-price-add],[data-price-remove],[data-price-reset],[data-price-save],[data-task-doc-open],[data-task-doc-reveal],[data-task-doc],[data-repo-remove],[data-task-pr-extra],[data-import-pick],[data-import-all],[data-import-scope],[data-import-project],[data-import-role],[data-import-auto]",
  );
  if (!target) return;
  const data = target.dataset;
  if (await automations.onClick(target)) return;
  if (await skills.onClick(target)) return;
  if (usage.onClick(target)) return;
  // A double click on a session's name renames it, in the sidebar and the
  // Sessions list (the header's menu has Rename). Counted from the click itself, since the
  // first click redraws the list and a dblclick never reaches the new row.
  if (event.detail === 2 && !state.picked) {
    const rename = target.classList.contains("side-session")
      ? data.open
      : target.matches(".row[data-open]") && event.target.closest(".row__title")
        ? data.open
        : null;
    if (rename) return openRename(rename);
  }
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
  if (data.inboxAnswer) {
    // Busy until the backend says it was resolved; the row then goes.
    return answer(data.inboxSession, data.inboxRequest, data.inboxAnswer);
  }
  if (data.inboxOpen) {
    state.menu = null;
    return openAnywhere(data.inboxOpen);
  }
  if (data.inboxExpand) {
    if (!state.inboxExpanded.delete(data.inboxExpand)) state.inboxExpanded.add(data.inboxExpand);
    return render();
  }
  if (data.relatedRemove && state.dialog?.kind === "task") {
    captureDraft();
    state.dialog.related = (state.dialog.related ?? []).filter((id) => id !== data.relatedRemove);
    return render();
  }
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
    if (data.syncToggle) return toggleSync(data.syncToggle);
    if (data.intakeToggle) return saveIntake({ [data.intakeToggle]: !state.projectDraft?.link?.intake?.[data.intakeToggle] });
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
      if (data.settingsSection === "Usage prices") usage.loadPrices();
      if (data.settingsSection === "Setup" && !state.diagnostics) await runDiagnostics();
      if (data.settingsSection === "Setup") checkMacImport().then(render);
      return;
    }
    if (data.prefSet === "language") {
      setLanguage(data.value);
      return render();
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
    state.anchor = { top: box.bottom + 4, right: Math.max(8, window.innerWidth - box.right), left: box.left };
    if (target.closest(".chat__composer")) state.anchor.bottom = window.innerHeight - box.top + 6;
    const handler = ACTIONS[data.action];
    // What else the clicked control says, for actions that need it.
    state.actionData = { ...data };
    if (handler) return handler();
    return;
  }
  if (data.project) return showProject(data.project);
  if (data.settingsOpen) return openSettings(data.settingsOpen);
  if (data.homeTasks) {
    state.page = null;
    state.taskScope = "all";
    state.tab = "tasks";
    await loadAllTasks();
    await loadPlanning();
    return render();
  }
  if (data.projectTab) {
    state.projectSettingsTab = data.projectTab;
    return render();
  }
  // A preset writes its settings in one go; Autopilot says first what it
  // will let run by itself.
  if (data.preset) {
    const draft = state.projectDraft;
    const patch = draft && presetPatch(data.preset, draft);
    if (!patch) return;
    if (data.preset !== "autopilot") return saveProjectField(patch);
    return openModal({
      kind: "confirm",
      title: t("Run this project's tasks on autopilot?"),
      body: patch.qa_auto
        ? t("New tasks are planned and their plans approved by themselves, then built, reviewed, tested and published as pull requests, {count} at a time. The queue starts the next task when one finishes.", { count: patch.max_parallel })
        : t("New tasks are planned and their plans approved by themselves, then built, reviewed and published as pull requests, {count} at a time. The queue starts the next task when one finishes.", { count: patch.max_parallel }),
      confirmLabel: t("Use Autopilot"),
      run: () => saveProjectField(patch),
    });
  }
  // The Sessions page's All / Reviews switch: the same page, two lists.
  if (data.sessionsShow) {
    state.tab = data.sessionsShow === "reviews" ? "reviews" : "sessions";
    keepProjectTab();
    return render();
  }
  if (data.tab) {
    if (state.page === "project") state.page = null;
    state.tab = data.tab;
    state.taskId = null;
    state.sessionId = null;
    state.files = null;
    keepProjectTab();
    render();
    if (data.tab === "specs" || data.tab === "tasks") await loadPlanning();
    if (data.tab === "docs") await loadDocs();
    if (data.tab === "automations") await automations.reload();
    return;
  }
  if (data.start) {
    // From Home or the Dashboard it may be another project's session.
    if (!state.sessions.some((item) => item.id === data.start)) await openAnywhere(data.start);
    return startSession(data.start);
  }
  if (data.stop) return terminal.stop(data.stop);
  // ⌘-click (Ctrl-click elsewhere) picks sessions in the sidebar.
  if (data.open && (event.metaKey || event.ctrlKey) && target.classList.contains("side-session")) {
    if (state.selected.has(data.open)) state.selected.delete(data.open);
    else state.selected.add(data.open);
    return render();
  }
  if (data.open) {
    // Opened from the helpers menu: the menu has done its job.
    if (state.menu?.kind === "helpers") state.menu = null;
    return openAnywhere(data.open);
  }
  if (data.split || data.splitInto) {
    const [pane, id] = data.splitInto
      ? [Number(data.splitInto.split(":")[0]), data.splitInto.slice(data.splitInto.indexOf(":") + 1)]
      : [state.dialog?.pane ?? state.focus, data.split];
    state.dialog = null;
    state.panes[pane] = id;
    state.focus = pane;
    savePanes();
    return openAnywhere(id);
  }
  if (data.paneNew !== undefined) {
    state.focus = Number(data.paneNew);
    return ACTIONS["new-session"]();
  }
  if (data.paneMax !== undefined) {
    const id = state.panes[Number(data.paneMax)];
    await setLayout(1);
    return id ? openAnywhere(id) : render();
  }
  if (data.paneQuick !== undefined) {
    await focusPane(Number(data.paneQuick));
    return ACTIONS["quick-menu"]();
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
    if (viewOf(session()) === "chat") return quickInChat(state.sessionId, data.quick);
    return done("quick_command_send", {
      sessionId: state.sessionId,
      commandId: data.quick,
    });
  }
  if (data.set) {
    captureDraft();
    state.dialog[fieldFor(data.set)] = data.value;
    if (["session", "review"].includes(state.dialog.kind) && data.set === "agent") {
      state.dialog.account = activeAccount(data.value);
      Object.assign(state.dialog, lastChoice(data.value));
    }
    // The skills offered follow the agent.
    if (data.set === "agent" && Array.isArray(state.dialog.skills)) {
      fitSkills(state.dialog, data.value, state.dialog.projectId ?? state.dialog.project_id ?? state.projectId);
    }
    // An account and a model belong to one agent; switching lets them go.
    if (state.dialog.kind === "task" && data.set === "agent") {
      const own = (state.profiles ?? []).some((item) => item.id === state.dialog.profile_id && item.agent === data.value);
      if (!own) state.dialog.profile_id = "";
      Object.assign(state.dialog, lastChoice(data.value));
    }
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
  if (data.commandEdit) {
    const command = state.quickCommands.find((item) => item.id === data.commandEdit);
    if (!command) return;
    Object.assign(state.dialog, {
      editing: command.id,
      title: command.title,
      text: command.text,
      submit: command.submit,
      scoped: !!command.project_id,
    });
    return render();
  }
  if (data.shortcutReset) {
    const next = { ...(state.settings.shortcuts ?? {}) };
    delete next[data.shortcutReset];
    return savePref({ shortcuts: next });
  }
  if (data.groupToggle) {
    const key = `group:${data.groupToggle}`;
    if (state.collapsed.has(key)) state.collapsed.delete(key);
    else state.collapsed.add(key);
    saveCollapsed();
    return render();
  }
  if (data.workspaceOption) {
    state.menu = null;
    if (data.workspaceOption === "hierarchy") {
      state.groupHierarchy = !state.groupHierarchy;
      try {
        localStorage.setItem("convoy.groupHierarchy", state.groupHierarchy ? "1" : "0");
      } catch {}
      return render();
    }
    if (data.workspaceOption === "sort") return savePref({ sort_projects: !state.settings.sort_projects });
    if (data.workspaceOption === "compact") return savePref({ compact_sidebar: state.settings.compact_sidebar === false });
  }
  if (data.accountPick) {
    const [agent, id] = data.accountPick.split(":");
    rememberAccount(agent, id);
    state.menu = null;
    return render();
  }
  if (data.specBrief) {
    const brief = await call("review_brief", { id: data.specBrief });
    if (brief === undefined) return;
    await navigator.clipboard.writeText(brief);
    return toast(t("Review brief copied"));
  }
  if (data.historyCopy) {
    await navigator.clipboard.writeText(data.historyCopy);
    return toast(t("Conversation ID copied"));
  }
  if (data.historyResume) return resumeHistory(data.historyResume);
  if (data.historyDelete) return confirmDelete([data.historyDelete]);
  if (data.knowledgeRemove && state.dialog?.kind === "session") {
    captureDraft();
    state.dialog.knowledge = state.dialog.knowledge.filter((item) => item.key !== data.knowledgeRemove);
    return render();
  }
  // The checkbox itself is handled as a change; the rest of the row toggles it.
  if (data.pickRow && state.picked && !event.target.matches("[data-pick]")) {
    togglePick(data.pickRow, !state.picked.has(data.pickRow));
    return render();
  }
  if (data.sweepGroup && state.dialog?.kind === "history-sweep") {
    const dialog = state.dialog;
    const key = data.sweepGroup === "-" ? "" : data.sweepGroup;
    const group = sweepGroups(dialog).find((item) => item.key === key);
    for (const row of group?.rows ?? []) if (sweepProject(dialog, row)) dialog.checked[row.provider_id] = data.value === "all";
    return render();
  }
  // Select all picks what the search shows; Deselect all clears every row,
  // shown or not, so nothing hidden is imported by surprise.
  if (data.sweepAll && state.dialog?.kind === "history-sweep") {
    if (data.sweepAll === "all") for (const row of sweepVisible(state.dialog)) state.dialog.checked[row.provider_id] = true;
    else state.dialog.checked = {};
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
  if (data.taskPrExtra) {
    const [id, index] = data.taskPrExtra.split(":");
    return done("task_pr_open", { id, extra: Number(index) });
  }
  if (data.repoRemove && state.dialog?.kind === "task") {
    captureDraft();
    state.dialog.extra_projects = (state.dialog.extra_projects ?? []).filter((id) => id !== data.repoRemove);
    return render();
  }
  if (data.taskDocOpen) return done("task_doc_open", { projectId: state.dialog?.project_id ?? state.projectId, path: data.taskDocOpen });
  if (data.taskDocReveal) return done("task_doc_reveal", { projectId: state.dialog?.project_id ?? state.projectId, path: data.taskDocReveal });
  if (data.taskDoc) return openTaskDoc(data.taskDoc);
  if (data.taskMore) {
    const box = target.getBoundingClientRect();
    return openContextMenu("task", data.taskMore, box.left - 180, box.bottom + 4);
  }
  if (data.taskView) {
    state.taskView = data.taskView;
    try {
      localStorage.setItem("convoy.taskView", state.taskView);
    } catch {
      /* not remembered */
    }
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
  if (usage.onInput(field)) return;
  // Anything re-renders — an agent reporting, the activity feed — and a
  // dialog is rebuilt from its state, so what is typed goes into that state
  // as it is typed. Reading the fields back at render time instead would undo
  // a handler that had just set them.
  const draftKey = DRAFT_FIELDS[field.id];
  if (draftKey && state.dialog && !isImportDialog() && field.closest(".modal")) state.dialog[draftKey] = field.value;
  if (isImportDialog() && imports.onInput(field)) return;
  if (automations.onInput(field)) return;
  if (skills.onInput(field)) return;
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
  if (field.id === "spec-filter") {
    state.specFilter = field.value;
    return render();
  }
  if (field.id === "sweep-filter" && state.dialog?.kind === "history-sweep") {
    state.dialog.query = field.value;
    return render();
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
  if (state.page === "project" && (onLinkChange(event.target) || onSyncChange(event.target))) return;
  if (automations.onChange(event.target)) return;
  if (skills.onChange(event.target)) return;
  if (usage.onChange(event.target)) return;
  if (event.target.dataset?.linkTask && event.target.value) {
    const id = event.target.dataset.linkTask;
    if (await done("task_link_session", { id, sessionId: event.target.value })) {
      await loadWorkspace();
      await loadPlanning();
      toast(t("Session linked"));
    }
    return render();
  }
  if (event.target.dataset?.relatedAdd && state.dialog?.kind === "task" && event.target.value) {
    captureDraft();
    state.dialog.related = [...(state.dialog.related ?? []), event.target.value];
    return render();
  }
  if (event.target.dataset?.dependsOn !== undefined && state.dialog?.kind === "task") {
    const picked = [...document.querySelectorAll("[data-depends-on]:checked")].map((box) => box.dataset.dependsOn);
    state.dialog.depends_on = picked;
    return;
  }
  if (event.target.id === "spec-filter" || event.target.id === "sweep-filter") return;
  if (event.target.dataset?.pick && state.picked) {
    togglePick(event.target.dataset.pick, event.target.checked);
    return render();
  }
  if (state.dialog?.kind === "history-sweep") {
    const { sweepCheck, sweepPick } = event.target.dataset ?? {};
    if (sweepCheck) state.dialog.checked[sweepCheck] = event.target.checked;
    // Picking a project for an unassigned conversation selects it.
    if (sweepPick) {
      state.dialog.picks[sweepPick] = event.target.value;
      state.dialog.checked[sweepPick] = !!event.target.value;
    }
    if (sweepCheck || sweepPick) return render();
  }
  if (event.target.id === "files-repo" && state.files) {
    state.files = { ...state.files, id: event.target.value, snapshot: null, selection: null, preview: null, branch: null };
    render();
    return loadFiles();
  }
  if (state.dialog?.kind === "session" && event.target.id === "draft-project") {
    captureDraft();
    return sessionProject(state.dialog, event.target.value);
  }
  // Picking a conversation adds it as a chip; the menu goes back to its prompt.
  if (state.dialog?.kind === "session" && event.target.id === "draft-knowledge") {
    captureDraft();
    const key = event.target.value;
    if (!key) return;
    const draft = state.dialog;
    const entry = key.startsWith("s:")
      ? anySession(key.slice(2))
      : draft.saved?.find((item) => `c:${item.agent}:${item.provider_id}` === key);
    if (entry && !draft.knowledge.some((item) => item.key === key)) {
      draft.knowledge.push({ key, agent: entry.agent, title: entry.title || t("Untitled conversation") });
    }
    return render();
  }
  if (["session", "review"].includes(state.dialog?.kind) && event.target.id === "draft-account") {
    state.dialog.account = event.target.value;
    return render();
  }
  // The effort levels follow the model picked.
  if (["session", "review"].includes(state.dialog?.kind) && event.target.id === "draft-model") {
    captureDraft();
    return render();
  }
  if (state.dialog?.kind === "task" && event.target.dataset?.repoAdd && event.target.value) {
    captureDraft();
    state.dialog.extra_projects = [...(state.dialog.extra_projects ?? []), event.target.value];
    return render();
  }
  if (state.dialog?.kind === "task" && event.target.id === "task-plan-auto") {
    state.dialog.plan_auto_approve = { yes: true, no: false }[event.target.value] ?? null;
    return render();
  }
  if (state.dialog?.kind === "task" && ["task-account", "task-spec", "task-model"].includes(event.target.id)) {
    state.dialog[DRAFT_FIELDS[event.target.id]] = event.target.value;
    return render();
  }
  // A role's instructions: the whole map is saved. The reviewer's replaces
  // the older review template, which is cleared so it no longer shows through.
  const projectRole = event.target.dataset?.projRole;
  if (projectRole) {
    const roles = { ...(state.projectDraft?.role_instructions ?? {}), [projectRole]: event.target.value };
    return saveProjectField({ role_instructions: roles, ...(projectRole === "review" ? { review_template: "" } : {}) });
  }
  const intakeKey = event.target.dataset?.intakeText ?? event.target.dataset?.intakeSelect ?? event.target.dataset?.intakeCheck;
  if (intakeKey) {
    const value = event.target.dataset.intakeCheck ? event.target.checked : event.target.dataset.intakeSelect ? Number(event.target.value) : event.target.value;
    return saveIntake({ [intakeKey]: value });
  }
  const globalRole = event.target.dataset?.prefRole;
  if (globalRole) {
    const roles = { ...(state.settings.role_instructions ?? {}), [globalRole]: event.target.value };
    return savePref({ role_instructions: roles, ...(globalRole === "review" ? { review_template: "" } : {}) });
  }
  const projectKey = event.target.dataset?.projText ?? event.target.dataset?.projSelect;
  if (projectKey) {
    const numeric = ["qa_timeout", "max_fix_rounds", "max_parallel"].includes(projectKey);
    return saveProjectField({ [projectKey]: numeric ? Number(event.target.value) : event.target.value });
  }
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
    const action = state.capturing;
    // Backspace or Delete leaves the action unbound, as on macOS.
    if (event.key === "Backspace" || event.key === "Delete") {
      state.capturing = null;
      return savePref({ shortcuts: { ...(state.settings.shortcuts ?? {}), [action]: "" } });
    }
    const pressed = capture(event);
    if (!pressed) return;
    const taken = SHORTCUTS.find(([other]) => other !== action && binding(other) === pressed);
    if (taken) {
      toast(t("{key} is already “{action}”. Change that one first, or press another.", { key: label(pressed), action: t(taken[2]) }));
      return;
    }
    state.capturing = null;
    return savePref({ shortcuts: { ...(state.settings.shortcuts ?? {}), [action]: pressed } });
  }
  if (event.target.id === "find-term" && (event.key === "Enter" || event.key === "Escape")) {
    event.preventDefault();
    if (event.key === "Escape") return closeFind();
    return findStep({ previous: event.shiftKey });
  }
  if (event.key === "Escape") {
    // Claimed whatever it does: an Escape the page leaves alone reaches the
    // macOS window, which takes it to mean leave full screen. It closes a
    // menu or dialog first; outside one, a search box still clears, as it
    // would by default.
    event.preventDefault();
    if (state.menu || state.dialog) return closeDialog();
    const field = event.target;
    if (field?.type === "search" && field.value) {
      field.value = "";
      field.dispatchEvent(new Event("input", { bubbles: true }));
      return;
    }
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
      if (size) toast(t("Terminal text {size} pt", { size }));
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

// ---------------------------------------------------------------- projects --

/// Puts a project just before another. Across groups it joins the target's
/// group, which is what a drag onto that group's row reads as.
async function placeProject(id, before) {
  const index = state.projects.findIndex((item) => item.id === before);
  const from = state.projects.findIndex((item) => item.id === id);
  if (index < 0 || from < 0) return;
  const moving = state.projects[from];
  const target = state.projects[index];
  if ((moving.group ?? "") !== (target.group ?? "")) {
    return toast(moving.group || target.group ? t("Projects move within their group.") : "");
  }
  if (await done("project_move", { id, index: from < index ? index - 1 : index })) await loadWorkspace();
}

/// Move up or down among the projects of the same group.
async function moveProject(id, step) {
  const moving = state.projects.find((item) => item.id === id);
  const peers = state.projects.filter((item) => (item.group ?? "") === (moving?.group ?? ""));
  const at = peers.findIndex((item) => item.id === id);
  const next = peers[at + step];
  if (!next) return render();
  const index = state.projects.findIndex((item) => item.id === next.id);
  if (await done("project_move", { id, index })) await loadWorkspace();
}

async function refreshProjects(id) {
  const added = await call("project_refresh", { id });
  if (added === undefined) return;
  await loadWorkspace();
  toast(added ? t("Added {count}", { count: plural(added, "project") }) : t("No new projects in this folder"));
}

// ----------------------------------------------------------------- history --

async function loadHistory() {
  const projectId = state.projectId;
  if (!projectId) return;
  const sessions = state.sessions.length;
  const entries = await call("history_scan", { projectId });
  if (state.projectId !== projectId) return;
  // A failed read is kept as empty, so the next render does not retry it.
  state.history = { projectId, sessions, entries: entries ?? [] };
  render();
}

/// Reads the project's saved conversations when the list shows a project it
/// has not read, or the project's sessions changed — a resume or an import
/// moves a conversation out of the list.
let historyLoading = null;
function ensureHistory() {
  const key = `${state.projectId}:${state.sessions.length}`;
  const read = state.history && `${state.history.projectId}:${state.history.sessions}`;
  if (!state.projectId || key === read || historyLoading === key) return;
  historyLoading = key;
  loadHistory().finally(() => {
    if (historyLoading === key) historyLoading = null;
  });
}

function togglePick(key, on) {
  if (on) state.picked.add(key);
  else state.picked.delete(key);
}

/// Asks first, then deletes sessions and saved conversations: each
/// conversation goes to the Trash and its session leaves Convoy.
function confirmDelete(keys) {
  if (!keys.length) return;
  const items = keys.map((key) => {
    if (key.startsWith("s:")) return { session_id: key.slice(2) };
    const [, agent, provider_id] = key.split(":");
    return { agent, provider_id };
  });
  const [key] = keys;
  const name =
    keys.length !== 1
      ? null
      : key.startsWith("s:")
        ? state.sessions.find((item) => item.id === key.slice(2))?.title
        : state.history?.entries.find((entry) => `c:${entry.agent}:${entry.provider_id}` === key)?.title;
  openModal({
    kind: "confirm",
    title: name
      ? t('Delete "{name}"?', { name })
      : t("Delete {count}?", { count: plural(keys.length, "conversation") }),
    body: t("The saved conversation moves to the Trash and leaves Convoy. You can put it back from the Trash."),
    confirmLabel: t("Delete"),
    danger: true,
    run: () => deleteConversations(items),
  });
}

/// Takes sessions out of Convoy, leaving their conversations on disk; they
/// show again under "Not in Convoy yet".
function confirmRemove(ids) {
  if (!ids.length) return;
  const name = ids.length === 1 ? state.sessions.find((item) => item.id === ids[0])?.title : null;
  openModal({
    kind: "confirm",
    title: name
      ? t('Remove "{name}" from Convoy?', { name })
      : t("Remove {count} from Convoy?", { count: plural(ids.length, "session") }),
    body: t("Only Convoy forgets them. The conversations stay with their agent and show under “Not in Convoy yet”."),
    confirmLabel: t("Remove"),
    run: async () => {
      const report = await call("sessions_remove", { ids });
      if (!report) return render();
      const [removed, running] = report;
      for (const id of ids) if (state.tabs.includes(id) && !isRunning(id)) closeTab(id);
      if (ids.includes(state.sessionId) && !isRunning(state.sessionId)) state.sessionId = null;
      if (state.picked) state.picked = new Set();
      await loadWorkspace();
      await loadSessions();
      await loadHistory();
      const kept = running ? t("{count} running — stop first", { count: plural(running, "session") }) : "";
      toast([t("Removed {count} from Convoy", { count: plural(removed, "session") }), kept].filter(Boolean).join(" · "));
    },
  });
}

async function deleteConversations(items) {
  const report = await call("sessions_delete", { items });
  if (!report) return render();
  const gone = new Set(items.map((item) => item.session_id).filter(Boolean));
  for (const id of gone) {
    if (!anySession(id)) continue;
    if (state.tabs.includes(id)) closeTab(id);
  }
  if (gone.has(state.sessionId)) state.sessionId = null;
  if (state.picked) state.picked = new Set();
  await loadWorkspace();
  await loadSessions();
  await loadHistory();
  const notes = [
    report.running ? t("{count} running — stop first", { count: plural(report.running, "session") }) : "",
    report.failed ? t("{count} could not be moved to the Trash", { count: plural(report.failed, "conversation") }) : "",
  ].filter(Boolean);
  toast([t("Deleted {count}", { count: plural(report.deleted, "conversation") }), ...notes].join(" · "));
}

/// Opens the conversation's session, recording one first when there is none,
/// and resumes it.
async function resumeHistory(providerId) {
  const entry = state.history?.entries.find((item) => item.provider_id === providerId);
  if (!entry) return;
  let id = entry.session_id;
  if (!id) {
    id = await call("transcripts_import", {
      projectId: state.projectId,
      agent: entry.agent,
      providerId: entry.provider_id,
      title: entry.title || t("Imported conversation"),
      profileId: entry.profile_id,
      directory: entry.directory,
    });
    if (!id) return;
    await loadWorkspace();
    await loadHistory();
  }
  await openAnywhere(id);
  if (!isRunning(id)) await startSession(id);
}

/// Every Claude Code and Codex conversation on this computer that is not in
/// Convoy yet, matched to a project for review. Scanning reads the disk off
/// the main thread; the dialog shows a spinner until it is done.
async function openHistorySweep() {
  const dialog = { kind: "history-sweep", rows: null, checked: {}, picks: {}, query: "", busy: false };
  openModal(dialog);
  const rows = await call("history_sweep_scan");
  if (state.dialog !== dialog) return;
  if (rows === undefined) return closeDialog();
  dialog.rows = rows;
  for (const row of rows) if (row.project_id) dialog.checked[row.provider_id] = true;
  render();
}

/// Records the checked conversations as resumable sessions. Nothing starts.
async function runHistorySweep() {
  const dialog = state.dialog;
  if (dialog?.kind !== "history-sweep" || dialog.busy) return;
  const choices = sweepChoices(dialog);
  if (!choices.length) return;
  dialog.busy = true;
  render();
  const report = await call("history_sweep_import", { choices });
  if (!report) {
    dialog.busy = false;
    return render();
  }
  if (state.dialog === dialog) closeDialog();
  await loadWorkspace();
  if (state.tab === "sessions") await loadHistory();
  const failed = report.failed ? t("{count} could not be copied", { count: plural(report.failed, "conversation") }) : "";
  toast([t("Imported {count}", { count: plural(report.imported, "conversation") }), failed].filter(Boolean).join(" · "));
}

/// ⌥⌘U as on macOS: a Claude session in this project that runs /usage, or
/// the one already open.
async function openClaudeUsage() {
  const existing = state.sessions.find((item) => item.agent === "claude" && item.title === "Claude usage" && isRunning(item.id));
  if (existing) {
    openSession(existing.id);
    return toast(t("Claude usage session selected. Run /usage there to refresh."));
  }
  const id = await call("session_create", {
    projectId: state.projectId,
    agent: "claude",
    title: "Claude usage",
    prompt: "/usage",
    model: "",
    profileId: activeAccount("claude") || null,
    notes: null,
    // /usage is the agent's own screen; only the terminal shows it.
    view: "terminal",
  });
  if (!id) return;
  await loadWorkspace();
  openSession(id);
  await startSession(id);
}

async function copyCommit(what) {
  const id = selectedCommit();
  if (!id) return toast(t("Select a commit first."));
  const commit = state.files?.snapshot?.log.find((entry) => entry.id === id);
  await navigator.clipboard.writeText(what === "id" ? id : (commit?.subject ?? ""));
  toast(what === "id" ? t("SHA copied") : t("Subject copied"));
}

async function openFolder() {
  const chosen = await openDialogNative({ directory: true, multiple: false });
  if (!chosen) return;
  const added = await call("project_open", { path: chosen });
  if (added === undefined) return;
  if (added > 1) toast(t("Added {count}", { count: plural(added, "project") }));
  await loadWorkspace();
  // The folder just opened is what to show next.
  if (["home", "dashboard", "usage"].includes(state.page)) state.page = null;
  if (!state.page && !state.sessionId && !state.files) keepProjectTab();
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
/// The project's settings, on one of its four parts (views-project.js); by
/// default the part last shown.
async function openProjectSettings(tab) {
  if (typeof tab === "string") state.projectSettingsTab = tab;
  if (!(await reloadProjectDraft())) return;
  await loadProjectLink();
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
  // The link is read on its own, so a field saved elsewhere keeps it.
  const previous = state.projectDraft?.id === state.projectId ? state.projectDraft : null;
  state.projectDraft = { ...project(), ...detail, connections: previous?.connections, link: previous?.link };
  return true;
}

// ------------------------------------------------------- integration link --

/// The connections, the project's link to one of them, and the workspaces
/// and projects it can narrow to.
async function loadProjectLink() {
  const id = state.projectDraft.id;
  const [connections, link] = await Promise.all([
    call("integrations_list"),
    call("project_link_get", { projectId: id }),
  ]);
  if (state.projectDraft?.id !== id) return;
  state.projectDraft.connections = connections ?? [];
  state.projectDraft.link = link ?? null;
  state.linkLevels = {};
  await loadLinkLevels();
}

const linkConnection = () =>
  state.projectDraft?.connections?.find((item) => item.id === state.projectDraft.link?.connection_id);

/// Reads whichever level lists the link needs and does not have yet.
async function loadLinkLevels() {
  const link = state.projectDraft?.link;
  const connection = linkConnection();
  if (!link || !connection || connection.auth === "mcp") return render();
  const levels = (state.linkLevels ??= {});
  const read = async (slot, owner, parent) => {
    if (levels[slot]?.for === owner) return;
    levels[slot] = { for: owner, loading: true, items: [] };
    render();
    const answer = await attempt("integration_scopes", { connectionId: connection.id, parent });
    if (levels[slot]?.for !== owner) return;
    levels[slot] = answer.ok ? { for: owner, items: answer.value } : { for: owner, items: [], error: answer.error };
  };
  await read("workspaces", connection.id, null);
  if (link.workspace && connection.levels[1]) await read("projects", `${connection.id}:${link.workspace.id}`, link.workspace);
  render();
  if (link.sync?.enabled) await loadSyncChoices();
}

// ------------------------------------------------------------ status sync --

/// The tracker's statuses for the mapping, read again whenever the link
/// points somewhere else.
async function loadSyncChoices() {
  const link = state.projectDraft?.link;
  if (!link) return;
  const key = [link.connection_id, link.workspace?.id, link.project?.id].join("/");
  if (state.syncChoices?.for === key && !state.syncChoices.error) return;
  state.syncChoices = { for: key, loading: true, statuses: [], suggested: {} };
  render();
  const answer = await attempt("tracker_statuses", { projectId: state.projectDraft.id });
  if (state.syncChoices?.for !== key) return;
  state.syncChoices = answer.ok ? { for: key, ...answer.value } : { for: key, statuses: [], suggested: {}, error: answer.error };
  // A sync switched on before the statuses were read starts from the suggestion.
  const sync = state.projectDraft?.link?.sync;
  if (answer.ok && sync?.enabled && !Object.keys(sync.map ?? {}).length && Object.keys(answer.value.suggested).length) {
    return saveProjectLink({ ...state.projectDraft.link, sync: { ...sync, map: answer.value.suggested } });
  }
  render();
}

function toggleSync(field) {
  const link = state.projectDraft?.link;
  if (!link) return;
  const sync = link.sync ?? { enabled: false, map: {}, link_pr: true };
  const next = { ...sync, [field]: !sync[field] };
  if (field === "enabled" && next.enabled && !Object.keys(next.map ?? {}).length) next.map = state.syncChoices?.suggested ?? {};
  return saveProjectLink({ ...link, sync: next });
}

function onSyncChange(field) {
  if (!field.dataset?.syncStatus) return false;
  const link = state.projectDraft?.link;
  if (!link?.sync) return false;
  const map = { ...link.sync.map };
  const picked = state.syncChoices?.statuses.find((status) => status.name === field.value);
  if (picked) map[field.dataset.syncStatus] = picked;
  else delete map[field.dataset.syncStatus];
  saveProjectLink({ ...link, sync: { ...link.sync, map } });
  return true;
}

async function saveProjectLink(link) {
  const projectId = state.projectDraft?.id;
  if (!projectId || !(await done("project_link_save", { projectId, link }))) return;
  state.projectDraft.link = link;
  await loadLinkLevels();
}

/// The three pickers on the project page: connection, then workspace, then
/// project, each emptying the ones below it.
function onLinkChange(field) {
  const link = state.projectDraft?.link;
  const pick = (slot) => state.linkLevels?.[slot]?.items.find((item) => item.id === field.value) ?? null;
  let next;
  if (field.id === "link-connection") {
    state.linkLevels = {};
    next = field.value ? { connection_id: field.value, workspace: null, project: null } : null;
  } else if (field.id === "link-workspace" && link) {
    next = { ...link, workspace: pick("workspaces"), project: null };
  } else if (field.id === "link-project" && link) {
    next = { ...link, project: pick("projects") };
  } else {
    return false;
  }
  saveProjectLink(next);
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
      filters: [{ name: t("Image"), extensions: ["png", "jpg", "jpeg"] }],
    });
    if (!chosen) return;
    value = chosen;
  }
  if (source === "favicon") {
    value = document.querySelector("#proj-favicon")?.value.trim() ?? "";
    if (!value) return toast(t("Enter a domain such as example.com."));
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
  return `${agentName(agent)} · ${when}`;
}

async function openNewSession(projectId) {
  if (!projectId) return toast(t("Open a folder first."));
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
    view: viewOf(null),
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
  Object.assign(draft, lastChoice(draft.agent));
  draft.worktree = !!state.settings.worktree_by_default;
  draft.branchEdited = false;
  draft.branch = suggestBranch(draft.title || "session", projectId);
  draft.refs = [];
  draft.repo = false;
  draft.base = "";
  draft.saved = [];
  draft.knowledge ??= [];
  fitSkills(draft, draft.agent, projectId);
  render();
  const [refs, detail, history] = await Promise.all([
    call("git_refs", { projectId }),
    call("project_detail", { id: projectId }),
    call("history_scan", { projectId }),
  ]);
  if (state.dialog !== draft || draft.projectId !== projectId) return;
  // Saved conversations no session holds, for the knowledge picker.
  draft.saved = (history ?? []).filter((entry) => !entry.session_id);
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
    effort: effortLevels(draft.agent, draft.account ?? "", draft.model ?? "").includes(draft.effort) ? draft.effort : null,
    profileId: draft.account || null,
    notes: draft.notes,
    view: draft.view === "chat" ? "chat" : "terminal",
    knowledge: (draft.knowledge ?? []).map((item) => {
      if (item.key.startsWith("s:")) return { session_id: item.key.slice(2) };
      const [, agent, provider_id] = item.key.split(":");
      return { agent, provider_id, title: item.title };
    }),
    manager: !!draft.manager,
    skills: draft.skills ?? null,
  });
  draft.busy = false;
  if (!id) return render();
  rememberAccount(draft.agent, draft.account);
  rememberChoice(draft.agent, draft.model, draft.effort);
  const wantsWorktree = draft.repo && draft.worktree;
  const branch = (draft.branch ?? "").trim() || suggestBranch(title, target);
  const base = (draft.base ?? "").trim();
  const setup = draft.setup;
  if (draft.keepOpen) {
    Object.assign(draft, { title: "", prompt: "", notes: "", branchEdited: false, branch: suggestBranch("session", target) });
    toast(t("Started {title}", { title }));
  } else {
    closeDialog();
  }
  await loadWorkspace();
  if (target !== state.projectId) await selectProject(target);
  if (!draft.keepOpen) openSession(id);

  if (wantsWorktree) {
    const made = await attempt("worktree_create", { id, branch, base: base || null });
    if (!made.ok) {
      toast(t("Started in the project folder: {error}", { error: made.error }));
    } else {
      await loadSessions();
      const plan = made.value;
      if (setup !== "skip" && (plan.shared_paths.length || plan.setup_command)) {
        toast(t("Running worktree setup…"));
        if (!(await done("worktree_setup", { id }))) return;
      }
    }
  }
  // Creating is starting, as on macOS; the first message goes with it.
  await startSession(id, { background: draft.keepOpen });
}

/// The rename sheet, with the name selected so typing replaces it.
function openRename(id) {
  const target = anySession(id);
  if (!target) return;
  openModal({ kind: "rename", id, title: target.title });
  document.querySelector("#rename-title")?.select();
}

async function renameSessionNow() {
  captureDraft();
  const dialog = state.dialog;
  if (dialog?.kind !== "rename") return;
  const title = dialog.title.trim();
  if (!title) return toast(t("Give the session a name."));
  if (!(await done("session_edit", { id: dialog.id, input: { title } }))) return;
  closeDialog();
  await loadWorkspace();
  await loadSessions();
  render();
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
  // The other agent by default: a second opinion from a different model.
  // Claude and Codex review each other; Claude reviews Gemini.
  const agent = current.agent === "claude" ? "codex" : "claude";
  openModal({ kind: "review", id: current.id, brief, agent, ...lastChoice(agent), account: activeAccount(agent) });
}

async function createReview() {
  captureDraft();
  const dialog = state.dialog;
  const id = await call("review_create", {
    id: dialog.id,
    prompt: dialog.brief,
    agent: dialog.agent,
    model: dialog.model === "__loading" ? "" : dialog.model ?? "",
    effort: effortLevels(dialog.agent, dialog.account ?? "", dialog.model ?? "").includes(dialog.effort) ? dialog.effort : null,
    profileId: dialog.account || null,
  });
  if (!id) return;
  rememberChoice(dialog.agent, dialog.model, dialog.effort);
  closeDialog();
  await loadSessions();
  openSession(id);
}

async function insertFeedback() {
  captureDraft();
  const text = value("draft-feedback") || state.dialog.feedback || "";
  const builder = await call("review_builder", { id: state.sessionId });
  if (!builder) return;
  if (viewOf(anySession(builder)) === "chat") {
    closeDialog();
    insertDraft(builder, text, { replace: true });
    return toast(t("Feedback is in the builder's message box; send it from there"));
  }
  if (await terminal.paste(builder, text, false)) {
    closeDialog();
    toast(t("Feedback inserted; press Enter in the builder to send it"));
  }
}

async function openSavedOutput() {
  const current = session();
  state.menu = null;
  const text = await call("session_output", { id: current.id });
  if (text === undefined) return;
  openModal({ kind: "output", title: current.title, text: text || t("Nothing saved yet.") });
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
    toast(t("Worktree created on {branch}", { branch: plan.branch }));
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
  if (state.settingsSection === "Usage prices") usage.loadPrices();
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
  if (!report) return toast(t("There is no macOS app workspace on this Mac."));
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
  toast(t("Imported {projects} and {sessions}", { projects: plural(report.projects, "project"), sessions: plural(report.sessions, "session") }));
  if (report.warnings.length) openModal({ kind: "macos-import-done", warnings: report.warnings });
}

async function runDiagnostics() {
  state.diagnostics = null;
  state.setupChecks = null;
  render();
  await loadSetup();
  render();
}

/// The tools, then everything else the macOS Setup check looks at, for the
/// accounts new sessions start with.
async function loadSetup() {
  const [tools, checks] = await Promise.all([
    call("diagnostics_run"),
    call("setup_checks", { claudeProfile: activeAccount("claude") || null, codexProfile: activeAccount("codex") || null }),
  ]);
  state.diagnostics = tools ?? [];
  state.setupChecks = checks ?? [];
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
  if (!label) return toast(t("Give the account a label."));
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
        return toast(t("Path copied"));
      case "reconnect":
        render();
        return reconnectProject();
      case "refresh":
        return refreshProjects(target.id);
      case "move-up":
        return moveProject(target.id, -1);
      case "move-down":
        return moveProject(target.id, 1);
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
    case "open-task": {
      const owned = taskOfSession(target.id);
      return owned ? openTask(owned.taskId) : render();
    }
    case "pin":
      if (await done("session_pin", { id: target.id, pinned: !target.pinned })) await loadSessions();
      return render();
    case "start":
      return startSession(target.id);
    case "sleep":
      await done("session_hibernate", { id: target.id });
      return render();
    case "stop":
      return openModal({
        kind: "confirm",
        title: t("Stop this agent?"),
        body: t("This interrupts current work. The saved conversation stays with the agent."),
        confirmLabel: t("Stop"),
        danger: true,
        run: () => terminal.stop(target.id),
      });
    case "rename":
      return openRename(target.id);
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
    case "delete":
      return confirmDelete([`s:${target.id}`]);
    case "remove":
      return confirmRemove([target.id]);
    case "reveal-worktree":
      render();
      return call("path_reveal", { path: target.working_directory });
    case "copy-worktree":
      await navigator.clipboard.writeText(target.working_directory);
      render();
      return toast(t("Path copied"));
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
  toast(t("Imported"));
}

async function addQuickCommand() {
  captureDraft();
  const dialog = state.dialog;
  const saved = await done("quick_command_save", {
    id: dialog.editing ?? "",
    title: dialog.title,
    text: dialog.text,
    submit: dialog.submit,
    projectId: dialog.scoped ? state.projectId : null,
  });
  if (!saved) return;
  await loadSessions();
  Object.assign(state.dialog, { title: "", text: "", submit: false, scoped: true, editing: null });
  render();
}

/// Reads the feed; re-renders only when something new arrived, since this
/// runs every few seconds for the bell's badge.
async function loadActivity() {
  const events = (await attempt("activity_read")).value ?? state.activity ?? [];
  const changed = events.length !== (state.activity?.length ?? -1) || events[0]?.id !== state.activity?.[0]?.id;
  state.activity = events;
  noteSleep(events);
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

/// The approvals inbox. The keyboard goes into it, so Tab reaches its rows.
function openApprovals() {
  state.dialog = null;
  state.menu = { kind: "approvals" };
  render();
  document.querySelector(".inbox")?.focus();
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
  const taskDocs = (await call("task_docs_list", { projectId: id })) ?? [];
  const previous = state.docs?.projectId === id ? state.docs : null;
  state.docs = {
    projectId: id,
    files,
    taskDocs,
    selected: previous?.selected && files.includes(previous.selected) ? previous.selected : null,
    text: "",
    editing: false,
    dirty: false,
  };
  if (state.docs.selected) await openDoc(state.docs.selected);
  else render();
}

/// A task document: Markdown rendered as the other docs are, a PDF in the
/// web view's own viewer.
let pdfUrl = null;
async function openTaskDoc(path) {
  if (pdfUrl) URL.revokeObjectURL(pdfUrl);
  pdfUrl = null;
  state.docs = { ...state.docs, selected: null, taskDoc: path, text: "", pdf: null, editing: false, dirty: false, loading: true };
  render();
  const content = await call("task_doc_read", { projectId: state.docs.projectId, path });
  if (content?.kind === "pdf") {
    const bytes = Uint8Array.from(atob(content.base64), (char) => char.charCodeAt(0));
    pdfUrl = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  }
  state.docs = { ...state.docs, text: content?.kind === "markdown" ? content.text : "", pdf: pdfUrl, loading: false };
  render();
}

async function openDoc(path) {
  // As on macOS, opening another file leaves unsaved edits behind.
  state.docs = { ...state.docs, selected: path, taskDoc: null, pdf: null, text: "", editing: false, dirty: false, loading: true };
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
  toast(t("Saved"));
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
  await startSession(id);
}

const docTitle = () => {
  const heading = (state.docs?.text ?? "").split("\n").find((line) => line.startsWith("# "));
  return heading ? heading.slice(2).trim() : state.docs?.selected ?? "";
};

Object.assign(ACTIONS, {
  activity: () => (state.menu?.kind === "activity" ? closeDialog() : openActivity()),
  approvals: () => (state.menu?.kind === "approvals" ? closeDialog() : openApprovals()),
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
    toast(t("Path copied"));
  },
  "doc-new-spec": () => openModal({ kind: "doc-spec", title: "" }),
  "doc-create-spec": async () => {
    captureDraft();
    const title = state.dialog.title?.trim();
    if (!title) return toast(t("Give the spec a title."));
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
    if (prompts) await startDocSession(t("Write project doc"), prompts.project_doc);
  },
  "doc-draft-spec": async () => {
    const title = docTitle();
    const prompts = await call("doc_prompts", { path: state.docs.selected, title });
    if (prompts) await startDocSession(t("Spec: {title}", { title }), prompts.spec);
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
      ...newTask(),
      spec_id: "",
    });
  },
});

/// What a new task's form starts with: the project's defaults, then this
/// machine's last model and account for its agent.
function newTask() {
  const defaults = newTaskDefaults(project(), state.settings);
  return { ...defaults, ...lastChoice(defaults.agent), profile_id: activeAccount(defaults.agent) };
}

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
    return openModal({ kind: "markdown", title: t("Specification"), text });
  }
  const { writeTextFile } = await import("@tauri-apps/plugin-fs");
  try {
    await writeTextFile(path, text);
    toast(t("Exported to {path}", { path }));
  } catch (error) {
    toast(error, "error");
  }
}

/// A click on a task opens its page: where it stands and each step, with
/// the sessions that ran them. Its form is Edit… there.
async function openTask(id) {
  const task = findTask(id);
  if (!task) return;
  if (task.project_id !== state.projectId) await showProject(task.project_id);
  await showTab("tasks");
  state.taskId = id;
  render();
}

/// The open task's form.
const editTask = (task) => task && openModal({ kind: "task", ...task, skills: task.skills ?? [], skillsTouched: true });

/// The task a step's button acts on: the open form's, or the page's.
const panelTaskId = () => (state.dialog?.kind === "task" ? state.dialog.id : null) ?? state.taskId;

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
      skills: Array.isArray(dialog.skills) ? dialog.skills : null,
    },
  });
  if (!id) return;
  const wanted = dialog.depends_on ?? [];
  const had = findTask(id)?.depends_on ?? [];
  if (wanted.length !== had.length || wanted.some((other) => !had.includes(other))) {
    if (!(await done("task_dependencies", { id, dependsOn: wanted }))) return;
  }
  const related = dialog.related ?? [];
  const linked = findTask(id)?.related ?? [];
  if (related.length !== linked.length || related.some((other) => !linked.includes(other))) {
    if (!(await done("task_related", { id, related }))) return;
    // The other side of each link changed too.
    if (state.allTasks) await loadAllTasks();
  }
  const model = (dialog.model ?? "").trim();
  const effort = effortLevels(dialog.agent, dialog.profile_id ?? "", model).includes(dialog.effort) ? dialog.effort : "";
  const before = findTask(id);
  if (
    model !== (before?.model ?? "") ||
    effort !== (before?.effort ?? "") ||
    (before && before.agent !== dialog.agent)
  ) {
    if (!(await done("task_agent", { id, agent: dialog.agent, model, effort: effort || null }))) return;
  }
  rememberChoice(dialog.agent, model, effort);
  const account = dialog.profile_id || null;
  if ((account ?? "") !== (before?.profile_id ?? "")) {
    if (!(await done("task_account", { id, profileId: account }))) return;
  }
  const repositories = dialog.extra_projects ?? [];
  const hadRepositories = before?.extra_projects ?? [];
  if (repositories.length !== hadRepositories.length || repositories.some((other) => !hadRepositories.includes(other))) {
    if (!(await done("task_repositories", { id, projects: repositories }))) return;
  }
  const planFirst = !!dialog.plan_first;
  const planAuto = dialog.plan_auto_approve ?? null;
  if (planFirst !== !!before?.plan_first || planAuto !== (before?.plan_auto_approve ?? null)) {
    if (!(await done("task_plan", { id, planFirst, autoApprove: planAuto }))) return;
  }
  closeDialog();
  await reloadTasks();
  if (run) await runTask(id);
}

const findTask = (id) =>
  state.planning?.tasks.find((item) => item.id === id) ?? state.allTasks?.find((item) => item.id === id);

async function loadAllTasks() {
  state.allTasks = (await call("tasks_all")) ?? [];
}

/// Reloads whichever task lists are on screen.
async function reloadTasks() {
  await loadPlanning();
  // Every project's tasks fold their sessions in the sidebar, so the list
  // stays current once loaded.
  if (state.taskScope === "all" || state.allTasks) await loadAllTasks();
  render();
}

/// Prepares the task's session and starts it, as Run does on macOS.
async function runTask(id) {
  const sessionId = await call("task_prepare", { id, profileId: null });
  if (!sessionId) return;
  await loadWorkspace();
  await reloadTasks();
  await openAnywhere(sessionId);
  await startSession(sessionId);
  await reloadTasks();
}

/// A task dropped on a phase, or moved from its menu: onto Working it runs,
/// onto To do, Ready or Done it takes that phase's status, and "changes"
/// sends it back to its builder. Nothing moves a task something works on.
async function moveTask(id, target) {
  const task = findTask(id);
  if (!task) return;
  const now = phaseOf(task);
  if (now.phase === target) return;
  if (!movable(now)) return toast(t("Something is working on this task. Stop it first."));
  if (target === "working") return runnable(task) ? runTask(id) : toast(t("Only a task waiting to start, sent back or stopped can run."));
  if (target === "needs_you") return toast(t("A task lands here by itself when it needs you."));
  const status = target === "changes" ? "changes" : dropStatus(task, target);
  if (!status || status === task.status) return;
  if (await done("task_status", { id, status })) {
    await reloadTasks();
    // A finished task may be what others in the queue waited for.
    if (status === "done" && state.queues.has(task.project_id)) await advanceQueue(task.project_id);
  }
}

// --------------------------------------------------------------------- QA --

/// Tests a task: the project's checks, then the review when it is due and
/// the QA agent. The run shows itself on the task as it goes; what follows
/// the checks is the pipeline's to say.
async function startQa(id) {
  const title = findTask(id)?.title ?? "";
  toast(t("Testing “{task}”…", { task: title }));
  const actions = await call("qa_start", { taskId: id });
  await reloadTasks();
  if (actions) await applyPipeline(actions);
  refreshTaskDialog(id);
  render();
}

// --------------------------------------------------------------- pipeline --

/// What the pipeline tells the person, by event.
function pipelineNotice(action) {
  const task = findTask(action.task)?.title ?? "";
  switch (action.event) {
    case "ready":
      return toast(t("“{task}” passed everything that runs by itself. It is ready for you to mark done.", { task }));
    case "fixing":
      return toast(t("{stage} found problems with “{task}”. They went back to its builder.", { stage: t(action.detail), task }));
    case "attention":
      return toast(t("“{task}” needs your attention: {reason}", { task, reason: action.detail.split("\n")[0] }), "bad");
    case "plan_ready":
      return toast(t("The plan for “{task}” is ready for your approval.", { task }));
    case "plan_approved":
      return toast(t("The plan for “{task}” approved itself. Building it now.", { task }));
    case "docs_written":
      return toast(t("The document for “{task}” is written.", { task }));
    case "qa_queued":
      return toast(t("“{task}” waits for its QA environment: another task is testing in it.", { task }));
    case "published":
      return toast(action.detail ? t("“{task}” is published: {url}", { task, url: action.detail }) : t("“{task}” is pushed.", { task }));
  }
}

/// Does what the pipeline asks, in order: start or resume a session, send
/// one a message, test or run a task, put a finished stage to sleep, carry
/// a queue on, tell the person.
async function applyPipeline(actions) {
  if (!actions?.length) return;
  let loaded = false;
  for (const action of actions) {
    switch (action.kind) {
      case "start":
        if (!loaded) {
          await loadSessions();
          loaded = true;
        }
        await startSession(action.session, { background: true });
        break;
      case "send":
        if (viewOf(anySession(action.session)) === "chat") await sendText(action.session, action.text);
        else await terminal.paste(action.session, action.text, true);
        break;
      case "test":
        // Not awaited: the checks can take minutes.
        startQa(action.task);
        break;
      case "run":
        await runTask(action.task);
        break;
      case "hibernate":
        await terminal.hibernate(action.session);
        break;
      case "publish":
        // Not awaited: pushing and opening the request take a while.
        publishTask(action.task);
        break;
      case "built":
        if (state.projects.find((item) => item.id === action.project)?.auto_run_tasks) state.queues.add(action.project);
        if (state.queues.has(action.project)) await advanceQueue(action.project);
        break;
      case "notice":
        pipelineNotice(action);
        break;
    }
  }
  await reloadTasks();
  for (const task of new Set(actions.map((action) => action.task).filter(Boolean))) refreshTaskDialog(task);
  render();
}

/// Every two seconds: stages that reported by file are moved on.
let pipelineBusy = false;
async function pipelineTick() {
  if (pipelineBusy) return;
  pipelineBusy = true;
  try {
    const actions = await attempt("pipeline_tick");
    if (actions.ok && actions.value?.length) await applyPipeline(actions.value);
  } finally {
    pipelineBusy = false;
  }
}

/// Changes the project's scheduled import; a change searches again at once.
function saveIntake(patch) {
  const link = state.projectDraft?.link;
  if (!link) return;
  const intake = { ...(link.intake ?? {}), ...patch, last_run: null };
  return saveProjectLink({ ...link, intake });
}

/// Every minute: linked projects whose scheduled search is due are searched,
/// and what is new is imported — and run, where the project asks.
let intakeBusy = false;
async function intakeTick() {
  if (intakeBusy) return;
  intakeBusy = true;
  try {
    const results = await attempt("intake_tick");
    if (!results.ok || !results.value?.length) return;
    let changed = false;
    for (const result of results.value) {
      const name = state.projects.find((item) => item.id === result.project_id)?.title ?? "";
      if (!result.created.length) continue;
      changed = true;
      toast(t("Imported {tasks} into {project}.", { tasks: plural(result.created.length, "task"), project: name }));
      if (result.run) state.queues.add(result.project_id);
    }
    if (!changed) return;
    await reloadTasks();
    for (const result of results.value) if (result.run && result.created.length) await advanceQueue(result.project_id);
  } finally {
    intakeBusy = false;
  }
}

/// Pushes a task's branch and opens its request.
async function publishTask(id) {
  toast(t("Publishing “{task}”…", { task: findTask(id)?.title ?? "" }));
  const actions = await call("task_publish", { taskId: id });
  if (actions) await applyPipeline(actions);
  else await reloadTasks();
}

/// Approves a plan and builds the task with it.
async function approvePlan(id) {
  if (!id || !(await done("plan_approve", { taskId: id }))) return;
  closeDialog();
  await reloadTasks();
  await runTask(id);
}

/// Writes the task's document now.
async function writeTaskDoc(id) {
  if (!id) return;
  const actions = await call("task_docs_start", { taskId: id });
  if (actions) await applyPipeline(actions);
}

/// A task form left open follows its task's QA run.
function refreshTaskDialog(id) {
  const task = findTask(id);
  if (state.dialog?.kind === "task" && state.dialog.id === id && task) {
    Object.assign(state.dialog, {
      qa: task.qa,
      status: task.status,
      findings: task.findings,
      sync: task.sync,
      review: task.review,
      review_session_id: task.review_session_id,
      round: task.round,
      build_summary: task.build_summary,
      plan: task.plan,
      docs: task.docs,
      docs_run: task.docs_run,
      publish_run: task.publish_run,
      pr_title: task.pr_title,
      pr_body: task.pr_body,
      pr_url: task.pr_url,
      extra_pr_urls: task.extra_pr_urls,
      extra_checkouts: task.extra_checkouts,
      attachments: task.attachments,
      last_error: task.last_error,
    });
  }
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
    case "task-qa":
      render();
      return startQa(id);
    case "task-plan":
      return openTask(id);
    case "task-planner":
      return openAnywhere(task.plan?.session_id);
    case "task-review-now": {
      render();
      const actions = await call("review_now", { taskId: id });
      if (actions) toast(t("The other agent is reviewing “{task}”.", { task: task.title }));
      return actions && applyPipeline(actions);
    }
    case "task-skip-review": {
      render();
      const actions = await call("review_skip", { taskId: id });
      return actions && applyPipeline(actions);
    }
    case "task-docs":
      render();
      return writeTaskDoc(id);
    case "task-sync-retry":
      render();
      if (await done("task_sync_retry", { id })) await reloadTasks();
      return render();
    case "task-edit":
      return editTask(task);
    case "task-delete":
      return openModal({
        kind: "confirm",
        title: t('Delete "{name}"?', { name: task.title }),
        body: t("The task goes; its session and anything the agent changed stay."),
        confirmLabel: t("Delete"),
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
  if (!summary?.length) return toast(t("No queued tasks."));
  openModal({ kind: "queue", summary });
}

/// Starts the next queued task. The rules are the core's; this supplies the
/// clock and the starting.
/// Starts what the queue may start now: the core picks queued tasks that
/// wait for nothing unfinished, as many as the project's free slots. With
/// nothing left queued the queue stops; with tasks only waiting, it waits.
async function advanceQueue(projectId) {
  if (!state.queues.has(projectId)) return;
  const next = (await call("queue_next", { projectId })) ?? [];
  if (!next.length) {
    await loadPlanning();
    const waiting = (state.planning?.tasks ?? []).some((task) => task.project_id === projectId && task.status === "queued");
    const busy = (state.planning?.tasks ?? []).some((task) => task.project_id === projectId && ["building", "planning"].includes(task.status));
    if (!waiting || !busy) state.queues.delete(projectId);
    return render();
  }
  for (const id of next) {
    const sessionId = await call("task_prepare", { id, profileId: null });
    if (!sessionId) {
      state.queues.delete(projectId);
      await reloadTasks();
      return render();
    }
    await loadSessions();
    await startSession(sessionId, { background: next.length > 1 });
  }
  await reloadTasks();
}

// ---------------------------------------------------------------- files ---

async function fileAct(reveal) {
  const path = selectedPath();
  if (!path) return toast(t("Select a file first."));
  await done("files_open", { id: state.files.id, path, reveal });
}

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
  openModal({ kind: "confirm", title, body, confirmLabel: t("Continue"), danger: true, run });
}

async function mutate(mutation) {
  if (!state.files) return;
  if (mutation.path === "" && "path" in mutation) return toast(t("Select a file first."));
  if (mutation.commit === "" && "commit" in mutation) return toast(t("Select a commit first."));
  state.menu = null;
  const output = await call("files_mutate", { id: state.files.id, mutation });
  if (output === undefined) return false;
  if (output.trim()) toast(output.trim().slice(0, 160));
  await loadFiles();
  return true;
}

async function trashFile() {
  const path = selectedPath();
  if (!path) return toast(t("Select a file first."));
  confirmThen(t("Move untracked file to Trash?"), path, async () => {
    if (await done("files_trash", { id: state.files.id, path })) await loadFiles();
  });
}

async function openHunkChoice() {
  const path = selectedPath();
  if (!path || state.files.selection.kind !== "unstaged") {
    return toast(t("Hunks can only be discarded from unstaged changes."));
  }
  const hunks = await call("files_hunks", { id: state.files.id, path });
  if (!hunks?.length) return toast(t("This hunk cannot be discarded separately."));
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

/// Commits what is staged. The message is kept when the commit fails, so it
/// is not lost to a hook or an empty index.
async function commit(amend) {
  const message = document.querySelector("#commit-message")?.value ?? state.files.message;
  state.files.message = message;
  const made = await mutate({ action: "commit", message, amend });
  if (made) state.files.message = "";
  render();
  return made;
}

async function generateMessage() {
  const existing = document.querySelector("#commit-message")?.value ?? "";
  const run = async () => {
    toast(t("Asking Claude for a commit message…"));
    const message = await call("commit_generate", { id: state.files.id });
    if (message === undefined) return;
    state.files.message = message;
    render();
  };
  if (existing.trim()) {
    return confirmThen(
      t("Replace the message you have written?"),
      t("Generating asks Claude for a new message for the staged diff."),
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
  !!state.sessionId && !state.files && state.page !== "settings" && state.page !== "project" && !state.dialog &&
  viewOf(session()) === "terminal";

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
    const now = sessionState(item);
    entries.push({
      kind: "Session",
      title: item.title,
      subtitle: [
        owner?.title,
        agentName(item.agent),
        item.branch ? `⎇ ${item.branch}` : "",
        item.review_of ? t("review") : "",
        now === "not_started" ? "" : t(sessionLabel(now)),
      ]
        .filter(Boolean)
        .join(" · "),
      running: isRunning(item.id),
      needsYou: now === "needs_you",
      run: () => openAnywhere(item.id),
    });
  }
  for (const item of state.projects) {
    entries.push({ kind: "Project", title: item.title, subtitle: item.path, run: () => showProject(item.id) });
  }
  const bound = (action) => label(binding(action));
  const inFiles = (then) => async () => {
    if (!state.files) await openFiles();
    await then();
  };
  const actions = [
    ["Home", "Every project at once", "home"],
    ["Agent Dashboard", "Every session by what its agent is doing", "dashboard"],
    ["Usage", "Tokens and estimated cost by day, model and project", "usagePage"],
    ["New session", "Starts an agent session", "newSession"],
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
    ["Import tasks", "From an integration into the current project", "importIssues"],
    ["Show project folder", "", "reveal"],
    ["Copy project path", "", "copyPath"],
    ["Toggle keep awake", "", "wake"],
    ["Activity", "What the agents did, newest first", "activity"],
    ["Needs you…", "What the agents are waiting on you for, in every project", "approvals"],
    ["Settings…", "Appearance, terminal, agents, git, notifications", "settings"],
    ["Files & Changes", "Browse files, stage, commit, push, log", "files"],
  ].map(([title, subtitle, action]) => ({ kind: "Action", title: t(title), subtitle: t(subtitle), shortcut: bound(action), run: () => SHORTCUT_ACTIONS[action]() }));
  const more = [
    ["Theme: System", "Follow the system appearance", () => savePref({ theme: "system" })],
    ["Theme: Light", "", () => savePref({ theme: "light" })],
    ["Theme: Dark", "", () => savePref({ theme: "dark" })],
    ["Setup check", "Find missing CLIs and configuration problems", () => openSettings("Setup")],
    ["Accounts", "Sign-ins for Claude Code and Codex", openAccounts],
    ["Project settings", "", openProjectSettings],
    ["Import provider history", "Conversations the agent already has for this folder", openTranscripts],
    ["Import all history…", "Every Claude Code and Codex conversation on this computer", openHistorySweep],
    ["Skills…", "Instructions you attach to sessions, tasks and automations", () => openSettings("Skills")],
    ["New skill…", "Write instructions once and attach them to sessions", () => skills.open(null)],
    ["New automation…", "Run a prompt on a schedule in the current project", () => withProject(() => automations.open(null))],
    ["Automations", "The current project's scheduled prompts", () => withProject(() => automations.openTab())],
    ["Remove all projects…", "Clear the project list; nothing on disk is touched", ACTIONS["remove-all-projects"]],
    ["Git: Commit…", "The commit composer in Files & Changes", inFiles(() => showFilesView("changes"))],
    ["Git: Push", "", inFiles(() => ACTIONS.push())],
    ["Git: Pull (fast-forward)", "", inFiles(() => ACTIONS.pull())],
    ["Git: Fetch", "", inFiles(() => ACTIONS.fetch())],
    ["Git: Log", "Recent commits with revert and reset", inFiles(() => showFilesView("log"))],
    ["Git: Files", "Browse the project tree", inFiles(() => showFilesView("files"))],
    ["Git: Branches", "", inFiles(() => showFilesView("branches"))],
  ].map(([title, subtitle, run]) => ({ kind: "Action", title: t(title), subtitle: t(subtitle), run }));
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
      subtitle: [owner?.title, t(sessionLabel(sessionState(item))), id === current ? t("current") : ""]
        .filter(Boolean)
        .join(" · "),
      shortcut: index < 9 ? label(`mod+${index + 1}`) : "",
      run: () => openAnywhere(id),
    };
  });
}

/// ⌘/: this project's and the global quick commands, sent to the open session.
function quickEntries() {
  const id = state.sessionId;
  if (!id) return [];
  return state.quickCommands.map((command) => ({
    kind: "Action",
    title: command.title,
    subtitle: command.text.slice(0, 90).replace(/\n/g, " "),
    shortcut: command.submit ? t("↩ sends") : command.project_id ? "" : t("global"),
    run: () => (viewOf(anySession(id)) === "chat" ? quickInChat(id, command.id) : done("quick_command_send", { sessionId: id, commandId: command.id })),
  }));
}

function paletteResults(query, mode = "all") {
  const needle = query.trim();
  if (mode === "quick" || mode === "search") {
    const items = mode === "quick" ? quickEntries() : paletteEntries().filter((item) => item.kind !== "Action");
    if (!needle) return items.slice(0, 40);
    return items
      .map((item) => [item, paletteScore(needle, item.title, item.subtitle)])
      .filter(([, score]) => score > 0)
      .sort((a, b) => b[1] - a[1])
      .map(([item]) => item);
  }
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

// ------------------------------------------------------------ starting ---

/// Every start goes through here: a session shown as a chat starts its agent
/// in chat mode, any other in a terminal.
async function startSession(id, { background = false } = {}) {
  if (viewOf(anySession(id)) !== "chat") return terminal.start(id, { background });
  if (!background) {
    state.sessionId = id;
    render();
  }
  const started = await done("chat_start", { id });
  await loadWorkspace();
  return started;
}

/// Waits for a stopped agent's process to be gone, so the other mode can
/// start it again.
async function waitStopped(id) {
  await loadWorkspace();
  for (let tries = 0; tries < 40 && isRunning(id); tries += 1) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    await loadWorkspace();
  }
  return !isRunning(id);
}

/// Chat ⇄ Terminal. A running agent is stopped and resumed in the other one.
async function switchView(id, view, restart) {
  if (restart) {
    if (!(await done("session_stop", { id }))) return;
    if (!(await waitStopped(id))) return toast(t("The agent did not stop. Try again in a moment."), "error");
  }
  if (!(await done("session_view", { id, view }))) return;
  for (const list of [state.sessions, state.allSessions]) {
    const item = list.find((entry) => entry.id === id);
    if (item) item.view = view;
  }
  render();
  if (restart) await startSession(id, { background: state.sessionId !== id });
  else await loadSessions();
}

/// A quick command in a chat: sent when it submits, typed in otherwise.
function quickInChat(id, commandId) {
  const command = state.quickCommands.find((item) => item.id === commandId);
  if (!command) return;
  if (command.submit) return sendText(id, command.text);
  insertDraft(id, command.text);
}

/// A chat event feeds the conversation and, for the tabs, badge and
/// notifications, the agent's state — as the hook monitor does for terminals.
function onChatEvent(payload) {
  const chat = chatEvent(payload);
  const { id, event } = payload;
  const before = state.agentState.get(id);
  let next = before;
  if (event.type === "turn_started") next = waitingFor(id) ? "waiting" : "working";
  else if (event.type === "approval") next = "waiting";
  else if (event.type === "approval_resolved") next = waitingFor(id) ? "waiting" : chat.turn ? "working" : before;
  else if (event.type === "turn_ended") next = "done";
  if (event.type === "turn_ended") chatSnapshot(id);
  if (event.type === "session") loadSessions();
  // A new approval asks even when the state already says waiting.
  const asks = event.type === "approval" || next !== before;
  if (!asks || !next) {
    // An answer or a turn's end changes the inbox even when the state
    // stays — another approval of the same session still waits.
    if (["approval_resolved", "turn_ended"].includes(event.type)) render();
    return;
  }
  state.agentState.set(id, next);
  if (next === "waiting" || next === "done") notify({ id, state: next, title: anySession(id)?.title ?? "" });
  render();
  updateBadge();
}

async function adoptWaiting() {
  const listed = await attempt("chat_approvals");
  if (!listed.ok) return;
  for (const id of adoptApprovals(listed.value)) state.agentState.set(id, "waiting");
  render();
  updateBadge();
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
    // A chat session's state comes from its own events.
    if (viewOf(anySession(entry.id)) === "chat") continue;
    if (state.agentState.get(entry.id) !== entry.state) {
      state.agentState.set(entry.id, entry.state);
      if (entry.state === "waiting") state.waitingSince.set(entry.id, Date.now());
      else state.waitingSince.delete(entry.id);
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
    title: entry.state === "done" ? t("Agent finished a turn") : t("Agent needs your attention"),
    body: entry.title,
    ...(prefs.notification_sound === "none" ? {} : { sound: prefs.notification_sound || "default" }),
  });
  notified = { id: entry.id, state: entry.state };
}

/// An automation's run ended. In front of the window it is a toast; behind
/// it, a desktop notification when notifications are on.
async function notifyAutomation(run) {
  const finished = run?.status === "done";
  const title = finished
    ? t("Automation finished: {title}", { title: run.title })
    : t("Automation failed: {title}", { title: run.title });
  const prefs = state.settings;
  if (document.hasFocus() && !prefs.notify_when_focused) {
    toast(title, finished ? "info" : "error");
    return;
  }
  if (!prefs.notifications || prefs.notify_done === false) return;
  const { isPermissionGranted, requestPermission, sendNotification } = await import(
    "@tauri-apps/plugin-notification"
  );
  let granted = await isPermissionGranted();
  if (!granted) granted = (await requestPermission()) === "granted";
  if (!granted) return;
  const project = state.projects.find((item) => item.id === run.project_id)?.title ?? "";
  sendNotification({
    title,
    body: [project, run.note ? t(run.note) : ""].filter(Boolean).join(" · "),
    ...(prefs.notification_sound === "none" ? {} : { sound: prefs.notification_sound || "default" }),
  });
  if (run.session_id) notified = { id: run.session_id, state: "done" };
}

/// The session the last notification was about. Desktop notifications give
/// no click callback, but clicking one brings the window forward: coming back
/// while that session still waits opens it, as clicking does on macOS.
let notified = null;
addEventListener("focus", () => {
  const last = notified;
  notified = null;
  if (!last) return;
  // Several things waiting: the inbox, where all of them are. One: its session.
  if (last.state === "waiting" && inboxItems().length > 1) {
    if (state.menu?.kind !== "approvals") openApprovals();
    return;
  }
  if (isRunning(last.id) && state.agentState.get(last.id) === last.state && state.sessionId !== last.id) {
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

/// Asks for the machine to stay awake when the setting wants it, and keeps
/// what the system answered: the status bar shows Awake only when it is.
async function applyKeepAwake() {
  const wanted =
    state.settings.keep_awake === "always" ||
    (state.settings.keep_awake === "sessions" && state.running.length > 0);
  const held = await call("keep_awake", { wanted });
  if (held === undefined) return;
  const failed = wanted && !held;
  if (failed && !state.awakeFailed) toast(t("Could not keep the computer awake."));
  state.awakeFailed = failed;
  if (state.awake !== held) {
    state.awake = held;
    render();
  }
}

terminal.onExit(async (exit) => {
  chatExited(exit.id, exit);
  // The exit was drawn before this ran; a sleep changes the session's word.
  if (exit.cause === "hibernated") {
    state.asleep.add(exit.id);
    render();
  } else state.asleep.delete(exit.id);
  // The conversation now shown in its place has more in it than was read.
  if (terminal.ranInTerminal(exit.id)) {
    historyChanged(exit.id);
    render();
  }
  state.waitingSince.delete(exit.id);
  updateBadge();
  if (state.menu?.kind === "approvals") render();
  if (exit.id.startsWith("login:")) {
    if (state.dialog?.id === exit.id) {
      state.dialog.finished = true;
      render();
    }
    return;
  }
  await applyKeepAwake();
  // A stage ending — a reviewer, a QA agent, a planner, a documentation
  // writer — cleanly or not: what it reported decides the task, and one
  // that reported nothing needs attention.
  const after = await attempt("pipeline_after_exit", { id: exit.id });
  if (after.ok && after.value?.length) await applyPipeline(after.value);
  const ended = anySession(exit.id);
  if (ended?.qa_of || ended?.review_of) return render();
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
      toast(t("The queue is paused: the last task did not finish cleanly."), "bad");
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
    toast(t("The specification changed while the task ran. It is back in changes."), "bad");
    await loadPlanning();
    return render();
  }
  // The round is done: its "built" action carries a running queue on.
  if (step.kind === "advance") await applyPipeline(step.actions);
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
  // Onto a chat: the paths go into its message box, for the agent to read.
  const chat = document.elementFromPoint(x, y)?.closest("[data-chat]");
  if (chat && paths?.length) {
    insertDraft(chat.dataset.chat, `${paths.map(shellQuote).join(" ")} `);
    return true;
  }
  const id = terminalAt(x, y);
  if (!id || !paths?.length) return false;
  if (!isRunning(id)) {
    toast(t("Start the session to drop files into it."));
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
  // The sidebar folds each task's sessions into one row.
  await loadAllTasks();
  // The status bar's account chips need them.
  await loadProfiles();
  // The window opens on Home, as the macOS app's does.
  if (state.projects.length) openHome();
  await applyKeepAwake();
  setInterval(monitorTick, 2000);
  setInterval(pipelineTick, 2000);
  setInterval(intakeTick, 60000);
  setTimeout(intakeTick, 10000);
  setInterval(applyKeepAwake, 10000);
  refreshLimits(false);
  setInterval(() => refreshLimits(false), 60000);
  scheduleGitPolling();
  loadActivity();
  setInterval(loadActivity, 5000);
  // What each running terminal shows is kept every few seconds.
  setInterval(() => {
    terminal.snapshotRunning();
    for (const id of state.running) if (viewOf(anySession(id)) === "chat") chatSnapshot(id);
  }, 4000);
  await listen("chat:event", ({ payload }) => onChatEvent(payload));
  // Approvals asked before this window listened — a reload, or a session it
  // never opened — are still waiting in the backend.
  await adoptWaiting();
  // How long each has waited, kept current while the inbox is open.
  setInterval(() => state.menu?.kind === "approvals" && render(), 30000);
  // The backend found a Codex id or a pull request in an agent's output.
  // A manager started a helper from its shell: a new session, already running.
  await listen("session:spawned", () => loadWorkspace());
  // An automation ran, was skipped or finished: its list is redrawn, and a
  // finished run is told as a notification.
  await automations.load();
  await listen("automation:changed", () => automations.reload());
  await skills.load();
  await listen("skills:changed", () => skills.reload());
  // Fresher usage numbers, after a scan or a change of prices.
  await listen("usage:updated", () => state.page === "usage" && usage.load());
  await listen("automation:finished", async ({ payload }) => {
    notifyAutomation(payload);
    await loadWorkspace();
    await automations.reload();
  });
  await listen("session:changed", async () => {
    await loadSessions();
    if (state.tab === "tasks") await loadPlanning();
    // A new reviewer or QA agent folds under its task.
    if (state.allTasks) {
      await loadAllTasks();
      render();
    }
  });
  // A QA run moved on: a check ended, or the run started or settled.
  for (const name of ["task:qa", "task:qa-check"]) {
    await listen(name, async ({ payload }) => {
      await reloadTasks();
      refreshTaskDialog(Array.isArray(payload) ? payload[0] : payload);
      render();
    });
  }
  // A push to a tracker finished: the task's sync chip changes.
  await listen("task:sync", async () => {
    if (state.tab === "tasks" || state.taskScope === "all") await reloadTasks();
    render();
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

// Projects are reordered by dragging one onto another, within its group.
let draggedProject = null;
app.addEventListener("dragstart", (event) => {
  const row = event.target.closest?.("[data-project-drag]");
  if (!row) return;
  draggedProject = row.dataset.projectDrag;
  event.dataTransfer.effectAllowed = "move";
  event.dataTransfer.setData("text/plain", draggedProject);
});
app.addEventListener("dragover", (event) => {
  const row = event.target.closest?.("[data-project-drag]");
  if (!row || !draggedProject) return;
  event.preventDefault();
  for (const other of app.querySelectorAll(".project-row--drop")) other.classList.remove("project-row--drop");
  if (row.dataset.projectDrag !== draggedProject) row.classList.add("project-row--drop");
});
app.addEventListener("drop", (event) => {
  const row = event.target.closest?.("[data-project-drag]");
  if (!row || !draggedProject) return;
  event.preventDefault();
  const id = draggedProject;
  draggedProject = null;
  if (row.dataset.projectDrag !== id) placeProject(id, row.dataset.projectDrag);
});
app.addEventListener("dragend", () => {
  draggedProject = null;
  for (const other of app.querySelectorAll(".project-row--drop")) other.classList.remove("project-row--drop");
});

// Tasks move between statuses by dragging a board card onto a column or a
// list row onto a group. Pointer events rather than HTML5 drag and drop:
// WebKit will not drag a <button>, and the webview's own file drop handler
// takes drops before the page sees them.
let taskDrag = null;
const taskDropAt = (x, y) => document.elementFromPoint(x, y)?.closest("[data-task-column]");
function clearTaskDrop() {
  for (const other of app.querySelectorAll(".board__column--drop")) other.classList.remove("board__column--drop");
}
function endTaskDrag() {
  if (!taskDrag) return;
  taskDrag.ghost?.remove();
  taskDrag.item.classList.remove("task-dragging");
  document.body.classList.remove("is-dragging-task");
  clearTaskDrop();
  taskDrag = null;
}
app.addEventListener("pointerdown", (event) => {
  if (event.button !== 0) return;
  const item = event.target.closest?.("[data-task-card]");
  if (!item || event.target.closest(".task-row__actions")) return;
  taskDrag = { id: item.dataset.taskCard, item, x: event.clientX, y: event.clientY, ghost: null };
});
window.addEventListener("pointermove", (event) => {
  if (!taskDrag) return;
  if (!taskDrag.ghost) {
    if (Math.hypot(event.clientX - taskDrag.x, event.clientY - taskDrag.y) < 5) return;
    const box = taskDrag.item.getBoundingClientRect();
    const ghost = taskDrag.item.cloneNode(true);
    ghost.classList.add("task-ghost");
    ghost.removeAttribute("data-task");
    ghost.removeAttribute("data-task-card");
    ghost.style.width = `${box.width}px`;
    document.body.append(ghost);
    Object.assign(taskDrag, { ghost, dx: taskDrag.x - box.left, dy: taskDrag.y - box.top });
    taskDrag.item.classList.add("task-dragging");
    document.body.classList.add("is-dragging-task");
  }
  taskDrag.ghost.style.transform = `translate(${event.clientX - taskDrag.dx}px, ${event.clientY - taskDrag.dy}px)`;
  const column = taskDropAt(event.clientX, event.clientY);
  clearTaskDrop();
  column?.classList.add("board__column--drop");
});
window.addEventListener("pointerup", (event) => {
  if (!taskDrag) return;
  const { id, ghost } = taskDrag;
  const column = ghost && taskDropAt(event.clientX, event.clientY);
  endTaskDrag();
  if (!ghost) return;
  justDropped = true;
  if (column) moveTask(id, column.dataset.taskColumn);
});
// The click that follows a drop must not open the task. Any later press
// clears this, so a drop that ends without a click swallows nothing.
let justDropped = false;
window.addEventListener("pointerdown", () => (justDropped = false), { capture: true });
window.addEventListener(
  "click",
  (event) => {
    if (!justDropped) return;
    justDropped = false;
    event.stopPropagation();
    event.preventDefault();
  },
  { capture: true },
);
window.addEventListener("pointercancel", endTaskDrag);
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && taskDrag?.ghost) endTaskDrag();
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
