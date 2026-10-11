// Specifications, tasks and the queue.
//
// A specification is revised and approved; a task references the revision it
// was written against; a session is prepared from both. A finished task
// reaches review, never done — accepting work stays a decision a person makes.

import { button, empty, escape, plural } from "./ui.js";
import { state } from "./state.js";
import { agentIcon, icons } from "./icons.js";
import { t } from "./i18n.js";

const COLUMNS = [
  ["queued", "Queued", "idle"],
  ["building", "Running", "running"],
  ["review", "Needs review", "review"],
  ["pr", "Pull request", "review"],
  ["changes", "Needs changes", "open"],
  ["done", "Done", "done"],
  ["failed", "Failed", "failed"],
];

const MODES = { none: "no publishing", pr: "pull request", push: "push" };

export function specsView() {
  const planning = state.planning;
  if (!planning) return `<div class="list__none">${escape(t("Reading…"))}</div>`;
  if (!planning.specs.length) {
    return `
      ${empty(
        "list",
        t("No specifications yet"),
        t("A task can run without one, but a specification is what makes acceptance checkable."),
      )}
      <div class="centre-action">
        ${button({ label: t("New specification"), icon: "plus", action: "new-spec", kind: "primary" })}
      </div>`;
  }
  const needle = (state.specFilter ?? "").toLowerCase();
  const specs = planning.specs.filter(
    (spec) => !needle || `${spec.title} ${spec.problem} ${spec.requirements}`.toLowerCase().includes(needle),
  );
  return `
    <div class="section">
      <h2 class="section__title">${escape(t("Specifications"))}
        <span class="section__count">${planning.specs.length}</span>
      </h2>
      <input id="spec-filter" class="section__filter" type="search" placeholder="${escape(t("Find a specification"))}"
             value="${escape(state.specFilter ?? "")}" spellcheck="false" />
      <span class="section__spacer"></span>
      ${button({ label: t("New specification"), icon: "plus", action: "new-spec", kind: "primary" })}
    </div>
    <div class="list">
      ${specs
        .map(
          (spec) => `
        <div class="row" data-spec="${escape(spec.id)}">
          <span class="row__mark">§</span>
          <div class="row__body">
            <div class="row__title">${escape(spec.title)}</div>
            <div class="row__meta">
              ${escape(spec.approved ? t("Revision {revision} · approved", { revision: spec.revision }) : t("Revision {revision} · draft", { revision: spec.revision }))}
              · ${plural(planning.tasks.filter((task) => task.spec_id === spec.id).length, "task")}
            </div>
          </div>
          <div class="row__actions">
            ${spec.approved ? "" : button({ label: t("Approve"), data: { approve: spec.id, revision: spec.revision } })}
            ${button({ label: t("Export"), icon: "download", data: { export: spec.id } })}
            ${button({ icon: "open", kind: "quiet", data: { spec: spec.id } })}
          </div>
        </div>`,
        )
        .join("")}
    </div>`;
}

/// The tasks shown: this project's, or every project's.
export const shownTasks = () =>
  state.taskScope === "all" ? (state.allTasks ?? []) : (state.planning?.tasks ?? []);

export function tasksView() {
  const planning = state.planning;
  if (!planning) return `<div class="list__none">${escape(t("Reading…"))}</div>`;
  const running = state.queues.has(state.projectId);
  const tasks = shownTasks();

  const head = `
    <div class="section">
      <h2 class="section__title">${escape(t("Tasks"))}
        <span class="section__count">${tasks.length}</span>
      </h2>
      <div class="segmented">
        <button data-task-view="list" aria-pressed="${state.taskView !== "board"}">${escape(t("List"))}</button>
        <button data-task-view="board" aria-pressed="${state.taskView === "board"}">${escape(t("Board"))}</button>
      </div>
      <div class="segmented">
        <button data-task-scope="project" aria-pressed="${state.taskScope !== "all"}">${escape(t("This project"))}</button>
        <button data-task-scope="all" aria-pressed="${state.taskScope === "all"}">${escape(t("All projects"))}</button>
      </div>
      <label class="check"><input type="checkbox" data-toggle-state="hideDone" ${state.hideDone ? "checked" : ""} /> ${escape(t("Hide done"))}</label>
      <span class="section__spacer"></span>
      <span class="section__count">${escape(t("{count} queued", { count: planning.queued }))}</span>
      ${button({
        label: running ? t("Pause queue") : t("Run queue"),
        icon: running ? "stop" : "play",
        action: running ? "queue-stop" : "queue-start",
        kind: running ? "danger" : "",
        disabled: !running && planning.queued === 0,
      })}
      ${button({ label: t("Import issues"), action: "import" })}
      ${button({ label: t("Linear & Jira"), action: "integrations" })}
      ${button({ label: t("New task"), icon: "plus", action: "new-task", kind: "primary" })}
    </div>`;

  if (!tasks.length) {
    return `${head}${empty("board", t("No tasks yet"), t("Add one to queue work for an agent."))}`;
  }
  return `${head}${state.taskView === "board" ? board(tasks) : taskList(tasks)}`;
}

/// Tasks that can be started from here: waiting, or back after a failure or
/// a request for changes.
export const runnable = (task) => ["queued", "failed", "changes"].includes(task.status);

const projectName = (task) =>
  state.taskScope === "all" ? state.projects.find((item) => item.id === task.project_id)?.title : "";

function taskRow(task) {
  const owner = projectName(task);
  return `
    <div class="row" data-task="${escape(task.id)}" data-task-menu="${escape(task.id)}">
      <span class="row__mark">${task.agent ? agentIcon(task.agent, 16) : "◇"}</span>
      <div class="row__body">
        <div class="row__title">${escape(task.title)}</div>
        <div class="row__meta">
          ${owner ? `${escape(owner)} · ` : ""}${task.source ? escape(`${task.source.tracker === "jira" ? "Jira" : "Linear"} ${task.source.key}`) + " · " : ""}${task.model ? escape(task.model) + " · " : ""}${escape(t(MODES[task.mode] ?? task.mode))}
          ${task.auto_review ? ` · ${escape(t("automatic review"))}` : ""}
          ${task.last_error ? ` · ${escape(task.last_error)}` : ""}
          ${task.blocked_by?.length ? ` · <span class="task-blocked">${escape(t("waits for {tasks}", { tasks: task.blocked_by.join(", ") }))}</span>` : ""}
        </div>
      </div>
      <div class="row__actions">
        ${task.pr_url ? button({ label: t("Pull request"), icon: "open", data: { "task-pr": task.id } }) : ""}
        ${task.source?.url ? button({ label: task.source.key, data: { "issue-source": task.id } }) : ""}
        ${runnable(task) ? button({ label: t("Run"), icon: "play", data: { "task-run": task.id } }) : ""}
        ${button({ icon: "more", kind: "quiet", title: t("Task actions"), data: { "task-more": task.id } })}
      </div>
    </div>`;
}

/// Grouped by status as on macOS, each group folding shut.
function taskList(tasks) {
  return COLUMNS.filter(([key]) => !(state.hideDone && key === "done"))
    .map(([key, label, tone]) => {
      const group = tasks.filter((task) => task.status === key);
      if (!group.length) return "";
      const folded = state.foldedStatuses?.has(key);
      return `
        <button class="task-group" data-task-group="${key}" aria-expanded="${!folded}">
          <span class="task-group__chevron">${folded ? icons.chevronRight : icons.chevronDown}</span>
          <span class="state__dot state__dot--${tone}"></span>
          <span class="task-group__title">${escape(t(label))}</span>
          <span class="board__count">${group.length}</span>
        </button>
        ${folded ? "" : `<div class="list">${group.map(taskRow).join("")}</div>`}`;
    })
    .join("");
}

/// Cards drag between columns: onto Running runs the task, onto Queued puts
/// it back, and anywhere else sets that status.
function board(tasks) {
  return `
    <div class="board">
      ${COLUMNS.filter(([key]) => !(state.hideDone && key === "done")).map(([key, label, tone]) => {
        const cards = tasks.filter((task) => task.status === key);
        return `
          <div class="board__column" data-task-column="${key}">
            <div class="board__head">
              <span class="state__dot state__dot--${tone}"></span>
              <span class="board__title">${escape(t(label))}</span>
              <span class="board__count">${cards.length}</span>
            </div>
            <div class="board__cards">
              ${
                cards.length
                  ? cards
                      .map(
                        (task) => `
                <button class="card" data-task="${escape(task.id)}" data-task-menu="${escape(task.id)}"
                        draggable="true" data-task-card="${escape(task.id)}">
                  <div class="card__title">${escape(task.title)}</div>
                  ${projectName(task) ? `<div class="card__project">${escape(projectName(task))}</div>` : ""}
                  ${task.details ? `<div class="card__body">${escape(task.details)}</div>` : ""}
                  <div class="card__foot">
                    <span class="badge--muted badge">${escape(t(MODES[task.mode] ?? task.mode))}</span>
                    ${task.auto_review ? `<span class="badge--muted badge">${escape(t("auto review"))}</span>` : ""}
                    ${task.pr_url ? '<span class="badge--muted badge">PR</span>' : ""}
                    ${task.blocked_by?.length ? `<span class="badge task-blocked" title="${escape(t("Waits for {tasks}", { tasks: task.blocked_by.join(", ") }))}">${escape(t("blocked"))}</span>` : ""}
                  </div>
                </button>`,
                      )
                      .join("")
                  : `<div class="board__empty">${escape(t("No tasks"))}</div>`
              }
            </div>
          </div>`;
      }).join("")}
    </div>`;
}

export const TASK_STATUSES = COLUMNS;
