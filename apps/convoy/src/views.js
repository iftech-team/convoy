// Every screen. Markup built from state; nothing here calls the backend.

import { conversationKey, historySection, lastActive, pickBox, shownConversations, when } from "./views-history.js";
import { agentIcon, appMark, icons, projectIcon, projectTint } from "./icons.js";
import {
  button,
  choice,
  empty,
  escape,
  group,
  plural,
  segmented,
  setting,
  toggle,
} from "./ui.js";
import { language, t } from "./i18n.js";
import { agentName } from "./task-defaults.js";
import { activeAccount, anySession, isProjectTab, isRunning, phaseOf, project, session, sessionState, state, visibleSessions } from "./state.js";
import { sessionLabel, sessionTone, sleptBy } from "./session-state.js";
import { stageIndex } from "./stages.js";
import { runnable } from "./views-planning.js";
import { movable } from "./phase.js";
import { missingConversation } from "./terminal.js";
import { shownAsTranscript, viewOf } from "./views-chat.js";
import { skillsChip, skillsMenu } from "./views-skills.js";
import { approvalsButton, approvalsPanel, inboxItems } from "./views-inbox.js";
import { helperActivity, helpersButton, helpersMenu } from "./views-helpers.js";
import { automationActivity } from "./views-automations.js";

const TABS = [
  ["sessions", "Sessions"],
  ["specs", "Specs"],
  ["tasks", "Tasks"],
  ["docs", "Docs"],
  ["automations", "Automations"],
];

// ---------------------------------------------------------------- sidebar --

/// "Show in Finder" on a Mac, the file manager's own name elsewhere.
export const revealLabel = () =>
  /Mac/.test(navigator.platform)
    ? t("Show in Finder")
    : /Win/.test(navigator.platform)
      ? t("Show in Explorer")
      : t("Show in Files");

/// The dot beside a session, titled with its state.
function stateDot(item, running = item.running) {
  const now = sessionState(item, !!running);
  return `<span class="dot dot--${sessionTone(now)}" title="${escape(t(sessionLabel(now)))}"></span>`;
}

/// A pinned session, from any project, with its project's name.
function sidebarSession(item) {
  const owner = state.projects.find((entry) => entry.id === item.project_id);
  return `
    <button class="side-session" data-open="${escape(item.id)}"
            data-menu-session="${escape(item.id)}"
            aria-current="${item.id === state.sessionId}"
            title="${escape(item.notes || item.title)}">
      ${stateDot(item)}
      <span class="side-session__text">
        <span class="side-session__title">${escape(item.title)}</span>
        ${owner ? `<span class="side-session__detail">${escape(owner.title)}</span>` : ""}
      </span>
      ${
        item.branch
          ? `<span class="chip chip--branch">${icons.branch}${escape(item.branch)}</span>`
          : `${agentIcon(item.agent, 11)}${item.review_of ? `<span class="side-session__tag">${escape(t("review"))}</span>` : ""}`
      }
    </button>`;
}

/// A project's sessions, for its row's dot: the selected project's come
/// from its own, fresher list.
function sessionsOf(id) {
  const source = id === state.projectId ? state.sessions : state.allSessions;
  return source.filter((entry) => entry.project_id === id && !entry.archived);
}

/// The second line of a detailed project row: its branch and changes, or
/// how many sessions it has when it is not a repository.
function projectDetail(item) {
  const git = state.settings.git_status === false ? null : state.projectGit.get(item.id);
  if (git) return `⎇ ${git.branch} · ${git.changed_files ? t("{count} changed", { count: git.changed_files }) : t("clean")}`;
  return plural(item.sessions, "session");
}

/// Every task the window has loaded, this project's freshest.
function knownTasks() {
  const tasks = new Map();
  for (const task of [...(state.allTasks ?? []), ...(state.planning?.tasks ?? [])]) tasks.set(task.id, task);
  return tasks;
}

/// The task a session belongs to, if any.
export const taskOfSession = (id) => {
  const stage = stageIndex([...knownTasks().values()], [...state.allSessions, ...state.sessions]).get(id);
  return stage ? { ...stage, task: knownTasks().get(stage.taskId) } : null;
};

export function sidebar() {
  const needle = state.search.toLowerCase();
  const groups = new Map();
  const loose = [];
  // As on macOS, a project found inside a folder sits under that folder's
  // row; a search lists every match on its own.
  const known = new Set(state.projects.map((item) => item.id));
  const nested = (item) => !needle && item.parent_id && known.has(item.parent_id) && item.parent_id !== item.id;
  const children = new Map();
  for (const item of state.projects) {
    if (!nested(item)) continue;
    if (!children.has(item.parent_id)) children.set(item.parent_id, []);
    children.get(item.parent_id).push(item);
  }
  for (const item of state.projects) {
    if (needle && !`${item.title} ${item.path}`.toLowerCase().includes(needle)) {
      continue;
    }
    if (nested(item)) continue;
    if (item.group) {
      if (!groups.has(item.group)) groups.set(item.group, []);
      groups.get(item.group).push(item);
    } else {
      loose.push(item);
    }
  }

  if (state.settings.sort_projects) {
    const byName = (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
    loose.sort(byName);
    for (const items of groups.values()) items.sort(byName);
    for (const items of children.values()) items.sort(byName);
  }

  const row = (item, depth = 0) => {
    const current = item.id === state.projectId;
    // A project is one row: its sessions are on its Sessions page, in the
    // tabs and on Home. A folder still folds away the projects inside it.
    const sessions = sessionsOf(item.id);
    const kids = depth < 12 ? (children.get(item.id) ?? []) : [];
    const foldable = kids.length > 0;
    const expanded = foldable && !state.collapsed.has(item.id) && !needle;
    const waiting = sessions.some((entry) => entry.running && state.agentState.get(entry.id) === "waiting");
    return `
      <div class="project-row${current ? " project-row--current" : ""}"
           data-menu-project="${escape(item.id)}"
           ${state.settings.sort_projects ? "" : `draggable="true" data-project-drag="${escape(item.id)}"`}>
        <button class="project-row__chevron" data-expand="${escape(item.id)}"
                title="${escape(expanded ? t("Collapse") : t("Expand"))}"
                ${foldable ? "" : 'style="visibility:hidden"'}>
          ${expanded ? icons.chevronDown : icons.chevronRight}
        </button>
        <button class="project-row__main" data-project="${escape(item.id)}" title="${escape(item.path)}">
          ${projectIcon(item)}
          <span class="project-row__text">
            <span class="project-row__title">${escape(item.title)}</span>
            ${state.settings.compact_sidebar === false ? `<span class="project-row__detail">${escape(projectDetail(item))}</span>` : ""}
          </span>
        </button>
        ${
          waiting
            ? `<span class="dot dot--waiting" title="${escape(t("A session needs you"))}"></span>`
            : item.running
              ? `<span class="dot dot--running" title="${escape(t("{count} running", { count: item.running }))}"></span>`
              : ""
        }
        ${item.sessions ? `<span class="project-row__count">${item.sessions}</span>` : ""}
        <button class="project-row__more" data-project-more="${escape(item.id)}"
                title="${escape(t("Actions for {name}", { name: item.title }))}">${icons.more}</button>
      </div>
      ${
        expanded && kids.length
          ? `<div class="sidebar__children sidebar__children--nested">${kids.map((kid) => row(kid, depth + 1)).join("")}</div>`
          : ""
      }`;
  };

  const pinned = needle ? [] : state.allSessions.filter((entry) => entry.pinned && !entry.archived);
  const body = [
    pinned.length
      ? `<div class="sidebar__group">${escape(t("Pinned"))}</div>
         <div class="project-row__sessions project-row__sessions--pinned">${pinned
           .map((entry) => sidebarSession(entry))
           .join("")}</div>`
      : "",
    // With the hierarchy on, a group is a row that folds its projects away,
    // as on macOS; off, its projects sit in the list like any other.
    ...(state.groupHierarchy === false
      ? [...groups.values()].flat().map((item) => row(item))
      : [...groups].map(([name, items]) => {
          const folded = state.collapsed.has(`group:${name}`) && !needle;
          return `
            <button class="sidebar__group sidebar__group--fold" data-group-toggle="${escape(name)}" aria-expanded="${!folded}">
              ${folded ? icons.chevronRight : icons.chevronDown}<span>${escape(name)}</span>
              <span class="project-row__count">${items.length}</span>
            </button>
            ${folded ? "" : `<div class="sidebar__children">${items.map((item) => row(item)).join("")}</div>`}`;
        })),
    ...loose.map((item) => row(item)),
  ].join("");

  return `
    <aside class="sidebar">
      <div class="sidebar__brand">
        <span class="sidebar__mark">${appMark(22)}</span>
        <span class="sidebar__name">Convoy</span>
      </div>
      <div class="sidebar__search">
        ${icons.search}
        <input id="project-search" type="search" placeholder="${escape(t("Find a project"))}"
               value="${escape(state.search)}" spellcheck="false" />
      </div>
      <div class="sidebar__label">
        <span>${escape(t("Projects"))}</span>
        <button class="sidebar__add" data-action="workspace-menu" title="${escape(t("Workspace options"))}">${icons.more}</button>
        <button class="sidebar__add" data-action="open-folder" title="${escape(t("Open folder"))}">${icons.plus}</button>
      </div>
      <div class="sidebar__list" data-scroll="sidebar">
        ${body || `<div class="sidebar__none">${escape(state.search ? t("No matching projects") : t("No projects yet."))}</div>`}
      </div>
      <div class="sidebar__footer">
        <div class="sidebar__open">
          <button class="sidebar__link" data-action="open-folder">${icons.folder} ${escape(t("Open folder…"))}</button>
          <span class="sidebar__hint">${escape(t("A repository, or a folder of related projects."))}</span>
        </div>
        <button class="sidebar__gear" data-action="settings" title="${escape(t("Settings"))}"
                aria-pressed="${state.page === "settings"}">${icons.gear}</button>
      </div>
    </aside>`;
}

/// Right-click and "…" menus for a project or a sidebar session, opened at
/// the pointer. Entries mirror the macOS app's.
export function contextMenu(menu) {
  if (menu.kind === "workspace") return workspaceMenu();
  if (menu.kind === "helpers") return helpersMenu(menu.id, anchorStyle());
  if (menu.kind === "skills") return skillsMenu(menu.id, anchorStyle());
  if (menu.kind === "account") return accountMenu(menu.agent);
  if (menu.kind === "limits") return limitsPanel();
  if (menu.kind === "awake") return awakeMenu();
  if (menu.kind === "tab") return tabMenu(menu);
  if (menu.kind === "activity") return activityPanel();
  if (menu.kind === "approvals") return approvalsPanel();
  const item = (label, act, { icon = "", enabled = true, danger = false, hint = "" } = {}) => `
    <button class="menu__item${danger ? " menu__item--danger" : ""}" data-menu-act="${act}"
            ${enabled ? "" : "disabled"}>
      <span class="menu__icon">${icons[icon] ?? ""}</span>
      <span>${escape(label)}</span>
      ${hint ? `<span class="menu__hint">${escape(t(hint))}</span>` : ""}
    </button>`;
  const divider = '<div class="menu__divider"></div>';
  let entries = "";

  if (menu.kind === "project") {
    entries = [
      item(t("New session…"), "new-session", { icon: "plus" }),
      item(t("Import provider history…"), "import-history", { icon: "history" }),
      item(
        state.projects.find((entry) => entry.id === menu.id)?.is_group ? t("Refresh projects") : t("Import projects from this folder"),
        "refresh",
        { icon: "refresh" },
      ),
      divider,
      item(revealLabel(), "reveal", { icon: "folder" }),
      item(t("Copy folder path"), "copy-path", { icon: "copy" }),
      item(t("Reconnect folder…"), "reconnect", { icon: "link" }),
      divider,
      item(t("Project settings…"), "project-settings", { icon: "gear" }),
      ...(state.settings.sort_projects
        ? []
        : [item(t("Move up"), "move-up"), item(t("Move down"), "move-down")]),
      divider,
      item(t("Remove from Convoy…"), "remove", { icon: "trash", danger: true }),
    ].join("");
  } else if (menu.kind === "task") {
    const task = [...(state.planning?.tasks ?? []), ...(state.allTasks ?? [])].find((entry) => entry.id === menu.id);
    if (!task) return "";
    const phase = phaseOf(task);
    const building = !movable(phase);
    const move = (label, target, icon = "") => (phase.phase === target ? "" : item(t(label), `task-status:${target}`, icon ? { icon } : {}));
    entries = [
      item(task.plan_first && task.plan?.status !== "approved" ? t("Make a plan") : t("Run"), "task-run", { icon: "play", enabled: runnable(task) }),
      task.status === "plan_review" ? item(t("Review the plan…"), "task-plan", { icon: "check" }) : "",
      task.plan?.session_id ? item(t("Open planner"), "task-planner", { icon: "terminal" }) : "",
      item(t("Open session"), "task-session", { icon: "terminal", enabled: !!task.session_id }),
      item(t("Open reviewer"), "task-reviewer", { icon: "review", enabled: !!task.review_session_id }),
      ["review", "pr", "testing", "changes", "attention", "failed"].includes(task.status) && task.session_id
        ? item(task.review?.status === "reviewing" ? t("Restart the review") : t("Review now"), "task-review-now", { icon: "review" })
        : "",
      ["reviewing", "no_verdict", "failed"].includes(task.review?.status)
        ? item(t("Skip review"), "task-skip-review", { icon: "chevronRight" })
        : "",
      task.pr_url ? item(t("Open pull request"), "task-pr", { icon: "open" }) : "",
      task.source?.url ? item(t("Open {key}", { key: task.source.key }), "task-issue", { icon: "open" }) : "",
      item(t("Edit…"), "task-edit", { icon: "edit" }),
      state.projects.find((entry) => entry.id === task.project_id)?.qa
        ? item(t("Test now"), "task-qa", { icon: "checkCircle", enabled: !building && !["checking", "testing"].includes(task.qa?.status) })
        : "",
      ["testing", "review", "pr", "done", "attention"].includes(task.status) && task.docs_run?.status !== "writing"
        ? item(task.docs?.length ? t("Write the document again") : t("Write the document"), "task-docs", { icon: "doc" })
        : "",
      task.sync?.failed ? item(t("Retry tracker sync"), "task-sync-retry", { icon: "refresh" }) : "",
      divider,
      ...(building
        ? [item(t("Move…"), "task-status:none", { enabled: false, hint: "stop it first" })]
        : [
            move("Move to To do", "todo"),
            move("Mark ready", "ready"),
            move("Mark done", "done", "check"),
            task.status === "changes" ? "" : item(t("Send back for changes"), "task-status:changes"),
          ]),
      divider,
      item(t("Delete task…"), "task-delete", { icon: "trash", danger: true, enabled: !building }),
    ].join("");
  } else {
    const target = anySession(menu.id);
    if (!target) return "";
    const running = target.running;
    const owned = taskOfSession(target.id);
    entries = [
      item(t("Open"), "open", { icon: "open" }),
      owned?.task ? item(t("Open task"), "open-task", { icon: "list" }) : "",
      item(target.pinned ? t("Unpin") : t("Pin to top"), "pin", { icon: "pin" }),
      running
        ? item(t("Sleep"), "sleep", { icon: "moon", hint: "stops, keeps the conversation" }) +
          item(t("Stop session…"), "stop", { icon: "stop", danger: true })
        : item(target.started ? t("Resume") : t("Start"), "start", {
            icon: "play",
            enabled: !target.archived && !target.worktree_removed,
          }),
      divider,
      item(t("Rename…"), "rename", { icon: "edit" }),
      item(t("Edit name & notes…"), "edit", { icon: "edit" }),
      item(t("Start review…"), "review", { icon: "review" }),
      item(target.archived ? t("Restore") : t("Archive"), "archive", {
        icon: "archive",
        enabled: !running,
        hint: running ? "stop it first" : "",
      }),
      item(t("Remove from Convoy…"), "remove", {
        icon: "close",
        enabled: !running,
        hint: running ? "stop it first" : "",
      }),
      item(t("Delete…"), "delete", {
        icon: "trash",
        danger: true,
        enabled: !running,
        hint: running ? "stop it first" : "",
      }),
      ...(target.working_directory
        ? [
            divider,
            item(t("{reveal} (worktree)", { reveal: revealLabel() }), "reveal-worktree", { icon: "folder" }),
            item(t("Copy worktree path"), "copy-worktree", { icon: "copy" }),
            item(t("Remove worktree…"), "remove-worktree", {
              icon: "trash",
              danger: true,
              enabled: !running && target.owns_worktree,
            }),
          ]
        : []),
    ].join("");
  }

  // Opened where it was asked for; render() keeps it inside the window by
  // its real size.
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--context" role="menu" style="left:${Math.max(8, menu.x)}px;top:${Math.max(8, menu.y)}px">
        ${entries}
      </div>
    </div>`;
}

// ----------------------------------------------------------------- header --

export function header() {
  const current = project();
  if (!current) return "";
  return `
    <div class="header">
      <div class="header__top">
        <div class="header__titles">
          <h1 class="header__name">
            ${escape(current.title)}
            ${current.group ? `<span class="badge">${escape(current.group)}</span>` : ""}
          </h1>
          <div class="header__path" title="${escape(current.path)}">${escape(current.path)}</div>
        </div>
        <div class="header__actions">
          ${button({ label: t("Settings"), icon: "gear", action: "project-settings", title: t("Project settings…") })}
        </div>
      </div>
      <nav class="tabs">
        ${TABS.map(
          ([key, label], index) =>
            `${index > 0 ? '<span class="tab__divider"></span>' : ""}
             <button class="tab" data-tab="${key}"
                     aria-selected="${state.tab === key || (key === "sessions" && state.tab === "reviews")}">${escape(t(label))}</button>`,
        ).join("")}
      </nav>
    </div>`;
}

// --------------------------------------------------------------- sessions --


/// Every row the Sessions list shows that can be deleted: its sessions that
/// are not running, then its saved conversations.
export const pickableKeys = () => [
  ...visibleSessions().filter((item) => !item.running).map((item) => `s:${item.id}`),
  ...shownConversations().map(conversationKey),
];

function sessionRow(item, { picking = false } = {}) {
  // What a person needs at a glance: is it running, which agent, where, and
  // when it was last used. The provider ID is in the tooltip.
  const now = sessionState(item, !!item.running);
  const status = item.running || now === "asleep"
    ? `<span class="row__state row__state--${sessionTone(now)}">${escape(t(sessionLabel(now)))}</span>`
    : "";
  const at = lastActive(item);
  const meta = [
    agentName(item.agent),
    now === "not_started" ? escape(t("Not started")) : null,
    item.archived ? escape(t("archived")) : null,
    item.branch ? escape(t("on {branch}", { branch: item.branch })) : null,
    at ? escape(when(at)) : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const key = `s:${item.id}`;
  const opener = picking
    ? `class="row row--picking"${item.running ? "" : ` data-pick-row="${escape(key)}"`}`
    : `class="row" data-open="${escape(item.id)}" data-menu-session="${escape(item.id)}"`;
  return `
    <div ${opener} title="${escape(item.running && picking ? t("Stop it to delete it") : item.provider_id)}">
      ${picking ? pickBox(key, item.title, item.running) : ""}
      <span class="row__mark">${agentIcon(item.agent, 16)}</span>
      <div class="row__body">
        <div class="row__title">
          ${item.pinned ? '<span class="row__pin">★</span>' : ""}${escape(item.title)}
        </div>
        <div class="row__meta">${status}${meta}</div>
      </div>
      ${picking ? "" : `<div class="row__actions">
        ${
          item.running
            ? button({ label: t("Stop"), icon: "stop", kind: "danger", data: { stop: item.id } })
            : button({
                label: item.started ? t("Resume") : t("Start"),
                icon: "play",
                kind: "quiet",
                data: { start: item.id },
                disabled: item.archived || item.worktree_removed,
              })
        }
      </div>`}
    </div>`;
}

/// The bar that replaces the list's buttons while rows are picked to delete.
function pickBar() {
  const keys = pickableKeys();
  const count = state.picked.size;
  return `
      <span class="pick__count">${escape(t("{count} selected", { count }))}</span>
      ${button({ label: t("Select all"), kind: "quiet", action: "pick-all", disabled: !keys.length || keys.every((key) => state.picked.has(key)) })}
      ${button({ label: t("Deselect all"), kind: "quiet", action: "pick-none", disabled: !count })}
      ${button({ label: t("Remove from Convoy…"), kind: "quiet", action: "pick-remove", title: t("Keeps the conversations; only Convoy forgets the sessions"), disabled: ![...state.picked].some((key) => key.startsWith("s:")) })}
      ${button({ label: t("Delete…"), icon: "trash", kind: "danger", action: "pick-delete", disabled: !count })}
      ${button({ label: t("Done"), action: "pick-done" })}`;
}

export function sessionsView() {
  const saved = historySection();
  if (!state.sessions.length && !state.showArchived) {
    // New session stays where it always is, even with nothing listed yet.
    if (!saved) {
      return `
      <div class="section">
        <span class="section__spacer"></span>
        ${button({ label: t("Import all history…"), icon: "history", action: "import-all-history" })}
        ${button({ label: t("New session"), icon: "plus", action: "new-session", kind: "primary" })}
      </div>
      ${empty("review", t("No sessions yet"), t("Start one to run an agent in this folder."))}`;
    }
    return `
      <div class="section">
        <h2 class="section__title">${escape(t("No sessions yet"))}</h2>
        <span class="section__spacer"></span>
        ${
          state.picked
            ? pickBar()
            : `${button({ label: t("Select"), icon: "check", action: "pick-start", title: t("Select conversations to delete") })}
        ${button({ label: t("Import all history…"), icon: "history", action: "import-all-history" })}
        ${button({ label: t("New session"), icon: "plus", action: "new-session", kind: "primary" })}`
        }
      </div>
      <p class="section__hint">${escape(t("Resume a conversation below or start a new session."))}</p>
      <div class="stack" data-scroll="sessions">${saved}</div>`;
  }
  // Running ones first: they are what someone comes back to.
  const shown = visibleSessions();
  const picking = !!state.picked;
  const rows = [...shown.filter((item) => item.running), ...shown.filter((item) => !item.running)]
    .map((item) => sessionRow(item, { picking }))
    .join("");
  return `
    <div class="section">
      <h2 class="section__title">${escape(t("Sessions"))}
        <span class="section__count">${shown.length}</span>
      </h2>
      ${sessionsSwitch()}
      <span class="section__spacer"></span>
      <div class="filter">
        ${icons.search}
        <input id="session-filter" type="search" placeholder="${escape(t("Filter by name or ID"))}"
               value="${escape(state.filter)}" spellcheck="false" />
      </div>
      ${
        picking
          ? pickBar()
          : `${button({ icon: "refresh", action: "refresh", kind: "icon", title: t("Refresh") })}
      <label class="check">
        <input type="checkbox" data-toggle-state="showArchived"
               ${state.showArchived ? "checked" : ""} />
        ${escape(t("Archived"))}
      </label>
      ${button({ label: t("Select"), icon: "check", action: "pick-start", title: t("Select sessions to delete") })}
      ${button({ label: t("Import all history…"), icon: "history", action: "import-all-history", title: t("Every Claude Code and Codex conversation on this computer") })}
      ${button({ label: t("New session"), icon: "plus", action: "new-session", kind: "primary" })}`
      }
    </div>
    <div class="stack" data-scroll="sessions">
      <div class="list">
        ${rows || `<div class="list__none">${escape(t("Nothing matches “{filter}”.", { filter: state.filter }))}</div>`}
      </div>
      ${saved}
    </div>`;
}

/// All sessions, or only reviews: the Sessions page's two views.
const sessionsSwitch = () => `
  <div class="segmented">
    <button data-sessions-show="sessions" aria-pressed="${state.tab !== "reviews"}">${escape(t("All"))}</button>
    <button data-sessions-show="reviews" aria-pressed="${state.tab === "reviews"}">${escape(t("Reviews"))}</button>
  </div>`;

export function reviewsView() {
  const reviews = state.sessions.filter((item) => item.review_of);
  if (!reviews.length) {
    return `
      <div class="section">
        <h2 class="section__title">${escape(t("Reviews"))}</h2>
        ${sessionsSwitch()}
      </div>
      ${empty(
        "review",
        t("Review with another agent"),
        t("Open a session and choose Start review. The other agent receives an editable brief."),
      )}`;
  }
  return `
    <div class="section">
      <h2 class="section__title">${escape(t("Reviews"))}
        <span class="section__count">${reviews.length}</span>
      </h2>
      ${sessionsSwitch()}
    </div>
    <p class="section__hint">
      ${escape(t("A review runs in the builder's own folder, so it reads exactly what was written there."))}
    </p>
    <div class="list">${reviews.map(sessionRow).join("")}</div>`;
}

// -------------------------------------------------------------- workbench --

export function workbench() {
  const current = session();
  if (!current) return "";
  const now = sessionState(current, !!current.running);
  const label = t(sessionLabel(now));
  const tone = { needs_you: "state--waiting", finished: "state--done", idle: "state--done", working: "state--running" }[now] ?? "";

  const git = state.settings.git_status === false ? null : state.git;
  const view = viewOf(current);
  return `
    <div class="workbench">
      <div class="workbench__bar">
        <button class="crumb crumb--menu" data-project-more="${escape(current.project_id ?? state.projectId)}" title="${escape(t("Project actions"))}">
          ${escape(project()?.title ?? "")} ${icons.chevronDown}
        </button>
        <span class="crumb__sep">/</span>
        <button class="crumb crumb--menu crumb--session" data-action="session-menu" title="${escape(t("Session actions"))}">
          <span>${escape(current.title)}</span> ${icons.chevronDown}
        </button>
        <span class="section__spacer"></span>
        ${
          git
            ? `<span class="bar-git" id="git-status" title="${escape(t("Branch and changed files"))}">${icons.branch} ${escape(git.branch)} · ${escape(t("{count} changed", { count: git.changed }))}</span>`
            : ""
        }
        ${view === "terminal" ? helpersButton(current.id) : ""}
        ${view === "terminal" ? skillsChip(current.id) : ""}
        ${viewToggle(view)}
        <span class="bar-agent">${agentName(current.agent)}</span>
        <span class="state ${tone}"><span class="state__dot"></span>${escape(label.replace(/^./, (c) => c.toUpperCase()))}</span>
        ${
          current.running
            ? ""
            : button({
                label: current.started ? t("Resume") : t("Start"),
                icon: "play",
                data: { start: current.id },
                disabled: current.archived || current.worktree_removed,
              })
        }
        ${button({ icon: "bolt", action: "quick-menu", kind: "quiet", title: t("Quick commands") })}
        ${button({ icon: "plus", action: "new-session", kind: "quiet", title: t("New session") })}
        ${button({ icon: "more", action: "session-menu", kind: "quiet", title: t("Session actions") })}
      </div>
      ${
        !current.running && missingConversation(current.id)
          ? `<div class="session-banner session-banner--warn">
               <div>
                 <div class="session-banner__title">${escape(t("Claude couldn't find this conversation"))}</div>
                 <div class="session-banner__text">${escape(t("It may have closed before the first message. Start fresh; this session is kept."))}</div>
               </div>
               ${button({ label: t("Start fresh"), action: "start-fresh", kind: "primary" })}
             </div>`
          : shownAsTranscript(current)
            ? `<div class="session-banner">${icons.history} ${escape(t("Saved conversation · read-only. Resume to reconnect to the agent."))}</div>`
            : ""
      }
      ${state.find ? findBar() : ""}
      ${
        state.layout > 1
          ? panesView()
          : view === "chat"
            ? '<div class="chat-slot" id="chat-host"></div>'
            : shownAsTranscript(current)
              ? '<div class="chat-slot chat-slot--readonly" id="chat-host"></div>'
              : '<div class="term" id="terminal-host"></div>'
      }
    </div>`;
}

/// Chat | Terminal: two ways to show the same conversation.
function viewToggle(view) {
  return `
    <div class="segmented segmented--bar" role="group" aria-label="${escape(t("Show the session as"))}">
      ${[
        ["chat", t("Chat")],
        ["terminal", t("Terminal")],
      ]
        .map(
          ([value, text]) =>
            `<button data-action="session-view" data-view="${value}" aria-pressed="${value === view}">${escape(text)}</button>`,
        )
        .join("")}
    </div>`;
}

/// Find in the focused terminal: next and previous, match case, "n of m".
function findBar() {
  const find = state.find;
  const count = !find.term ? "" : find.count ? t("{index} of {count}", { index: find.index + 1, count: find.count }) : t("No matches");
  return `
    <div class="find-bar" role="search">
      ${icons.search}
      <input id="find-term" class="find-bar__input" type="search" placeholder="${escape(t("Find in terminal"))}"
             value="${escape(find.term)}" spellcheck="false" autocomplete="off" />
      <span class="find-bar__count">${escape(count)}</span>
      <button class="find-bar__button find-bar__case" data-action="find-case" aria-pressed="${find.caseSensitive}"
              title="${escape(t("Match case"))}">Aa</button>
      <button class="find-bar__button find-bar__up" data-action="find-previous" title="${escape(t("Previous match (⇧↩)"))}">${icons.chevronDown}</button>
      <button class="find-bar__button" data-action="find-next" title="${escape(t("Next match (↩)"))}">${icons.chevronDown}</button>
      <button class="find-bar__button" data-action="find-close" title="${escape(t("Close (Esc)"))}">${icons.close}</button>
    </div>`;
}

/// Two or four panes, as in the macOS app. Each shows its own session; the
/// focused one is the session the bars and tabs refer to.
/// What an empty pane offers: running sessions first, then recent ones, from
/// any project, minus those already on screen.
function paneSuggestions() {
  const shown = new Set(state.panes.filter(Boolean));
  const candidates = state.allSessions.filter((item) => !shown.has(item.id));
  return [
    ...candidates.filter((item) => isRunning(item.id)),
    ...[...candidates].reverse().filter((item) => !isRunning(item.id)),
  ].slice(0, 4);
}

function panesView() {
  const cells = Array.from({ length: state.layout }, (_, index) => {
    const id = state.panes[index];
    const info = id ? tabInfo(id) : null;
    const focused = index === state.focus;
    if (!info) {
      return `
        <div class="pane pane--empty${focused ? " pane--focused" : ""}" data-pane="${index}">
          <div class="pane__label"><span class="pane__number">${index + 1}</span> ${escape(t("Empty pane"))}</div>
          <div class="pane__empty">
            ${paneSuggestions()
              .map((item) => `
              <button class="pane__suggestion" data-split-into="${index}:${escape(item.id)}">
                ${agentIcon(item.agent, 12)}
                <span>${escape(item.title)}</span>
                <span class="pane__project">${escape(state.projects.find((entry) => entry.id === item.project_id)?.title ?? "")}</span>
              </button>`)
              .join("")}
            <div class="pane__empty-actions">
              ${button({ label: t("Choose a session…"), icon: "terminal", data: { "pane-pick": index } })}
              ${button({ label: t("New session"), icon: "plus", data: { "pane-new": index } })}
            </div>
          </div>
        </div>`;
    }
    const running = state.running.includes(id);
    const owner = state.projects.find((item) => item.id === info.project_id);
    return `
      <div class="pane${focused ? " pane--focused" : ""}" data-pane="${index}">
        <div class="pane__label">
          <span class="pane__number" style="background:${owner ? projectTint(owner) : "var(--text-faint)"}">${index + 1}</span>
          ${agentIcon(info.agent, 11)}
          <span class="pane__title">${escape(info.title)}</span>
          <span class="pane__project">${escape(owner?.title ?? "")}</span>
          <span class="section__spacer"></span>
          ${
            running
              ? ""
              : button({ label: info.started ? t("Resume") : t("Start"), icon: "play", kind: "quiet", data: { start: id } })
          }
          ${running ? button({ icon: "bolt", kind: "quiet", title: t("Quick commands"), data: { "pane-quick": index } }) : ""}
          ${button({ icon: "paneOne", kind: "quiet", title: t("Maximize (one pane)"), data: { "pane-max": index } })}
          ${button({ icon: "close", kind: "quiet", title: t("Close pane"), data: { "pane-close": index } })}
        </div>
        ${
          viewOf(anySession(id) ?? info) === "chat"
            ? `<div class="chat-slot" id="chat-host-${index}"></div>`
            : shownAsTranscript(anySession(id) ?? info)
              ? `<div class="chat-slot chat-slot--readonly" id="chat-host-${index}"></div>`
              : `<div class="term" id="terminal-host-${index}"></div>`
        }
      </div>`;
  }).join("");
  return `<div class="workbench__panes workbench__panes--${state.layout}">${cells}</div>`;
}

/// The thirteen entries, each offered only when it applies.
export function sessionMenu() {
  const current = session();
  if (!current) return "";
  const busy = current.running;
  const item = (label, action, enabled = true, hint = "") => `
    <button class="menu__item" data-action="${action}" ${enabled ? "" : "disabled"}>
      <span>${escape(t(label))}</span>
      ${hint ? `<span class="menu__hint">${escape(t(hint))}</span>` : ""}
    </button>`;

  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--session menu--anchored" role="menu" style="${anchorStyle()}">
        ${busy ? item("Stop session…", "stop-session") : item(current.started ? "Resume" : "Start", "start-session", !current.archived && !current.worktree_removed)}
        ${busy ? item("Sleep", "sleep-session", true, "keeps the conversation") : ""}
        <div class="menu__divider"></div>
        ${item("Rename…", "rename-current")}
        ${item("Edit session…", "edit-session")}
        ${item(current.pinned ? "Unpin" : "Pin", "pin-session")}
        ${item(current.archived ? "Restore" : "Archive", "archive-session", !busy, busy ? "stop it first" : "")}
        ${item("Fresh recovery session", "recover-session", !busy && !current.worktree_removed)}
        <div class="menu__divider"></div>
        ${item("Start review…", "start-review")}
        ${item("Send feedback to builder…", "send-feedback", !!current.review_of)}
        <div class="menu__divider"></div>
        ${item("Saved output…", "saved-output")}
        ${item("Usage limits…", "usage")}
        ${item("Refresh Git status", "git-status")}
        <div class="menu__divider"></div>
        ${item(
          "Create worktree…",
          "create-worktree",
          !busy && !current.started && !current.working_directory && !current.review_of && !current.archived,
        )}
        ${item("Remove worktree…", "remove-worktree", !busy && current.owns_worktree)}
        <div class="menu__divider"></div>
        ${item("Two panes", "layout-2", state.layout !== 2)}
        ${item("Four panes", "layout-4", state.layout !== 4)}
        ${item("Single pane", "layout-1", state.layout > 1)}
      </div>
    </div>`;
}

export function quickMenu() {
  const commands = state.quickCommands;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--anchored" role="menu" style="${anchorStyle()}">
        ${
          commands.length
            ? commands
                .map(
                  (command) => `
            <button class="menu__item" data-quick="${escape(command.id)}">
              <span>${escape(command.title)}</span>
              ${command.submit ? `<span class="menu__hint">${escape(t("sends Enter"))}</span>` : ""}
            </button>`,
                )
                .join("")
            : `<div class="menu__empty">${escape(t("No quick commands yet."))}</div>`
        }
        <div class="menu__divider"></div>
        <button class="menu__item" data-action="manage-commands">
          <span>${escape(t("Manage quick commands…"))}</span>
        </button>
      </div>
    </div>`;
}

// ----------------------------------------------------------------- status --

export function status() {
  const running = state.running.length;
  const queues = state.queues.size;
  return `
    <footer class="status">
      <button class="status__item status__button" data-action="limits"
              aria-pressed="${state.menu?.kind === "limits"}" title="${escape(t("AI Limits"))}">
        ${icons.limits} <strong>${escape(t("AI Limits"))}</strong>
        ${limitsSummary()}
      </button>
      ${accountChip("claude")}${accountChip("codex")}${accountChip("gemini")}
      <span class="status__spacer"></span>
      ${queues ? `<span class="status__item">${icons.bolt} ${escape(plural(queues, "queue"))}</span><span class="status__sep"></span>` : ""}
      ${needsYouButton()}
      <span class="status__item">${icons.terminal} ${escape(t("{count} running", { count: running }))}</span>
      <span class="status__sep"></span>
      <button class="status__item status__button${awakeActive() ? " status__button--on" : ""}" data-action="awake-menu"
              aria-pressed="${state.menu?.kind === "awake"}" title="${escape(t("Keep the computer awake while agents work"))}">
        ${awakeActive() ? icons.cupFilled : icons.cup} ${escape(awakeActive() ? t("Awake") : t("Keep awake"))} ${icons.chevronDown}
      </button>
    </footer>`;
}


export { escape, empty, group, setting, segmented, choice, toggle, button };

// -------------------------------------------------------------- AI limits --

/// The most used window, which is the one that stops you first.
const peak = (reading) =>
  reading?.windows?.length ? Math.max(...reading.windows.map((window) => window.percent)) : null;

function limitsSummary() {
  const limits = state.limits;
  if (!limits) return "";
  // A reading older than five minutes says so, as on macOS.
  const stale = (reading) => !!reading?.updated_at && Date.now() / 1000 - reading.updated_at > 300;
  const parts = [
    ["Codex", peak(limits.codex), stale(limits.codex)],
    ["Claude", peak(limits.claude), stale(limits.claude)],
  ]
    .filter(([, percent]) => percent !== null)
    .map(([name, percent, stale]) => `<span class="status__limit${percent >= 90 ? " status__limit--high" : ""}">${escape(t("{name} {percent}% used", { name, percent: Math.round(percent) }))}${stale ? ` · ${escape(t("cached"))}` : ""}</span>`);
  return parts.join("");
}

const windowName = (window) => {
  if (window.minutes) {
    if (window.minutes % 1440 === 0) return t("{count}-day window", { count: window.minutes / 1440 });
    if (window.minutes % 60 === 0) return t("{count}-hour window", { count: window.minutes / 60 });
  }
  return window.name.replace(/_/g, " ");
};

const resetText = (window) => {
  if (!window.resets_at) return "";
  const date = new Date(window.resets_at * 1000);
  const soon = date - Date.now() < 24 * 3600 * 1000;
  return t("resets {when}", { when: soon ? date.toLocaleTimeString(language(), { hour: "numeric", minute: "2-digit" }) : date.toLocaleDateString(language(), { weekday: "short", hour: "numeric", minute: "2-digit" }) });
};

export function limitsBlock(name, icon, reading, loading) {
  const rows = reading?.windows?.length
    ? reading.windows
        .map(
          (window) => `
      <div class="limit">
        <div class="limit__head">
          <span>${escape(windowName(window))}</span>
          <strong>${Math.round(window.percent)}%</strong>
        </div>
        <div class="limit__bar"><span class="${window.percent >= 90 ? "limit__fill--high" : window.percent >= 70 ? "limit__fill--mid" : ""}" style="width:${Math.min(100, Math.max(2, window.percent))}%"></span></div>
        ${resetText(window) ? `<div class="limit__note">${escape(resetText(window))}</div>` : ""}
      </div>`,
        )
        .join("")
    : `<div class="limit__empty">${escape(loading ? t("Reading…") : reading?.note ?? t("Not read yet."))}</div>`;
  const updated = reading?.updated_at
    ? new Date(reading.updated_at * 1000).toLocaleTimeString(language(), { hour: "numeric", minute: "2-digit" })
    : "";
  return `
    <section class="limits__provider">
      <div class="limits__title">${agentIcon(icon, 14)}<span>${name}</span>${updated ? `<span class="limits__updated">${escape(updated)}</span>` : ""}</div>
      ${rows}
    </section>`;
}

export function limitsPanel() {
  const limits = state.limits ?? {};
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu limits" role="dialog" aria-label="${escape(t("AI Limits"))}">
        <div class="limits__header">
          <strong>${escape(t("AI Limits"))}</strong>
          <span class="section__spacer"></span>
          ${button({ icon: "refresh", action: "limits-refresh", kind: "quiet", title: t("Refresh"), disabled: !!limits.loading })}
        </div>
        ${limitsBlock("Claude", "claude", limits.claude, false)}
        ${limitsBlock("Codex", "codex", limits.codex, limits.loading && !limits.codex?.windows?.length)}
        <div class="limits__foot">
          <span>${escape(t("Read from your own logins. No tokens are copied and no model request is sent."))}</span>
          <button class="sidebar__link" data-action="limits-settings">${escape(t("Settings…"))}</button>
        </div>
      </div>
    </div>`;
}

// --------------------------------------------------------------- activity --

/// Events newer than the last time the feed was opened.
export const unreadActivity = () =>
  (state.activity ?? []).filter((event) => !state.activitySeen || event.at > state.activitySeen).length;

/// The bell, as in the macOS top bar: the feed spans every project, and the
/// badge counts what arrived since it was last opened.
export function bell() {
  const unread = unreadActivity();
  return `
    <button class="button button--icon bell" data-action="activity" title="${escape(t("Activity"))}"
            aria-pressed="${state.menu?.kind === "activity"}">
      ${icons.bell}
      ${unread ? `<span class="bell__badge">${Math.min(99, unread)}</span>` : ""}
    </button>`;
}

const ACTIVITY = {
  waiting: ["needs you", "warn", "activity--waiting"],
  done: ["finished", "check", "activity--done"],
  started: ["started", "play", ""],
  resumed: ["resumed", "refresh", ""],
  hibernated: ["asleep", "moon", ""],
  exited: ["stopped", "stop", ""],
  worktree: ["worktree", "branch", "activity--worktree"],
};

function activityPanel() {
  const needle = (state.activityFilter ?? "").trim().toLowerCase();
  const events = (state.activity ?? []).filter(
    (event) =>
      !needle || `${event.title} ${event.project ?? ""} ${event.detail}`.toLowerCase().includes(needle),
  );
  const days = new Map();
  const today = new Date().toDateString();
  const yesterday = new Date(Date.now() - 86400000).toDateString();
  for (const event of events) {
    const date = new Date(event.at);
    const key = date.toDateString();
    const name = key === today ? t("Today") : key === yesterday ? t("Yesterday") : date.toLocaleDateString(language(), { day: "numeric", month: "short", year: "numeric" });
    if (!days.has(name)) days.set(name, []);
    days.get(name).push(event);
  }
  const row = (event) => {
    const helper = helperActivity(event.detail);
    const automation = helper ? null : automationActivity(event.detail);
    const [label, glyph, tone] = helper
      ? [helper[0], "helpers", event.kind === "done" ? "activity--done" : ""]
      : automation
        ? [automation[0], "clock", event.kind === "done" ? "activity--done" : ""]
        : (ACTIVITY[sleptBy(event) ? "hibernated" : event.kind] ?? [event.kind, "clock", ""]);
    if (helper) event = { ...event, detail: helper[1] };
    if (automation) event = { ...event, detail: automation[1] };
    const time = new Date(event.at).toLocaleTimeString(language(), { hour: "numeric", minute: "2-digit" });
    return `
      <button class="activity ${tone}" data-activity-open="${escape(event.session_id)}"
              data-activity-project="${escape(event.project_id ?? "")}">
        <span class="activity__icon">${icons[glyph] ?? ""}</span>
        <span class="activity__body">
          <span class="activity__title">${escape(event.title)} <em>${escape(t(label))}</em></span>
          <span class="activity__meta">${escape([event.project, event.detail].filter(Boolean).join(" · "))}</span>
        </span>
        <span class="activity__time">${escape(time)}</span>
      </button>`;
  };
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu activity-feed" role="dialog" aria-label="${escape(t("Activity"))}">
        <div class="activity-feed__head">
          <strong>${escape(t("Activity"))}</strong>
          <span class="section__spacer"></span>
          <button class="sidebar__link" data-action="activity-clear">${escape(t("Clear"))}</button>
        </div>
        <div class="sidebar__search activity-feed__search">
          ${icons.search}
          <input id="activity-filter" type="search" placeholder="${escape(t("Filter by session or project"))}"
                 value="${escape(state.activityFilter ?? "")}" spellcheck="false" />
        </div>
        <div class="activity-feed__list" data-scroll="activity">
          ${
            events.length
              ? [...days].map(([day, items]) => `<div class="activity-feed__day">${escape(day)}</div>${items.map(row).join("")}`).join("")
              : `<p class="activity-feed__none">${escape(needle ? t("Nothing matches.") : t("Nothing yet. Completions, questions and worktree events show here."))}</p>`
          }
        </div>
      </div>
    </div>`;
}

// -------------------------------------------------------------------- tabs --

/// What a tab shows for a session, from the loaded list when its project is
/// the open one, else from what was remembered when it was opened.
export const tabInfo = (id) =>
  state.sessions.find((item) => item.id === id) ?? state.tabInfo?.[id] ?? state.allSessions.find((item) => item.id === id);

/// A tab's state, drawn as macOS draws it: a check once the agent finished
/// its turn, a warning when it needs you, a dot while it works.
function tabGlyph(id, running, info) {
  const item = { ...info, id, started: info.started ?? running };
  const now = sessionState(item, running);
  if (now === "finished") return `<span class="tab-glyph tab-glyph--done" title="${escape(t(sessionLabel(now)))}">${icons.checkCircle}</span>`;
  if (now === "needs_you") return `<span class="tab-glyph tab-glyph--waiting" title="${escape(t(sessionLabel(now)))}">${icons.warn}</span>`;
  return stateDot(item, running);
}

const PROJECT_VIEWS = {
  sessions: "Sessions",
  reviews: "Reviews",
  specs: "Specs",
  tasks: "Tasks",
  docs: "Docs",
  automations: "Automations",
};

/// A project's page as a tab: its icon and name, and the part of the page
/// it was left on.
function projectTabItem(id, index) {
  const info = tabInfo(id);
  const owner = state.projects.find((item) => item.id === info?.project_id);
  if (!owner) return "";
  const selected =
    !state.files && !state.sessionId && state.projectId === owner.id && (!state.page || state.page === "project");
  const view = PROJECT_VIEWS[info.view] ?? PROJECT_VIEWS.sessions;
  return `
    <div class="tab-item tab-item--project${selected ? " tab-item--selected" : ""}" data-tab-open="${escape(id)}"
         draggable="true" title="${escape(`${owner.title} · ${t(view)}`)}">
      <span class="tab-item__bar" style="background:${projectTint(owner)}"></span>
      ${projectIcon(owner, 13)}
      <span class="tab-item__text">
        <span class="tab-item__title">${escape(owner.title)}</span>
        <span class="tab-item__project">${escape(t(view))}</span>
      </span>
      ${index < 9 && !selected ? `<span class="tab-item__number">${/mac/i.test(navigator.platform) ? "⌘" : "Ctrl+"}${index + 1}</span>` : ""}
      <button class="tab-item__close" data-tab-close="${escape(id)}" title="${escape(t("Close tab"))}">${icons.close}</button>
    </div>`;
}

/// The bar across the top, as in the macOS app: Home, one tab per open
/// session or project page, then Files & Changes, the bell and New session.
export function tabBar() {
  const tabs = state.tabs
    .map((id, index) => {
      if (isProjectTab(id)) return projectTabItem(id, index);
      const info = tabInfo(id);
      if (!info) return "";
      const owner = state.projects.find((item) => item.id === info.project_id);
      const running = state.running.includes(id);
      const selected = state.sessionId === id && !state.files && state.page !== "project";
      return `
        <div class="tab-item${selected ? " tab-item--selected" : ""}" data-tab-open="${escape(id)}"
             draggable="true" title="${escape(`${owner?.title ?? ""} · ${agentName(info.agent)}${running ? "" : ` · ${t(sessionLabel(sessionState({ ...info, id }, false)))}`}`)}">
          <span class="tab-item__bar" style="background:${owner ? projectTint(owner) : "var(--border-strong)"}"></span>
          ${tabGlyph(id, running, info)}
          ${agentIcon(info.agent, 12)}
          <span class="tab-item__text">
            <span class="tab-item__title">${escape(info.title)}</span>
            <span class="tab-item__project">${escape(owner?.title ?? "")}</span>
          </span>
          ${
            state.layout > 1 && state.panes.indexOf(id) >= 0 && state.panes.indexOf(id) < state.layout
              ? `<span class="tab-item__pane" style="background:${owner ? projectTint(owner) : "var(--text-faint)"}" title="${escape(t("In pane {number}", { number: state.panes.indexOf(id) + 1 }))}">${state.panes.indexOf(id) + 1}</span>`
              : ""
          }
          ${index < 9 && !selected && !(state.layout > 1 && state.panes.includes(id)) ? `<span class="tab-item__number">${/mac/i.test(navigator.platform) ? "⌘" : "Ctrl+"}${index + 1}</span>` : ""}
          <button class="tab-item__close" data-tab-close="${escape(id)}"
                  title="${escape(running ? t("Stop and close") : t("Close tab"))}">${icons.close}</button>
        </div>`;
    })
    .join("");
  const home = ["home", "dashboard", "usage"].includes(state.page);
  return `
    <div class="tabbar">
      <button class="tabbar__home${state.sidebarHidden ? "" : " tabbar__home--on"}" data-action="toggle-sidebar"
              title="${escape(state.sidebarHidden ? t("Show sidebar") : t("Hide sidebar"))}">${icons.sidebar}</button>
      <button class="tabbar__home${home ? " tabbar__home--on" : ""}" data-action="home" title="${escape(t("Home"))}">${icons.home}</button>
      <span class="tabbar__divider"></span>
      <div class="tabbar__tabs" data-scroll="tabs">
        ${tabs || `<span class="tabbar__none">${escape(t("No open terminals"))}</span>`}
      </div>
      <span class="tabbar__actions">
        <span class="layouts">
          ${[
            [1, "paneOne", "One pane"],
            [2, "split", "Two panes"],
            [4, "paneFour", "Four panes"],
          ]
            .map(
              ([count, glyph, name]) =>
                `<button class="layouts__button" data-layout="${count}" aria-pressed="${state.layout === count}" title="${escape(t(name))}">${icons[glyph]}</button>`,
            )
            .join("")}
        </span>
        <button class="button button--icon counted" data-action="files" title="${escape(t("Files & Changes"))}">
          ${icons.git}
          ${state.settings.git_status !== false && state.git?.changed ? `<span class="counted__badge">${Math.min(99, state.git.changed)}</span>` : ""}
        </button>
        ${approvalsButton()}
        ${bell()}
        ${state.projects.length ? button({ icon: "plus", action: "new-session", kind: "icon", title: t("New session") }) : ""}
      </span>
    </div>`;
}

function tabMenu(menu) {
  const at = state.tabs.indexOf(menu.id);
  const item = (label, act, enabled = true) =>
    `<button class="menu__item" data-tab-act="${act}" ${enabled ? "" : "disabled"}><span>${escape(t(label))}</span></button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--context" role="menu" style="left:${Math.max(8, Math.min(menu.x, window.innerWidth - 240))}px;top:${menu.y}px">
        ${
          state.layout > 1 && !isProjectTab(menu.id)
            ? Array.from({ length: state.layout }, (_, index) => item(t("Open in pane {number}", { number: index + 1 }), `pane-${index}`)).join("") +
              '<div class="menu__divider"></div>'
            : ""
        }
        ${item("Move left", "left", at > 0)}
        ${item("Move right", "right", at < state.tabs.length - 1)}
        <div class="menu__divider"></div>
        ${item("Close tab", "close")}
        ${item("Close other tabs", "others", state.tabs.length > 1)}
      </div>
    </div>`;
}

/// Whether the computer is being kept awake right now, as the system
/// answered, not just what the setting asks for.
export const awakeActive = () => state.awake === true;

/// "N need you": what waits for an answer, in any project. One opens its
/// session; more open the approvals inbox.
function needsYouButton() {
  const items = inboxItems();
  if (!items.length) return "";
  const titles = [...new Set(items.map((item) => tabInfo(item.id)?.title).filter(Boolean))].join(", ");
  const target = items.length === 1 ? `data-needs-you="${escape(items[0].id)}"` : 'data-action="approvals"';
  return `
    <button class="status__item status__button status__needs" ${target}
            title="${escape(titles)}">
      ${icons.warn} ${escape(plural(items.length, "needs you", "need you"))}
    </button>
    <span class="status__sep"></span>`;
}

/// "Awake ⌄", as on macOS: the three modes, then Settings.
/// The sidebar's own options, as the macOS sidebar menu has them.
function workspaceMenu() {
  const item = (on, act, name) => `
    <button class="menu__item" data-workspace-option="${act}">
      <span class="menu__icon">${on ? icons.check : ""}</span><span>${escape(t(name))}</span>
    </button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--anchored" role="menu" style="${anchorStyle()}">
        <div class="menu__heading">${escape(t("Sidebar"))}</div>
        ${item(state.groupHierarchy !== false, "hierarchy", "Show group hierarchy")}
        ${item(!!state.settings.sort_projects, "sort", "Sort projects by name")}
        ${item(state.settings.compact_sidebar !== false, "compact", "Compact rows")}
        <div class="menu__divider"></div>
        <button class="menu__item" data-action="settings"><span class="menu__icon">${icons.gear}</span><span>${escape(t("Settings…"))}</span></button>
        ${
          state.projects.length
            ? `<div class="menu__divider"></div>
               <button class="menu__item menu__item--danger" data-action="remove-all-projects"><span class="menu__icon">${icons.trash}</span><span>${escape(t("Remove all projects…"))}</span></button>`
            : ""
        }
      </div>
    </div>`;
}

/// Which account new sessions of one agent start with, from the status bar.
function accountMenu(agent) {
  const active = activeAccount(agent);
  const item = (id, name) => `
    <button class="menu__item" data-account-pick="${agent}:${escape(id)}">
      <span class="menu__icon">${active === id ? icons.check : ""}</span><span>${escape(name)}</span>
    </button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--status" role="menu" style="left:${Math.max(8, state.anchor?.left ?? 8)}px">
        <div class="menu__heading">${escape(t("{agent} account", { agent: agentName(agent) }))}</div>
        ${item("", t("System login"))}
        ${(state.profiles ?? []).filter((profile) => profile.agent === agent).map((profile) => item(profile.id, profile.label)).join("")}
        <div class="menu__divider"></div>
        <button class="menu__item" data-settings-open="Accounts"><span class="menu__icon">${icons.person}</span><span>${escape(t("Manage accounts…"))}</span></button>
      </div>
    </div>`;
}

/// The account chip for an agent, shown once there is an account to pick.
function accountChip(agent) {
  if (!(state.profiles ?? []).some((profile) => profile.agent === agent)) return "";
  const id = activeAccount(agent);
  const name = id ? state.profiles.find((profile) => profile.id === id)?.label : t("System");
  return `
    <button class="status__item status__button" data-action="account-menu" data-agent="${agent}"
            title="${escape(t("Account for new {agent} sessions", { agent: agentName(agent) }))}">
      ${agentIcon(agent, 12)} ${escape(name ?? t("System"))} ${icons.chevronDown}
    </button>`;
}

function awakeMenu() {
  const mode = state.settings.keep_awake;
  const item = (value, name) => `
    <button class="menu__item" data-awake="${value}">
      <span class="menu__icon">${mode === value ? icons.check : ""}</span><span>${escape(t(name))}</span>
    </button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--awake" role="menu">
        <div class="menu__heading">${escape(t("Keep awake"))}</div>
        ${item("always", "Always while Convoy is open")}
        ${item("sessions", "While a session is running")}
        ${item("off", "Off — normal system sleep")}
        <div class="menu__divider"></div>
        <button class="menu__item" data-action="awake-settings">
          <span class="menu__icon">${icons.gear}</span><span>${escape(t("Settings…"))}</span>
        </button>
      </div>
    </div>`;
}

/// Places a menu under the control that opened it, kept on screen.
export const anchorStyle = () => {
  const at = state.anchor;
  if (!at) return "";
  // A control at the bottom of the window (a chat's composer) opens upwards.
  if (at.bottom != null) return `bottom:${at.bottom}px;left:${Math.max(8, at.left)}px;top:auto`;
  const top = Math.min(at.top, Math.max(8, window.innerHeight - 420));
  // A control in the left half (the sidebar) opens its menu rightwards, so
  // a wide menu never runs past the window's left edge.
  if (at.left < window.innerWidth / 2) return `top:${top}px;left:${Math.max(8, at.left)}px`;
  return `top:${top}px;right:${at.right}px`;
};
