// Every screen. Markup built from state; nothing here calls the backend.

import { agentIcon, appMark, icons, projectIcon, projectTint } from "./icons.js";
import {
  button,
  choice,
  empty,
  escape,
  group,
  segmented,
  setting,
  toggle,
} from "./ui.js";
import { isRunning, project, session, state, visibleSessions } from "./state.js";

const TABS = [
  ["sessions", "Sessions"],
  ["reviews", "Reviews"],
  ["specs", "Specs"],
  ["tasks", "Tasks"],
  ["docs", "Docs"],
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
      <span class="side-session__text">
        <span class="side-session__title">${escape(item.title)}</span>
        ${
          state.settings.compact_sidebar === false
            ? `<span class="side-session__detail">${escape(
                [
                  item.running ? (state.agentState.get(item.id) ?? "Running") : item.started ? "Stopped" : "Saved",
                  item.agent === "claude" ? "Claude Code" : "Codex",
                ].join(" · "),
              )}</span>`
            : ""
        }
      </span>
      ${
        item.branch
          ? `<span class="chip chip--branch">${icons.branch}${escape(item.branch)}</span>`
          : `${agentIcon(item.agent, 11)}${item.review_of ? '<span class="side-session__tag">review</span>' : ""}`
      }
    </button>`;
}

/// The second line of a detailed project row: its branch and changes, or
/// how many sessions it has when it is not a repository.
function projectDetail(item) {
  const git = state.settings.git_status === false ? null : state.projectGit.get(item.id);
  if (git) return `⎇ ${git.branch}${git.changed_files ? ` · ${git.changed_files} changed` : " · clean"}`;
  return `${item.sessions} session${item.sessions === 1 ? "" : "s"}`;
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

  if (state.settings.sort_projects) {
    const byName = (a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
    loose.sort(byName);
    for (const items of groups.values()) items.sort(byName);
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
          <span class="project-row__text">
            <span class="project-row__title">${escape(item.title)}</span>
            ${state.settings.compact_sidebar === false ? `<span class="project-row__detail">${escape(projectDetail(item))}</span>` : ""}
          </span>
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
        <span class="sidebar__mark">${appMark(22)}</span>
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
  if (menu.kind === "limits") return limitsPanel();
  if (menu.kind === "awake") return awakeMenu();
  if (menu.kind === "tab") return tabMenu(menu);
  if (menu.kind === "activity") return activityPanel();
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
          <div class="header__path" title="${escape(current.path)}">${escape(current.path)}</div>
        </div>
        <div class="header__actions">
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

  const git = state.settings.git_status === false ? null : state.git;
  return `
    <div class="workbench">
      <div class="workbench__bar">
        <button class="crumb crumb--menu" data-project-more="${escape(current.project_id ?? state.projectId)}" title="Project actions">
          ${escape(project()?.title ?? "")} ${icons.chevronDown}
        </button>
        <span class="crumb__sep">/</span>
        <button class="crumb crumb--menu crumb--session" data-action="session-menu" title="Session actions">
          <span>${escape(current.title)}</span> ${icons.chevronDown}
        </button>
        <span class="section__spacer"></span>
        ${
          git
            ? `<span class="bar-git" id="git-status" title="Branch and changed files">${icons.branch} ${escape(git.branch)} · ${git.changed} changed</span>`
            : ""
        }
        <span class="bar-agent">${current.agent === "claude" ? "Claude Code" : "Codex"}</span>
        <span class="state ${tone}"><span class="state__dot"></span>${escape(label.replace(/^./, (c) => c.toUpperCase()))}</span>
        ${
          current.running
            ? ""
            : button({
                label: current.started ? "Resume" : "Start",
                icon: "play",
                data: { start: current.id },
                disabled: current.archived || current.worktree_removed,
              })
        }
        ${button({ icon: "bolt", action: "quick-menu", kind: "quiet", title: "Quick commands" })}
        ${button({ icon: "plus", action: "new-session", kind: "quiet", title: "New session" })}
        ${button({ icon: "more", action: "session-menu", kind: "quiet", title: "Session actions" })}
      </div>
      ${
        other
          ? `<div class="workbench__panes">
               <div class="pane">
                 <div class="pane__label">${escape(current.title)}</div>
                 <div class="term" id="terminal-host"></div>
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
                 <div class="term" id="terminal-host-split"></div>
               </div>
             </div>`
          : '<div class="term" id="terminal-host"></div>'
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
      <div class="menu menu--session menu--anchored" role="menu" style="${anchorStyle()}">
        ${busy ? item("Stop session…", "stop-session") : item(current.started ? "Resume" : "Start", "start-session", !current.archived && !current.worktree_removed)}
        ${busy ? item("Sleep", "sleep-session", true, "keeps the conversation") : ""}
        <div class="menu__divider"></div>
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
      <div class="menu menu--anchored" role="menu" style="${anchorStyle()}">
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
      <button class="status__item status__button" data-action="limits"
              aria-pressed="${state.menu?.kind === "limits"}" title="AI Limits">
        ${icons.limits} <strong>AI Limits</strong>
        ${limitsSummary()}
      </button>
      <span class="status__spacer"></span>
      ${queues ? `<span class="status__item">${icons.bolt} ${queues} queue${queues > 1 ? "s" : ""}</span><span class="status__sep"></span>` : ""}
      <span class="status__item">${icons.terminal} ${running} running</span>
      <span class="status__sep"></span>
      <button class="status__item status__button" data-action="awake-menu" aria-pressed="${state.menu?.kind === "awake"}">
        ${icons.awake} ${escape(awakeLabel())} ${icons.chevronDown}
      </button>
    </footer>`;
}

const awakeLabel = () =>
  ({ off: "Sleep allowed", always: "Awake", sessions: "Awake while running" })[
    state.settings.keep_awake
  ] ?? "Awake";

export { escape, empty, group, setting, segmented, choice, toggle, button };

// -------------------------------------------------------------- AI limits --

/// The most used window, which is the one that stops you first.
const peak = (reading) =>
  reading?.windows?.length ? Math.max(...reading.windows.map((window) => window.percent)) : null;

function limitsSummary() {
  const limits = state.limits;
  if (!limits) return "";
  const parts = [
    ["Codex", peak(limits.codex)],
    ["Claude", peak(limits.claude)],
  ]
    .filter(([, percent]) => percent !== null)
    .map(([name, percent]) => `<span class="status__limit${percent >= 90 ? " status__limit--high" : ""}">${name} ${Math.round(percent)}% used</span>`);
  return parts.join("");
}

const windowName = (window) => {
  if (window.minutes) {
    if (window.minutes % 1440 === 0) return `${window.minutes / 1440}-day window`;
    if (window.minutes % 60 === 0) return `${window.minutes / 60}-hour window`;
  }
  return window.name.replace(/_/g, " ");
};

const resetText = (window) => {
  if (!window.resets_at) return "";
  const date = new Date(window.resets_at * 1000);
  const soon = date - Date.now() < 24 * 3600 * 1000;
  return `resets ${soon ? date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : date.toLocaleDateString([], { weekday: "short", hour: "numeric", minute: "2-digit" })}`;
};

function limitsBlock(name, icon, reading, loading) {
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
    : `<div class="limit__empty">${escape(loading ? "Reading…" : reading?.note ?? "Not read yet.")}</div>`;
  const updated = reading?.updated_at
    ? new Date(reading.updated_at * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
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
      <div class="menu limits" role="dialog" aria-label="AI Limits">
        <div class="limits__header">
          <strong>AI Limits</strong>
          <span class="section__spacer"></span>
          ${button({ icon: "refresh", action: "limits-refresh", kind: "quiet", title: "Refresh", disabled: !!limits.loading })}
        </div>
        ${limitsBlock("Claude", "claude", limits.claude, false)}
        ${limitsBlock("Codex", "codex", limits.codex, limits.loading && !limits.codex?.windows?.length)}
        <div class="limits__foot">
          <span>Read from your own logins. No tokens are copied and no model request is sent.</span>
          <button class="sidebar__link" data-action="limits-settings">Settings…</button>
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
    <button class="button button--icon bell" data-action="activity" title="Activity"
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
  hibernated: ["slept", "moon", ""],
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
    const name = key === today ? "Today" : key === yesterday ? "Yesterday" : date.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
    if (!days.has(name)) days.set(name, []);
    days.get(name).push(event);
  }
  const row = (event) => {
    const [label, glyph, tone] = ACTIVITY[event.kind] ?? [event.kind, "clock", ""];
    const time = new Date(event.at).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    return `
      <button class="activity ${tone}" data-activity-open="${escape(event.session_id)}"
              data-activity-project="${escape(event.project_id ?? "")}">
        <span class="activity__icon">${icons[glyph] ?? ""}</span>
        <span class="activity__body">
          <span class="activity__title">${escape(event.title)} <em>${escape(label)}</em></span>
          <span class="activity__meta">${escape([event.project, event.detail].filter(Boolean).join(" · "))}</span>
        </span>
        <span class="activity__time">${escape(time)}</span>
      </button>`;
  };
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu activity-feed" role="dialog" aria-label="Activity">
        <div class="activity-feed__head">
          <strong>Activity</strong>
          <span class="section__spacer"></span>
          <button class="sidebar__link" data-action="activity-clear">Clear</button>
        </div>
        <div class="sidebar__search activity-feed__search">
          ${icons.search}
          <input id="activity-filter" type="search" placeholder="Filter by session or project"
                 value="${escape(state.activityFilter ?? "")}" spellcheck="false" />
        </div>
        <div class="activity-feed__list" data-scroll="activity">
          ${
            events.length
              ? [...days].map(([day, items]) => `<div class="activity-feed__day">${escape(day)}</div>${items.map(row).join("")}`).join("")
              : `<p class="activity-feed__none">${needle ? "Nothing matches." : "Nothing yet. Completions, questions and worktree events show here."}</p>`
          }
        </div>
      </div>
    </div>`;
}

// -------------------------------------------------------------------- tabs --

/// What a tab shows for a session, from the loaded list when its project is
/// the open one, else from what was remembered when it was opened.
export const tabInfo = (id) => state.sessions.find((item) => item.id === id) ?? state.tabInfo?.[id];

/// A tab's state, drawn as macOS draws it: a check once the agent finished
/// its turn, a warning when it needs you, a dot while it works.
function tabGlyph(id, running, info) {
  const reported = state.agentState.get(id);
  if (running && reported === "done") return `<span class="tab-glyph tab-glyph--done" title="Finished its turn">${icons.checkCircle}</span>`;
  if (running && reported === "waiting") return `<span class="tab-glyph tab-glyph--waiting" title="Needs you">${icons.warn}</span>`;
  return stateDot({ ...info, running, started: info.started ?? running });
}

/// The bar across the top, as in the macOS app: Home, one tab per open
/// session in any project, then Files & Changes, the bell and New session.
export function tabBar() {
  const tabs = state.tabs
    .map((id, index) => {
      const info = tabInfo(id);
      if (!info) return "";
      const owner = state.projects.find((item) => item.id === info.project_id);
      const running = state.running.includes(id);
      const selected = state.sessionId === id && !state.files && state.page !== "project";
      return `
        <div class="tab-item${selected ? " tab-item--selected" : ""}" data-tab-open="${escape(id)}"
             draggable="true" title="${escape(`${owner?.title ?? ""} · ${info.agent === "claude" ? "Claude Code" : "Codex"}${running ? "" : " · not running"}`)}">
          <span class="tab-item__bar" style="background:${owner ? projectTint(owner) : "var(--border-strong)"}"></span>
          ${tabGlyph(id, running, info)}
          ${agentIcon(info.agent, 12)}
          <span class="tab-item__text">
            <span class="tab-item__title">${escape(info.title)}</span>
            <span class="tab-item__project">${escape(owner?.title ?? "")}</span>
          </span>
          ${index < 9 && !selected ? `<span class="tab-item__number">${/mac/i.test(navigator.platform) ? "⌘" : "Ctrl+"}${index + 1}</span>` : ""}
          <button class="tab-item__close" data-tab-close="${escape(id)}"
                  title="${running ? "Stop and close" : "Close tab"}">${icons.close}</button>
        </div>`;
    })
    .join("");
  const home = !state.sessionId || state.page === "project";
  return `
    <div class="tabbar">
      <button class="tabbar__home${state.sidebarHidden ? "" : " tabbar__home--on"}" data-action="toggle-sidebar"
              title="${state.sidebarHidden ? "Show sidebar" : "Hide sidebar"}">${icons.sidebar}</button>
      <button class="tabbar__home${home ? " tabbar__home--on" : ""}" data-action="home" title="Home">${icons.home}</button>
      <span class="tabbar__divider"></span>
      <div class="tabbar__tabs" data-scroll="tabs">
        ${tabs || '<span class="tabbar__none">No open terminals</span>'}
      </div>
      <span class="tabbar__actions">
        <span class="layouts">
          <button class="layouts__button" data-action="layout-one" aria-pressed="${!state.split}" title="One pane">${icons.paneOne}</button>
          <button class="layouts__button" data-action="layout-two" aria-pressed="${!!state.split}" title="Two panes"
                  ${state.sessionId ? "" : "disabled"}>${icons.split}</button>
        </span>
        <button class="button button--icon counted" data-action="files" title="Files & Changes">
          ${icons.git}
          ${state.settings.git_status !== false && state.git?.changed ? `<span class="counted__badge">${Math.min(99, state.git.changed)}</span>` : ""}
        </button>
        ${bell()}
        ${state.projects.length ? button({ icon: "plus", action: "new-session", kind: "icon", title: "New session" }) : ""}
      </span>
    </div>`;
}

function tabMenu(menu) {
  const at = state.tabs.indexOf(menu.id);
  const item = (label, act, enabled = true) =>
    `<button class="menu__item" data-tab-act="${act}" ${enabled ? "" : "disabled"}><span>${escape(label)}</span></button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--context" role="menu" style="left:${Math.max(8, Math.min(menu.x, window.innerWidth - 240))}px;top:${menu.y}px">
        ${item("Move left", "left", at > 0)}
        ${item("Move right", "right", at < state.tabs.length - 1)}
        <div class="menu__divider"></div>
        ${item("Close tab", "close")}
        ${item("Close other tabs", "others", state.tabs.length > 1)}
      </div>
    </div>`;
}

/// "Awake ⌄": the keep-awake modes, from the status bar.
function awakeMenu() {
  const mode = state.settings.keep_awake;
  const item = (value, name) => `
    <button class="menu__item" data-awake="${value}">
      <span class="menu__icon">${mode === value ? icons.check : ""}</span><span>${name}</span>
    </button>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--awake" role="menu">
        ${item("always", "Always keep awake")}
        ${item("sessions", "While a session runs")}
        ${item("off", "Allow sleep")}
      </div>
    </div>`;
}

/// Places a menu under the control that opened it, kept on screen.
export const anchorStyle = () => {
  const at = state.anchor;
  if (!at) return "";
  const top = Math.min(at.top, Math.max(8, window.innerHeight - 420));
  return `top:${top}px;right:${at.right}px`;
};
