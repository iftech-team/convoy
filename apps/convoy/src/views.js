// Every screen. Markup built from state; nothing here calls the backend.

import { icons } from "./icons.js";
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

  const sessions = (item) =>
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
            data-menu-project="${escape(item.id)}"
            aria-current="${item.id === state.projectId}">
      <span class="project__icon">${escape(item.icon || "")}</span>
      <span class="project__title">${escape(item.title)}</span>
      ${item.running ? '<span class="project__running"></span>' : ""}
      <span class="project__count">${item.sessions || ""}</span>
    </button>${sessions(item)}`;

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
      <div class="sidebar__label">Projects</div>
      <div class="sidebar__list">
        ${body || `<div class="sidebar__none">${state.search ? "Nothing matches." : "No projects yet."}</div>`}
      </div>
      <div class="sidebar__footer">
        ${button({ label: "Open folder…", icon: "folder", action: "open-folder" })}
      </div>
    </aside>`;
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
      <span class="row__mark">${item.agent === "claude" ? "✳" : "◉"}</span>
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
