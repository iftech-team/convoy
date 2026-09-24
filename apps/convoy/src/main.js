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
import { escape, numberValue, value } from "./ui.js";
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
  status,
  workbench,
} from "./views.js";
import { filesView, remoteMenu } from "./views-files.js";
import { activityView, specsView, tasksView } from "./views-planning.js";

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
  if (state.sessionId) return workbench();
  switch (state.tab) {
    case "reviews":
      return reviewsView();
    case "specs":
      return specsView();
    case "tasks":
      return tasksView();
    case "activity":
      return activityView();
    default:
      return sessionsView();
  }
}

function render() {
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

  app.innerHTML = `
    <div class="shell">
      ${sidebar()}
      <main class="main">${state.files ? "" : header()}${content()}</main>
      ${status()}
    </div>`;

  if (state.menu === "session") app.insertAdjacentHTML("beforeend", sessionMenu());
  if (state.menu === "quick") app.insertAdjacentHTML("beforeend", quickMenu());
  if (state.menu === "remote") app.insertAdjacentHTML("beforeend", remoteMenu());
  if (state.dialog) app.insertAdjacentHTML("beforeend", isImportDialog() ? imports.dialog() : dialogView());

  for (const [key, top] of scrolled) {
    const node = document.querySelector(`[data-scroll="${key}"]`);
    if (node) node.scrollTop = top;
  }

  if (state.sessionId && !state.files) {
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

// -------------------------------------------------------------- selection --

async function selectProject(id) {
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
  if (!current) return;
  const result = await attempt("git_status", { id: current.id });
  state.gitStatus = result.ok
    ? `${result.value.branch} · ${result.value.changed_files} changed`
    : "";
  const label = document.querySelector("#git-status");
  if (label) label.textContent = state.gitStatus;
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
  if (dialog.kind === "settings") {
    dialog.settings.font_size = numberValue("set-font", dialog.settings.font_size);
    dialog.settings.scrollback = numberValue("set-scrollback", dialog.settings.scrollback);
    dialog.settings.hibernate_minutes = numberValue(
      "set-hibernate",
      dialog.settings.hibernate_minutes,
    );
  }
}

// -------------------------------------------------------------- actions ---

const ACTIONS = {
  // projects
  "open-folder": openFolder,
  settings: openSettings,
  accounts: openAccounts,
  "import-history": openTranscripts,
  "project-settings": openProjectSettings,
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
      agent: state.settings.default_agent,
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
  "save-settings": saveSettings,
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
      agent: state.settings.default_agent,
      mode: "none",
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
  next: () => stepSession(1),
  previous: () => stepSession(-1),
  settings: () => openSettings(),
  search: () => document.querySelector("#project-search")?.focus(),
};

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
      + "[data-capture],[data-issue],[data-conn-add],[data-conn-edit],[data-conn-remove],[data-conn-auth],[data-issue-source]",
  );
  if (!target) return;
  const data = target.dataset;
  if (data.issueSource) return done("issue_open", { id: data.issueSource });
  if (data.action === "edit-task-model") {
    await imports.loadTasks();
    return imports.editModel(state.dialog.id);
  }
  if (data.action === "import" || data.action === "integrations") await imports.loadTasks();
  if (isImportDialog() || ["import", "integrations"].includes(data.action)) {
    if (await imports.onClick(target)) return;
  }

  if (data.dismiss) {
    if (event.target.closest(".modal, .menu") && !target.classList.contains("button")) return;
    return closeDialog();
  }
  if (data.action) {
    const handler = ACTIONS[data.action];
    if (handler) return handler();
    return;
  }
  if (data.project) return selectProject(data.project);
  if (data.tab) {
    state.tab = data.tab;
    state.sessionId = null;
    state.files = null;
    render();
    if (data.tab === "specs" || data.tab === "tasks") await loadPlanning();
    if (data.tab === "activity") await loadActivity();
    return;
  }
  if (data.start) return terminal.start(data.start);
  if (data.stop) return terminal.stop(data.stop);
  if (data.open) return openSession(data.open);
  if (data.capture) {
    captureDraft();
    state.dialog.capturing = data.capture;
    return render();
  }
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
    if (state.dialog.kind === "settings") state.dialog.settings[data.set] = data.value;
    else state.dialog[fieldFor(data.set)] = data.value;
    return render();
  }
  if (data.toggle) {
    captureDraft();
    const key = data.toggle;
    if (state.dialog.kind === "settings") {
      state.dialog.settings[key] = !state.dialog.settings[key];
    } else {
      state.dialog[key] = !state.dialog[key];
    }
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
  if (isImportDialog() && imports.onChange(event.target)) return;
  if (event.target.dataset.toggleState) {
    state[event.target.dataset.toggleState] = event.target.checked;
    await loadSessions();
  }
});

addEventListener("keydown", async (event) => {
  if (event.key === "Escape") {
    if (state.menu || state.dialog) return closeDialog();
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
  // The shortcut editor is listening for the next press, and takes it whole
  // rather than letting it also do what it is currently bound to.
  if (state.dialog?.kind === "settings" && state.dialog.capturing) {
    const pressed = capture(event);
    if (pressed || event.key === "Escape") {
      event.preventDefault();
      if (pressed) state.dialog.settings.shortcuts[state.dialog.capturing] = pressed;
      state.dialog.capturing = null;
      return render();
    }
    return;
  }

  const bound = SHORTCUTS.find(([action]) => matches(event, binding(action)));
  if (bound) {
    event.preventDefault();
    return SHORTCUT_ACTIONS[bound[0]]();
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

async function openProjectSettings() {
  const detail = await call("project_detail", { id: state.projectId });
  if (!detail) return;
  openModal({ kind: "project", ...detail });
}

async function saveProject() {
  captureDraft();
  const dialog = state.dialog;
  const saved = await done("project_edit", {
    id: dialog.id,
    input: {
      title: dialog.title,
      group: dialog.group,
      icon: dialog.icon,
      setup_command: dialog.setup_command,
      shared_paths: dialog.shared_paths,
      review_template: dialog.review_template,
    },
  });
  if (!saved) return;
  closeDialog();
  await loadWorkspace();
}
ACTIONS["save-project"] = saveProject;

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

const suggestBranch = (title) =>
  `convoy/${title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "work"}`;

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

async function openSettings() {
  await loadProfiles();
  openModal({
    kind: "settings",
    // `shortcuts` is copied rather than shared: a binding changed in the
    // dialog and then cancelled must leave the live settings alone.
    settings: { ...state.settings, shortcuts: { ...(state.settings.shortcuts ?? {}) } },
    capturing: null,
  });
}

async function saveSettings() {
  captureDraft();
  if (!(await done("settings_save", { input: state.dialog.settings }))) return;
  closeDialog();
  await loadSettings();
  terminal.applySettings();
  await applyKeepAwake();
  render();
  toast("Settings saved");
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

async function loadActivity() {
  state.activity = (await call("activity_read")) ?? [];
  render();
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
    ["Activity", "Action", async () => {
      state.tab = "activity";
      state.sessionId = null;
      render();
      await loadActivity();
    }],
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
  if (!state.settings.notifications || document.hasFocus()) return;
  const { isPermissionGranted, requestPermission, sendNotification } = await import(
    "@tauri-apps/plugin-notification"
  );
  let granted = await isPermissionGranted();
  if (!granted) granted = (await requestPermission()) === "granted";
  if (!granted) return;
  sendNotification({
    title: entry.state === "done" ? "Agent finished a turn" : "Agent needs your attention",
    body: entry.title,
  });
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
} catch (error) {
  fatal(error?.stack ?? String(error));
}
