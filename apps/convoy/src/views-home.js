// Home and the Agent Dashboard: views over every project at once, as the
// macOS app has them.

import { agentIcon, icons, projectIcon } from "./icons.js";
import { button, escape, plural } from "./ui.js";
import { isRunning, state } from "./state.js";
import { limitsBlock } from "./views.js";

const ownerOf = (item) => state.projects.find((entry) => entry.id === item.project_id);
const reported = (item) => (isRunning(item.id) ? state.agentState.get(item.id) : null);
const waiting = (item) => reported(item) === "waiting";

function greeting() {
  const hour = new Date().getHours();
  return hour < 5 ? "Good night" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
}

/// One session as a card: its state, project and agent, and the one action
/// that fits — open it, or resume it.
function sessionCard(item) {
  const owner = ownerOf(item);
  const running = isRunning(item.id);
  const state_ = reported(item);
  const label = !running ? (item.started ? "Stopped" : "Not started") : state_ === "waiting" ? "Needs you" : state_ === "done" ? "Finished its turn" : "Working";
  const tone = !running ? "stopped" : state_ === "waiting" ? "waiting" : state_ === "done" ? "done" : "running";
  return `
    <div class="home-card" data-open="${escape(item.id)}" data-menu-session="${escape(item.id)}">
      <div class="home-card__head">
        <span class="dot dot--${tone}"></span>
        <span class="home-card__title">${escape(item.title)}</span>
        ${agentIcon(item.agent, 13)}
      </div>
      <div class="home-card__meta">${escape([owner?.title, label, item.branch ? `⎇ ${item.branch}` : ""].filter(Boolean).join(" · "))}</div>
      ${
        running
          ? ""
          : `<div class="home-card__actions">${button({ label: item.started ? "Resume" : "Start", icon: "play", data: { start: item.id } })}</div>`
      }
    </div>`;
}

function block(title, body, count = null) {
  return `
    <section class="home-block">
      <h3 class="home-block__title">${escape(title)}${count === null ? "" : ` <span class="section__count">${count}</span>`}</h3>
      ${body}
    </section>`;
}

const cards = (items, none) =>
  items.length ? `<div class="home-grid">${items.map(sessionCard).join("")}</div>` : `<p class="home-none">${escape(none)}</p>`;

export function homeView() {
  const sessions = state.allSessions;
  const needs = sessions.filter(waiting);
  const running = sessions.filter((item) => isRunning(item.id) && !waiting(item));
  // Most recently created first; the ones on screen above are left out.
  const recent = [...sessions].reverse().filter((item) => !isRunning(item.id)).slice(0, 6);
  const tasks = state.allTasks ?? [];
  const count = (...statuses) => tasks.filter((task) => statuses.includes(task.status)).length;
  const missing = (state.diagnostics ?? []).filter((check) => !check.found).length;
  const limits = state.limits ?? {};
  const activity = (state.activity ?? []).slice(0, 6);

  return `
    <div class="home" data-scroll="home">
      <div class="home__head">
        <h1 class="home__greeting">${greeting()}</h1>
        <div class="home__summary">
          ${escape(plural(running.length + needs.length, "agent") + " running")}${needs.length ? ` · <strong>${needs.length} need${needs.length === 1 ? "s" : ""} you</strong>` : ""}
          ${missing ? `<button class="chip chip--warn" data-settings-open="Setup">${icons.warn} ${escape(plural(missing, "setup problem"))}</button>` : ""}
        </div>
        <span class="section__spacer"></span>
        ${button({ label: "Agent Dashboard", icon: "board", action: "dashboard" })}
        ${button({ label: "New session", icon: "plus", action: "new-session", kind: "primary" })}
      </div>
      ${needs.length ? block("Needs you", cards(needs, ""), needs.length) : ""}
      ${block("Running", cards(running, "Nothing is running."), running.length)}
      ${block("Recent", cards(recent, "No sessions yet. Start one with New session."))}
      <div class="home-columns">
        ${block(
          "Tasks",
          `<div class="home-stats">
             ${[
               ["Queued", count("queued")],
               ["Running", count("building")],
               ["To review", count("review", "pr", "changes")],
               ["Done", count("done")],
             ]
               .map(([name, value]) => `<button class="home-stat" data-home-tasks="1"><strong>${value}</strong><span>${name}</span></button>`)
               .join("")}
           </div>`,
        )}
        ${block("AI Limits", `<div class="home-limits">${limitsBlock("Claude", "claude", limits.claude, limits.loading)}${limitsBlock("Codex", "codex", limits.codex, limits.loading)}</div>`)}
        ${block(
          "Activity",
          activity.length
            ? `<ul class="home-activity">${activity
                .map(
                  (event) => `
                <li><button data-activity-open="${escape(event.session_id)}">
                  <span>${escape(event.title)}</span>
                  <span class="home-activity__detail">${escape(event.detail)}</span>
                </button></li>`,
                )
                .join("")}</ul>`
            : '<p class="home-none">Nothing yet.</p>',
        )}
      </div>
      ${block(
        "Projects",
        state.projects.length
          ? `<div class="home-grid">${state.projects
              .map(
                (item) => `
            <button class="home-card home-card--project" data-project="${escape(item.id)}">
              <div class="home-card__head">${projectIcon(item)}<span class="home-card__title">${escape(item.title)}</span></div>
              <div class="home-card__meta">${escape(plural(item.sessions, "session"))}${item.running ? ` · ${item.running} running` : ""}</div>
            </button>`,
              )
              .join("")}</div>`
          : '<p class="home-none">Open a folder to add a project.</p>',
        state.projects.length,
      )}
    </div>`;
}

/// Every session by what its agent is doing, as the macOS Agent Dashboard.
export function dashboardView() {
  const needle = (state.dashboardFilter ?? "").toLowerCase();
  const sessions = state.allSessions.filter(
    (item) => !needle || `${item.title} ${ownerOf(item)?.title ?? ""}`.toLowerCase().includes(needle),
  );
  const columns = [
    ["Needs you", "waiting", sessions.filter(waiting)],
    ["Working", "running", sessions.filter((item) => isRunning(item.id) && !["waiting", "done"].includes(reported(item)))],
    ["Done", "done", sessions.filter((item) => reported(item) === "done")],
    ["Idle", "stopped", sessions.filter((item) => !isRunning(item.id))],
  ];
  return `
    <div class="home" data-scroll="dashboard">
      <div class="home__head">
        <h1 class="home__greeting">Agent Dashboard</h1>
        <input id="dashboard-filter" class="home__filter" type="search" placeholder="Filter sessions and projects"
               value="${escape(state.dashboardFilter ?? "")}" spellcheck="false" />
        <span class="section__spacer"></span>
        ${button({ label: "Home", icon: "home", action: "home" })}
      </div>
      <div class="board board--dashboard">
        ${columns
          .map(
            ([title, tone, items]) => `
          <div class="board__column">
            <div class="board__head">
              <span class="dot dot--${tone}"></span>
              <span class="board__title">${escape(title)}</span>
              <span class="board__count">${items.length}</span>
            </div>
            <div class="board__cards">${items.length ? items.map(sessionCard).join("") : '<div class="board__empty">None</div>'}</div>
          </div>`,
          )
          .join("")}
      </div>
    </div>`;
}
