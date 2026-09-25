// Specifications, tasks and the queue.
//
// A specification is revised and approved; a task references the revision it
// was written against; a session is prepared from both. A finished task
// reaches review, never done — accepting work stays a decision a person makes.

import { button, empty, escape, plural } from "./ui.js";
import { state } from "./state.js";
import { agentIcon } from "./icons.js";

const COLUMNS = [
  ["queued", "Queued", "idle"],
  ["building", "Running", "running"],
  ["review", "Needs review", "review"],
  ["changes", "Needs changes", "open"],
  ["done", "Done", "done"],
  ["failed", "Failed", "failed"],
];

const MODES = { none: "no publishing", pr: "pull request", push: "push" };

export function specsView() {
  const planning = state.planning;
  if (!planning) return '<div class="list__none">Reading…</div>';
  if (!planning.specs.length) {
    return `
      ${empty(
        "list",
        "No specifications yet",
        "A task can run without one, but a specification is what makes acceptance checkable.",
      )}
      <div class="centre-action">
        ${button({ label: "New specification", icon: "plus", action: "new-spec", kind: "primary" })}
      </div>`;
  }
  return `
    <div class="section">
      <h2 class="section__title">Specifications
        <span class="section__count">${planning.specs.length}</span>
      </h2>
      <span class="section__spacer"></span>
      ${button({ label: "New specification", icon: "plus", action: "new-spec", kind: "primary" })}
    </div>
    <div class="list">
      ${planning.specs
        .map(
          (spec) => `
        <div class="row" data-spec="${escape(spec.id)}">
          <span class="row__mark">§</span>
          <div class="row__body">
            <div class="row__title">${escape(spec.title)}</div>
            <div class="row__meta">
              Revision ${spec.revision} · ${spec.approved ? "approved" : "draft"}
              · ${plural(planning.tasks.filter((task) => task.spec_id === spec.id).length, "task")}
            </div>
          </div>
          <div class="row__actions">
            ${spec.approved ? "" : button({ label: "Approve", data: { approve: spec.id, revision: spec.revision } })}
            ${button({ label: "Export", icon: "download", data: { export: spec.id } })}
            ${button({ icon: "open", kind: "quiet", data: { spec: spec.id } })}
          </div>
        </div>`,
        )
        .join("")}
    </div>`;
}

export function tasksView() {
  const planning = state.planning;
  if (!planning) return '<div class="list__none">Reading…</div>';
  const running = state.queues.has(state.projectId);

  const head = `
    <div class="section">
      <h2 class="section__title">Tasks
        <span class="section__count">${planning.tasks.length}</span>
      </h2>
      <div class="segmented">
        <button data-task-view="list" aria-pressed="${state.taskView !== "board"}">List</button>
        <button data-task-view="board" aria-pressed="${state.taskView === "board"}">Board</button>
      </div>
      <span class="section__spacer"></span>
      <span class="section__count">${planning.queued} queued</span>
      ${button({
        label: running ? "Pause queue" : "Run queue",
        icon: running ? "stop" : "play",
        action: running ? "queue-stop" : "queue-start",
        kind: running ? "danger" : "",
        disabled: !running && planning.queued === 0,
      })}
      ${button({ label: "Import issues", action: "import" })}
      ${button({ label: "Linear & Jira", action: "integrations" })}
      ${button({ label: "New task", icon: "plus", action: "new-task", kind: "primary" })}
    </div>`;

  if (!planning.tasks.length) {
    return `${head}${empty("board", "No tasks yet", "Add one to queue work for an agent.")}`;
  }
  return `${head}${state.taskView === "board" ? board(planning) : taskList(planning)}`;
}

function taskList(planning) {
  return `
    <div class="list">
      ${planning.tasks
        .map(
          (task) => `
        <div class="row" data-task="${escape(task.id)}">
          <span class="row__mark">${task.agent ? agentIcon(task.agent, 16) : "◇"}</span>
          <div class="row__body">
            <div class="row__title">${escape(task.title)}</div>
            <div class="row__meta">
              ${task.source ? escape(`${task.source.tracker === "jira" ? "Jira" : "Linear"} ${task.source.key}`) + " · " : ""}${task.model ? escape(task.model) + " · " : ""}${escape(task.status)} · ${escape(MODES[task.mode] ?? task.mode)}
              ${task.auto_review ? " · automatic review" : ""}
              ${task.last_error ? ` · ${escape(task.last_error)}` : ""}
            </div>
          </div>
          <div class="row__actions">
            ${task.source?.url ? button({ label: task.source.key, data: { "issue-source": task.id } }) : ""}
            ${button({ label: "Prepare", data: { prepare: task.id }, disabled: task.status === "building" })}
            ${button({ icon: "open", kind: "quiet", data: { task: task.id } })}
          </div>
        </div>`,
        )
        .join("")}
    </div>`;
}

function board(planning) {
  return `
    <div class="board">
      ${COLUMNS.map(([key, label, tone]) => {
        const cards = planning.tasks.filter((task) => task.status === key);
        return `
          <div class="board__column">
            <div class="board__head">
              <span class="state__dot state__dot--${tone}"></span>
              <span class="board__title">${escape(label)}</span>
              <span class="board__count">${cards.length}</span>
            </div>
            <div class="board__cards">
              ${
                cards.length
                  ? cards
                      .map(
                        (task) => `
                <button class="card" data-task="${escape(task.id)}">
                  <div class="card__title">${escape(task.title)}</div>
                  ${task.details ? `<div class="card__body">${escape(task.details)}</div>` : ""}
                  <div class="card__foot">
                    <span class="badge--muted badge">${escape(MODES[task.mode] ?? task.mode)}</span>
                    ${task.auto_review ? '<span class="badge--muted badge">auto review</span>' : ""}
                  </div>
                </button>`,
                      )
                      .join("")
                  : '<div class="board__empty">No tasks</div>'
              }
            </div>
          </div>`;
      }).join("")}
    </div>`;
}

