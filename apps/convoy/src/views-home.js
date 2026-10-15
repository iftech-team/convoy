// Home and the Agent Dashboard: views over every project at once, as the
// macOS app has them.

import { agentIcon, icons, projectIcon } from "./icons.js";
import { button, escape, plural } from "./ui.js";
import { isRunning, phaseOf, sessionState, state } from "./state.js";
import { sessionLabel, sessionTone } from "./session-state.js";
import { limitsBlock } from "./views.js";
import { t } from "./i18n.js";
import { exact, nextAutomation, relative } from "./views-automations.js";
import { checklist, PITCH } from "./onboarding.js";
import { PHASES } from "./phase.js";
import { phaseLine } from "./views-planning.js";
import { flow } from "./views-welcome.js";

const ownerOf = (item) => state.projects.find((entry) => entry.id === item.project_id);
const reported = (item) => (isRunning(item.id) ? state.agentState.get(item.id) : null);
const waiting = (item) => reported(item) === "waiting";

function greeting() {
  const hour = new Date().getHours();
  return t(hour < 5 ? "Good night" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening");
}

/// The first-run checklist from what the window has read.
export const gettingStarted = () =>
  checklist({ diagnostics: state.diagnostics, setupChecks: state.setupChecks, projects: state.projects, tasks: state.allTasks });

/// The checklist as a card: each step, done or with the button that does
/// it. `dismissible` offers to put it away.
export function checklistCard({ dismissible = true } = {}) {
  const list = gettingStarted();
  const steps = list.items
    .map(
      (item) => `
      <li class="checklist__item${item.done ? " checklist__item--done" : ""}" data-check="${item.key}">
        <span class="checklist__mark" aria-label="${escape(item.done ? t("Done") : t("Not yet"))}">${item.done ? icons.check : ""}</span>
        <span class="checklist__text">
          <span class="checklist__label">${escape(t(item.label))}</span>
          <span class="checklist__detail">${escape(t(item.detail[0], item.detail[1]))}</span>
        </span>
        ${item.done ? "" : button({ label: item.button ? t(item.button[0], item.button[1]) : t(item.label), kind: "quiet", data: { "checklist-do": item.action, "checklist-agent": item.agent ?? "" } })}
      </li>`,
    )
    .join("");
  return `
    <section class="checklist" aria-label="${escape(t("Get started"))}">
      <div class="checklist__head">
        <h3 class="checklist__title">${escape(t("Get started"))}</h3>
        <span class="checklist__count">${escape(t("{done} of {total} done", { done: list.done, total: list.total }))}</span>
        <span class="section__spacer"></span>
        ${dismissible ? button({ icon: "close", kind: "quiet", title: t("Hide"), action: "checklist-dismiss" }) : ""}
      </div>
      <ol class="checklist__items">${steps}</ol>
    </section>`;
}

/// One session as a card: its state, project and agent, and the one action
/// that fits — open it, or resume it.
function sessionCard(item) {
  const owner = ownerOf(item);
  const now = sessionState(item);
  const label = t(sessionLabel(now));
  const tone = sessionTone(now);
  return `
    <div class="home-card" data-open="${escape(item.id)}" data-menu-session="${escape(item.id)}">
      <div class="home-card__head">
        <span class="dot dot--${tone}"></span>
        <span class="home-card__title">${escape(item.title)}</span>
        ${agentIcon(item.agent, 13)}
      </div>
      <div class="home-card__meta">${escape([owner?.title, label, item.branch ? `⎇ ${item.branch}` : ""].filter(Boolean).join(" · "))}</div>
      ${
        isRunning(item.id)
          ? ""
          : `<div class="home-card__actions">${button({ label: item.started ? t("Resume") : t("Start"), icon: "play", data: { start: item.id } })}</div>`
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

/// A task that needs you, as a card: where it stands and its project.
function taskCard(task) {
  const owner = ownerOf(task);
  return `
    <div class="home-card home-card--task" data-task="${escape(task.id)}">
      <div class="home-card__head">
        <span class="dot dot--review"></span>
        <span class="home-card__title">${escape(task.title)}</span>
        ${agentIcon(task.agent, 13)}
      </div>
      <div class="home-card__meta">${escape([owner?.title, phaseLine(phaseOf(task))].filter(Boolean).join(" · "))}</div>
    </div>`;
}

/// Every project's tasks by phase, left to right as a task moves: the
/// factory floor at a glance. Each phase opens the task list.
function pipeline(tasks) {
  const count = (phase) => tasks.filter((task) => phaseOf(task).phase === phase).length;
  return `
    <div class="pipeline" role="list" aria-label="${escape(t("Tasks in every project"))}">
      ${PHASES.map(
        ([phase, name, tone]) => `
        <button class="pipeline__phase pipeline__phase--${tone}${count(phase) ? "" : " pipeline__phase--none"}" role="listitem" data-home-tasks="${phase}">
          <strong>${count(phase)}</strong><span>${escape(t(name))}</span>
        </button>`,
      ).join('<span class="pipeline__arrow" aria-hidden="true">→</span>')}
    </div>`;
}

export function homeView() {
  const sessions = state.allSessions;
  const needs = sessions.filter(waiting);
  const running = sessions.filter((item) => isRunning(item.id) && !waiting(item));
  // Most recently created first; the ones on screen above are left out.
  const recent = [...sessions].reverse().filter((item) => !isRunning(item.id)).slice(0, 6);
  const tasks = state.allTasks ?? [];
  const waitingTasks = tasks.filter((task) => phaseOf(task).phase === "needs_you");
  // Tools not found, and every other check that is not fine, as on macOS.
  // A missing gh is counted once, not again as a missing gh login.
  const noGh = (state.diagnostics ?? []).some((check) => check.name === "gh" && !check.found);
  const missing =
    (state.diagnostics ?? []).filter((check) => !check.found).length +
    (state.setupChecks ?? []).filter((check) => check.status !== "ok" && !(noGh && check.id === "gh")).length;
  // Until the first run is done, its checklist stands in for the problems.
  const showChecklist = !state.checklistDismissed && !gettingStarted().complete;
  // How a task moves through Convoy, until there are tasks to show it.
  const showFlow = state.homeFlow ?? !tasks.length;
  const limits = state.limits ?? {};
  const activity = (state.activity ?? []).slice(0, 6);
  const upcoming = nextAutomation();
  const needCount = needs.length + waitingTasks.length;

  return `
    <div class="home" data-scroll="home">
      <div class="home__head">
        <h1 class="home__greeting">${escape(greeting())}</h1>
        <div class="home__summary">
          ${escape(t("{agents} running", { agents: plural(running.length + needs.length, "agent") }))}${needCount ? ` · <strong>${escape(plural(needCount, "needs you", "need you"))}</strong>` : ""}
          ${missing && !showChecklist ? `<button class="chip chip--warn" data-settings-open="Get started">${icons.warn} ${escape(plural(missing, "setup problem"))}</button>` : ""}
          ${upcoming ? `<button class="home__next" data-auto="tab" data-auto-project="${escape(upcoming.project_id)}" title="${escape(exact(upcoming.next_run))}">${icons.clock}<span>${escape(t("Next automation: {title}, {when}", { title: upcoming.title, when: relative(upcoming.next_run) }))}</span></button>` : ""}
        </div>
        <span class="section__spacer"></span>
        ${button({ label: t("How it works"), kind: "quiet", action: "home-flow" })}
        ${button({ label: t("Usage"), icon: "limits", action: "usage-page" })}
        ${button({ label: t("Agent Dashboard"), icon: "board", action: "dashboard" })}
        ${state.projects.length ? button({ label: t("New task"), icon: "plus", action: "home-new-task", kind: "primary" }) : ""}
      </div>
      ${showFlow ? `<section class="home-flow"><p class="home-flow__lead">${escape(t(PITCH))}</p>${flow()}</section>` : ""}
      ${showChecklist ? checklistCard() : ""}
      ${block(t("Tasks"), pipeline(tasks), tasks.length)}
      ${
        needCount
          ? block(
              t("Needs you"),
              `<div class="home-grid">${waitingTasks.map(taskCard).join("")}${needs.map(sessionCard).join("")}</div>`,
              needCount,
            )
          : ""
      }
      ${block(t("Running"), cards(running, t("Nothing is running.")), running.length)}
      <div class="home-columns">
        ${block(t("Usage limits"), `<div class="home-limits">${limitsBlock("Claude", "claude", limits.claude, limits.loading)}${limitsBlock("Codex", "codex", limits.codex, limits.loading)}</div>`)}
        ${block(
          t("Activity"),
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
            : `<p class="home-none">${escape(t("Nothing yet."))}</p>`,
        )}
      </div>
      ${block(t("Recent sessions"), cards(recent, t("No sessions yet. Tasks start them, or start one with New session.")))}
      ${block(
        t("Projects"),
        state.projects.length
          ? `<div class="home-grid">${state.projects
              .map(
                (item) => `
            <button class="home-card home-card--project" data-project="${escape(item.id)}">
              <div class="home-card__head">${projectIcon(item)}<span class="home-card__title">${escape(item.title)}</span></div>
              <div class="home-card__meta">${escape(plural(item.sessions, "session"))}${item.running ? ` · ${escape(t("{count} running", { count: item.running }))}` : ""}</div>
            </button>`,
              )
              .join("")}</div>`
          : `<p class="home-none">${escape(t("Open a folder to add a project."))}</p>`,
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
    ["Needs you", "waiting", sessions.filter((item) => sessionState(item) === "needs_you")],
    ["Working", "running", sessions.filter((item) => sessionState(item) === "working")],
    ["Idle", "done", sessions.filter((item) => ["finished", "idle"].includes(sessionState(item)))],
    ["Not running", "stopped", sessions.filter((item) => !isRunning(item.id))],
  ];
  return `
    <div class="home" data-scroll="dashboard">
      <div class="home__head">
        <h1 class="home__greeting">${escape(t("Agent Dashboard"))}</h1>
        <input id="dashboard-filter" class="home__filter" type="search" placeholder="${escape(t("Filter sessions and projects"))}"
               value="${escape(state.dashboardFilter ?? "")}" spellcheck="false" />
        <span class="section__spacer"></span>
        ${button({ label: t("Home"), icon: "home", action: "home" })}
      </div>
      <div class="board board--dashboard">
        ${columns
          .map(
            ([title, tone, items]) => `
          <div class="board__column">
            <div class="board__head">
              <span class="dot dot--${tone}"></span>
              <span class="board__title">${escape(t(title))}</span>
              <span class="board__count">${items.length}</span>
            </div>
            <div class="board__cards">${items.length ? items.map(sessionCard).join("") : `<div class="board__empty">${escape(t("None"))}</div>`}</div>
          </div>`,
          )
          .join("")}
      </div>
    </div>`;
}
