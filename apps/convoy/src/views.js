// Every screen. Markup built from state; nothing here calls the backend.

import { agentIcon, icons, projectIcon } from "./icons.js";
import {
  button,
  choice,
  empty,
  escape,
  group,
  segmented,
  setting,
  shorten,
  toggle,
} from "./ui.js";
import { isRunning, project, session, state, visibleSessions } from "./state.js";

const TABS = [
  ["sessions", "Sessions"],
  ["reviews", "Reviews"],
  ["specs", "Specs"],
  ["tasks", "Tasks"],
  ["activity", "Activity"],
];

// ---------------------------------------------------------------- sidebar --

/// "Show in Finder" on a Mac, the file manager's own name elsewhere.
export const revealLabel = () =>
  /Mac/.test(navigator.platform)
    ? "Show in Finder"
    : /Win/.test(navigator.platform)
      ? "Show in Explorer"
      : "Show in Files";

/// The dot beside a session: what its agent last reported, or whether it runs.
function stateDot(item) {
  const reported = state.agentState.get(item.id);
  const tone = !item.running
    ? item.started
      ? "stopped"
      : "idle"
    : reported === "waiting"
      ? "waiting"
      : reported === "done"
        ? "done"
        : "running";
  const label = {
    stopped: "Stopped",
    idle: "Not started",
    waiting: "Needs you",
    done: "Finished its turn",
    running: reported ?? "Running",
  }[tone];
  return `<span class="dot dot--${tone}" title="${escape(label)}"></span>`;
}

function sidebarSession(item) {
  return `
    <button class="side-session" data-open="${escape(item.id)}"
            data-menu-session="${escape(item.id)}"
            aria-current="${item.id === state.sessionId}"
            title="${escape(item.notes || item.title)}">
      ${stateDot(item)}
      ${item.pinned ? `<span class="side-session__pin">${icons.pin}</span>` : ""}
      <span class="side-session__title">${escape(item.title)}</span>
      ${
        item.branch
          ? `<span class="chip chip--branch">${icons.branch}${escape(item.branch)}</span>`
          : `${agentIcon(item.agent, 11)}${item.review_of ? '<span class="side-session__tag">review</span>' : ""}`
      }
    </button>`;
}

export function sidebar() {
  const needle = state.search.toLowerCase();
  const groups = new Map();
  const loose = [];
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

  const row = (item) => {
    const current = item.id === state.projectId;
    // Sessions are loaded for the selected project, so that is the one that
    // can open; the others show a chevron that selects and opens them.
    const expanded = current && !state.collapsed.has(item.id) && !needle;
    const sessions = current ? state.sessions.filter((entry) => !entry.archived) : [];
    const waiting = current && sessions.some((entry) => entry.running && state.agentState.get(entry.id) === "waiting");
    return `
      <div class="project-row${current ? " project-row--current" : ""}"
           data-menu-project="${escape(item.id)}">
        <button class="project-row__chevron" data-expand="${escape(item.id)}"
                title="${expanded ? "Collapse" : "Expand"}"
                ${item.sessions || current ? "" : 'style="visibility:hidden"'}>
          ${expanded ? icons.chevronDown : icons.chevronRight}
        </button>
        <button class="project-row__main" data-project="${escape(item.id)}" title="${escape(item.path)}">
          ${projectIcon(item)}
          <span class="project-row__title">${escape(item.title)}</span>
        </button>
        ${
          waiting
            ? '<span class="dot dot--waiting" title="A session needs you"></span>'
            : item.running
              ? `<span class="dot dot--running" title="${item.running} running"></span>`
              : ""
        }
        ${item.sessions ? `<span class="project-row__count">${item.sessions}</span>` : ""}
        <button class="project-row__more" data-project-more="${escape(item.id)}"
                title="Actions for ${escape(item.title)}">${icons.more}</button>
      </div>
      ${
        expanded && sessions.length
          ? `<div class="project-row__sessions">${sessions.map(sidebarSession).join("")}</div>`
          : ""
      }`;
  };

  const body = [
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
      <div class="sidebar__label">
        <span>Projects</span>
        <button class="sidebar__add" data-action="open-folder" title="Open folder">${icons.plus}</button>
      </div>
      <div class="sidebar__list" data-scroll="sidebar">
        ${body || `<div class="sidebar__none">${state.search ? "No matching projects" : "No projects yet."}</div>`}
      </div>
      <div class="sidebar__footer">
        <div class="sidebar__open">
          <button class="sidebar__link" data-action="open-folder">${icons.folder} Open folder…</button>
          <span class="sidebar__hint">A repository, or a folder of related projects.</span>
        </div>
        <button class="sidebar__gear" data-action="settings" title="Settings"
                aria-pressed="${state.page === "settings"}">${icons.gear}</button>
      </div>
    </aside>`;
}

/// Right-click and "…" menus for a project or a sidebar session, opened at
/// the pointer. Entries mirror the macOS app's.
export function contextMenu(menu) {
  const item = (label, act, { icon = "", enabled = true, danger = false, hint = "" } = {}) => `
    <button class="menu__item${danger ? " menu__item--danger" : ""}" data-menu-act="${act}"
            ${enabled ? "" : "disabled"}>
      <span class="menu__icon">${icons[icon] ?? ""}</span>
      <span>${escape(label)}</span>
      ${hint ? `<span class="menu__hint">${escape(hint)}</span>` : ""}
    </button>`;
  const divider = '<div class="menu__divider"></div>';
  let entries = "";

  if (menu.kind === "project") {
    entries = [
      item("New session…", "new-session", { icon: "plus" }),
      item("Import provider history…", "import-history", { icon: "history" }),
      divider,
      item(revealLabel(), "reveal", { icon: "folder" }),
      item("Copy folder path", "copy-path", { icon: "copy" }),
      item("Reconnect folder…", "reconnect", { icon: "link" }),
      divider,
      item("Project settings…", "project-settings", { icon: "gear" }),
      divider,
      item("Remove from Convoy…", "remove", { icon: "trash", danger: true }),
    ].join("");
  } else {
    const target = state.sessions.find((entry) => entry.id === menu.id);
    if (!target) return "";
    const running = target.running;
    entries = [
      item("Open", "open", { icon: "open" }),
      item(target.pinned ? "Unpin" : "Pin to top", "pin", { icon: "pin" }),
      running
        ? item("Sleep", "sleep", { icon: "moon", hint: "stops, keeps the conversation" }) +
          item("Stop session…", "stop", { icon: "stop", danger: true })
        : item(target.started ? "Resume" : "Start", "start", {
            icon: "play",
            enabled: !target.archived && !target.worktree_removed,
          }),
      divider,
      item("Edit name & notes…", "edit", { icon: "edit" }),
      item("Start review…", "review", { icon: "review" }),
      item(target.archived ? "Restore" : "Archive", "archive", {
        icon: "archive",
        enabled: !running,
        hint: running ? "stop it first" : "",
      }),
      ...(target.working_directory
        ? [
            divider,
            item(`${revealLabel()} (worktree)`, "reveal-worktree", { icon: "folder" }),
            item("Copy worktree path", "copy-worktree", { icon: "copy" }),
            item("Remove worktree…", "remove-worktree", {
              icon: "trash",
              danger: true,
              enabled: !running && target.owns_worktree,
            }),
          ]
        : []),
    ].join("");
  }

  // Kept on screen: a menu opened near the bottom or right edge flips.
  const left = Math.min(menu.x, window.innerWidth - 260);
  const top = Math.min(menu.y, window.innerHeight - (menu.kind === "project" ? 300 : 360));
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--context" role="menu" style="left:${Math.max(8, left)}px;top:${Math.max(8, top)}px">
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
          <div class="header__path">${escape(current.path)}</div>
        </div>
        <div class="header__actions">
          ${button({ icon: "git", action: "files", title: "Files & Changes", kind: "icon" })}
          ${button({ icon: "gear", action: "settings", title: "Settings", kind: "icon" })}
          ${button({ label: "New session", icon: "plus", action: "new-session", kind: "primary" })}
        </div>
      </div>
      <nav class="tabs">
        ${TABS.map(
          ([key, label], index) =>
            `${index > 1 ? '<span class="tab__divider"></span>' : ""}
             <button class="tab" data-tab="${key}"
                     aria-selected="${state.tab === key}">${label}</button>`,
        ).join("")}
      </nav>
    </div>`;
}

// --------------------------------------------------------------- sessions --

const agentName = (agent) => (agent === "claude" ? "Claude Code" : "Codex");

function sessionRow(item) {
  const reported = state.agentState.get(item.id);
  const meta = [
    agentName(item.agent),
    item.running ? (reported ?? "running") : item.started ? "stopped" : "never started",
    item.archived ? "archived" : null,
    item.branch ? `on ${item.branch}` : null,
    item.provider_id ? `<code>${escape(item.provider_id.slice(0, 8))}</code>` : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return `
    <div class="row" data-open="${escape(item.id)}">
      <span class="row__mark">${agentIcon(item.agent, 16)}</span>
      <div class="row__body">
        <div class="row__title">
          ${item.pinned ? '<span class="row__pin">★</span>' : ""}${escape(item.title)}
        </div>
        <div class="row__meta">${meta}</div>
      </div>
      <div class="row__actions">
        ${
          item.running
            ? button({ label: "Stop", icon: "stop", kind: "danger", data: { stop: item.id } })
            : button({
                label: item.started ? "Resume" : "Start",
                icon: "play",
                data: { start: item.id },
                disabled: item.archived || item.worktree_removed,
              })
        }
        ${button({ icon: "open", kind: "quiet", data: { open: item.id }, title: "Open the terminal" })}
      </div>
    </div>`;
}

export function sessionsView() {
  if (!state.sessions.length && !state.showArchived) {
    return empty(
      "review",
      "No sessions yet",
      "Start one to run Claude Code or Codex in this folder.",
    );
  }
  const rows = visibleSessions().map(sessionRow).join("");
  return `
    <div class="section">
      <h2 class="section__title">Saved conversations
        <span class="section__count">${visibleSessions().length}</span>
      </h2>
      <span class="section__spacer"></span>
      <div class="filter">
        ${icons.search}
        <input id="session-filter" type="search" placeholder="Filter by name or ID"
               value="${escape(state.filter)}" spellcheck="false" />
      </div>
      ${button({ icon: "refresh", action: "refresh", kind: "icon", title: "Refresh" })}
      <label class="check">
        <input type="checkbox" data-toggle-state="showArchived"
               ${state.showArchived ? "checked" : ""} />
        Archived
      </label>
      ${button({ label: "New session", icon: "plus", action: "new-session", kind: "primary" })}
    </div>
    <p class="section__hint">
      Pick a session, start a new one, or import a conversation Claude Code or
      Codex already saved for this folder.
    </p>
    <div class="list">
      ${rows || `<div class="list__none">Nothing matches “${escape(state.filter)}”.</div>`}
    </div>`;
}

export function reviewsView() {
  const reviews = state.sessions.filter((item) => item.review_of);
  if (!reviews.length) {
    return empty(
      "review",
      "Review with another agent",
      "Open a session and choose Start review. The other agent receives an editable brief.",
    );
  }
  return `
    <div class="section">
      <h2 class="section__title">Reviews
        <span class="section__count">${reviews.length}</span>
      </h2>
    </div>
    <p class="section__hint">
      A review runs in the builder's own folder, so it reads exactly what was
      written there.
    </p>
    <div class="list">${reviews.map(sessionRow).join("")}</div>`;
}

// -------------------------------------------------------------- workbench --

export function workbench() {
  const current = session();
  if (!current) return "";
  const other = state.split
    ? state.sessions.find((item) => item.id === state.split)
    : null;
  const reported = state.agentState.get(current.id);
  const label = current.running
    ? (reported ?? "Running")
    : current.started
      ? "Stopped"
      : "Not started";
  const tone = current.running
    ? reported === "waiting"
      ? "state--waiting"
      : reported === "done"
        ? "state--done"
        : "state--running"
    : "";

  return `
    <div class="workbench">
      <div class="workbench__bar">
        <span class="crumb">
          ${escape(project()?.title ?? "")}
          <span class="crumb__sep">/</span>
          <span class="crumb__muted">${escape(current.title)}</span>
        </span>
        ${current.branch ? `<span class="badge--muted badge">${escape(current.branch)}</span>` : ""}
        <span class="section__spacer"></span>
        <span class="state ${tone}"><span class="state__dot"></span>${escape(label)}</span>
        <span class="git-status" id="git-status">${escape(state.gitStatus ?? "")}</span>
        ${
          current.running
            ? button({ label: "Stop", icon: "stop", kind: "danger", data: { stop: current.id } })
            : button({
                label: current.started ? "Resume" : "Start",
                icon: "play",
                data: { start: current.id },
              })
        }
        ${button({ icon: "bolt", action: "quick-menu", kind: "quiet", title: "Quick commands" })}
        ${button({ icon: "more", action: "session-menu", kind: "quiet", title: "Session actions" })}
        ${button({ icon: "back", action: "back", kind: "quiet", title: "Back to the list" })}
      </div>
      ${
        other
          ? `<div class="workbench__panes">
               <div class="pane">
                 <div class="pane__label">${escape(current.title)}</div>
                 <div class="terminal" id="terminal-host"></div>
               </div>
               <div class="pane">
                 <div class="pane__label">
                   ${escape(other.title)}
                   ${
                     other.running
                       ? button({ label: "Stop", icon: "stop", kind: "quiet", data: { stop: other.id } })
                       : button({
                           label: other.started ? "Resume" : "Start",
                           icon: "play",
                           kind: "quiet",
                           data: { start: other.id },
                         })
                   }
                   ${button({ icon: "close", action: "close-split", kind: "quiet", title: "Close the split" })}
                 </div>
                 <div class="terminal" id="terminal-host-split"></div>
               </div>
             </div>`
          : '<div class="terminal" id="terminal-host"></div>'
      }
    </div>`;
}

/// The thirteen entries, each offered only when it applies.
export function sessionMenu() {
  const current = session();
  if (!current) return "";
  const busy = current.running;
  const item = (label, action, enabled = true, hint = "") => `
    <button class="menu__item" data-action="${action}" ${enabled ? "" : "disabled"}>
      <span>${escape(label)}</span>
      ${hint ? `<span class="menu__hint">${escape(hint)}</span>` : ""}
    </button>`;

  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--session" role="menu">
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
        ${item("Open split terminal…", "open-split", state.sessions.length > 1)}
        ${item("Close split view", "close-split", !!state.split)}
      </div>
    </div>`;
}

export function quickMenu() {
  const commands = state.quickCommands;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu" role="menu">
        ${
          commands.length
            ? commands
                .map(
                  (command) => `
            <button class="menu__item" data-quick="${escape(command.id)}">
              <span>${escape(command.title)}</span>
              ${command.submit ? '<span class="menu__hint">sends Enter</span>' : ""}
            </button>`,
                )
                .join("")
            : '<div class="menu__empty">No quick commands yet.</div>'
        }
        <div class="menu__divider"></div>
        <button class="menu__item" data-action="manage-commands">
          <span>Manage quick commands…</span>
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
      <span class="status__item">${icons.limits} <strong>AI Limits</strong></span>
      <span class="status__sep"></span>
      <span class="status__item" title="${escape(state.storage)}">
        ${escape(shorten(state.storage, 52))}
      </span>
      <span class="status__spacer"></span>
      ${queues ? `<span class="status__item">${icons.bolt} ${queues} queue${queues > 1 ? "s" : ""}</span><span class="status__sep"></span>` : ""}
      <span class="status__item">${icons.terminal} ${running} running</span>
      <span class="status__sep"></span>
      <span class="status__item">${icons.awake} ${escape(awakeLabel())}</span>
    </footer>`;
}

const awakeLabel = () =>
  ({ off: "Sleep allowed", always: "Awake", sessions: "Awake while running" })[
    state.settings.keep_awake
  ] ?? "Awake";

export { escape, empty, group, setting, segmented, choice, toggle, button };
