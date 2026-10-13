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
  ["testing", "In testing", "testing"],
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
        <button data-task-view="list" aria-pressed="${state.taskView === "list"}">${escape(t("List"))}</button>
        <button data-task-view="board" aria-pressed="${state.taskView !== "list"}">${escape(t("Board"))}</button>
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
      ${button({ label: t("Import tasks"), action: "import" })}
      ${button({ label: t("Integrations"), action: "integrations" })}
      ${button({ label: t("New task"), icon: "plus", action: "new-task", kind: "primary" })}
    </div>`;

  if (!tasks.length) {
    return `${head}${empty("board", t("No tasks yet"), t("Add one to queue work for an agent."))}`;
  }
  return `${head}${state.taskView === "list" ? taskList(tasks) : board(tasks)}`;
}

/// Tasks that can be started from here: waiting, or back after a failure or
/// a request for changes.
export const runnable = (task) => ["queued", "failed", "changes"].includes(task.status);

const projectName = (task) =>
  state.taskScope === "all" ? state.projects.find((item) => item.id === task.project_id)?.title : "";

/// One line per task, as in Linear: status ring, key, title, then what sets
/// it apart on the right. Rows drag onto another group like board cards.
function taskRow(task, tone) {
  const owner = projectName(task);
  const key = task.source?.key ?? "";
  const tags = [
    owner ? `<span class="badge badge--muted">${escape(owner)}</span>` : "",
    task.blocked_by?.length
      ? `<span class="badge task-blocked" title="${escape(t("Waits for {tasks}", { tasks: task.blocked_by.join(", ") }))}">${escape(t("blocked"))}</span>`
      : "",
    task.auto_review ? `<span class="badge badge--muted">${escape(t("auto review"))}</span>` : "",
    `<span class="badge badge--muted">${escape(t(MODES[task.mode] ?? task.mode))}</span>`,
  ].join("");
  return `
    <div class="task-row" data-task="${escape(task.id)}" data-task-menu="${escape(task.id)}" data-task-card="${escape(task.id)}">
      <span class="task-row__ring task-row__ring--${tone}"></span>
      <span class="task-row__key">${escape(key)}</span>
      <span class="task-row__title">${escape(task.title)}</span>
      ${task.last_error ? `<span class="task-row__error" title="${escape(task.last_error)}">${escape(task.last_error)}</span>` : ""}
      <span class="task-row__tags">${tags}</span>
      <span class="task-row__agent" title="${escape(task.model || "")}">${task.agent ? agentIcon(task.agent, 14) : ""}</span>
      <span class="task-row__actions">
        ${task.pr_url ? button({ icon: "open", kind: "quiet", title: t("Pull request"), data: { "task-pr": task.id } }) : ""}
        ${task.source?.url ? button({ icon: "open", kind: "quiet", title: t("Open {name}", { name: task.source.key }), data: { "issue-source": task.id } }) : ""}
        ${runnable(task) ? button({ icon: "play", kind: "quiet", title: t("Run"), data: { "task-run": task.id } }) : ""}
        ${button({ icon: "more", kind: "quiet", title: t("Task actions"), data: { "task-more": task.id } })}
      </span>
    </div>`;
}

/// Grouped by status as in Linear: a bar per group that folds shut, empty
/// groups left out. A group takes rows dropped on it.
function taskList(tasks) {
  return `<div class="task-list">${COLUMNS.filter(([key]) => !(state.hideDone && key === "done"))
    .map(([key, label, tone]) => {
      const group = tasks.filter((task) => task.status === key);
      if (!group.length) return "";
      const folded = state.foldedStatuses?.has(key);
      return `
        <section class="task-list__group" data-task-column="${key}">
          <button class="task-group" data-task-group="${key}" aria-expanded="${!folded}">
            <span class="task-group__chevron">${folded ? icons.chevronRight : icons.chevronDown}</span>
            <span class="state__dot state__dot--${tone}"></span>
            <span class="task-group__title">${escape(t(label))}</span>
            <span class="board__count">${group.length}</span>
          </button>
          ${folded ? "" : group.map((task) => taskRow(task, tone)).join("")}
        </section>`;
    })
    .join("")}</div>`;
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
                        data-task-card="${escape(task.id)}">
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
