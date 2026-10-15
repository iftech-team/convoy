// A task's own page: where it stands, then the road it takes — plan, build,
// review, test, publish, document — each step with how far it got, the
// session that ran it, and what it found. Before, this was a form with five
// panels stacked above the fields, and the sessions were only in menus.

import { button, escape } from "./ui.js";
import { anySession, phaseOf, sessionState, state, stepsOf } from "./state.js";
import { icons } from "./icons.js";
import { t } from "./i18n.js";
import { PHASES, STEP_STATES } from "./phase.js";
import { sessionLabel } from "./session-state.js";
import { phaseLine, runnable } from "./views-planning.js";
import { docsPanel, planPanel, publishPanel, qaPanel, reviewPanel, syncNotice } from "./dialogs.js";

/// The task the page shows, from whichever list holds it.
export const pageTask = () =>
  state.taskId ? (state.planning?.tasks.find((item) => item.id === state.taskId) ?? state.allTasks?.find((item) => item.id === state.taskId) ?? null) : null;

/// What a step found, under its line: the panels the form used to stack.
function stepDetail(step, task) {
  if (!step.on) return "";
  switch (step.key) {
    case "plan":
      return planPanel(task);
    case "build": {
      const parts = [];
      if (task.status === "failed" && task.last_error) parts.push(`<div class="qa-panel__findings">${escape(task.last_error)}</div>`);
      if (task.status === "changes" && task.findings) parts.push(`<details open><summary>${escape(t("Findings sent back"))}</summary><div class="qa-panel__findings">${escape(task.findings)}</div></details>`);
      if (task.build_summary) parts.push(`<details><summary>${escape(t("What the builder reports"))}</summary><div class="qa-panel__findings">${escape(task.build_summary)}</div></details>`);
      return parts.join("");
    }
    case "review":
      return task.review || task.round ? reviewPanel(task) : "";
    case "test":
      return qaPanel(task);
    case "publish":
      return publishPanel(task);
    case "document":
      return docsPanel(task);
    default:
      return "";
  }
}

/// A step's session, as a button that opens it, with its state.
function sessionButton(id) {
  const item = id ? anySession(id) : null;
  if (!item) return "";
  return `<button class="button button--quiet step__session" data-open="${escape(id)}" title="${escape(item.title)}">
      ${icons.terminal}<span>${escape(t("Open session"))}</span><span class="step__session-state">${escape(t(sessionLabel(sessionState(item))))}</span>
    </button>`;
}

function stepRow(step, task) {
  const detail = stepDetail(step, task);
  return `
    <li class="step step--${step.state}${step.current ? " step--current" : ""}" data-step="${step.key}">
      <span class="step__mark" aria-hidden="true">${step.state === "passed" ? icons.check : ""}</span>
      <div class="step__body">
        <div class="step__head">
          <span class="step__label">${escape(t(step.label))}</span>
          <span class="step__state">${escape(t(STEP_STATES[step.state]))}</span>
          <span class="section__spacer"></span>
          ${step.on ? sessionButton(step.session) : ""}
        </div>
        ${detail ? `<div class="step__detail">${detail}</div>` : ""}
      </div>
    </li>`;
}

/// The one thing to do next, from where the task stands.
function primaryAction(task, phase) {
  if (phase.action === "run" && runnable(task)) {
    return button({ label: task.plan_first && task.plan?.status !== "approved" ? t("Make a plan") : t("Run"), icon: "play", kind: "primary", data: { "task-run": task.id } });
  }
  if (phase.action === "mark-done") return button({ label: t("Mark done"), icon: "check", kind: "primary", action: "task-mark-done" });
  if (phase.action === "open-session" && phase.session) return button({ label: t("Open session"), icon: "terminal", kind: "primary", data: { open: phase.session } });
  return "";
}

export function taskPage() {
  const task = pageTask();
  if (!task) return "";
  const project = state.projects.find((item) => item.id === task.project_id);
  const phase = phaseOf(task);
  const [, phaseName] = PHASES.find(([key]) => key === phase.phase);
  const steps = stepsOf(task, project);
  return `
    <div class="task-page" data-scroll="task-page" data-task-page="${escape(task.id)}">
      <div class="task-page__head">
        <button class="link-button task-page__back" data-action="task-back">${icons.chevronLeft ?? "‹"} ${escape(t("Tasks"))}</button>
        <div class="task-page__title-row">
          <h2 class="task-page__title">
            ${task.source?.key && !task.title.startsWith(task.source.key) ? `<code class="task-page__key">${escape(task.source.key)}</code>` : ""}
            ${escape(task.title)}
          </h2>
          <div class="task-page__actions">
            ${primaryAction(task, phase)}
            ${button({ label: t("Edit…"), icon: "edit", action: "task-page-edit" })}
            ${button({ icon: "more", kind: "quiet", title: t("Task actions"), data: { "task-more": task.id } })}
          </div>
        </div>
        <div class="task-page__phase task-page__phase--${phase.phase}">
          <span class="state__dot state__dot--${phase.tone}"></span>
          <strong>${escape(t(phaseName))}</strong>
          <span>${escape(phaseLine(phase))}</span>
        </div>
        ${syncNotice(task)}
      </div>
      <ol class="steps">${steps.map((step) => stepRow(step, task)).join("")}</ol>
      ${
        task.details
          ? `<section class="task-page__section"><h3 class="task-page__heading">${escape(t("Details"))}</h3><div class="task-page__text">${escape(task.details)}</div></section>`
          : ""
      }
    </div>`;
}
