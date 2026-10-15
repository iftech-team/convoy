// Which sessions belong to a task, and as which step. A task runs as several
// sessions — its builder, and a planner, reviewer, QA agent, PR author and
// doc writer — and a session's menu offers to open its task.
//
// Only the builder carries the task's id. The others are found from the
// task's own runs, from the links a reviewer (review_of) and a QA agent
// (qa_of) keep, and, for older planners, PR authors and doc writers whose
// run has moved on, from the title the core gives them.
//
// Pure: no imports.

const TITLED = [
  ["Plan: ", "plan"],
  ["PR: ", "publish"],
  ["Docs: ", "document"],
];

/// sessionId → { taskId, stage }, stage one of build, plan, review, test,
/// publish, document.
export function stageIndex(tasks, sessions) {
  const index = new Map();
  const put = (id, taskId, stage) => {
    if (id && !index.has(id)) index.set(id, { taskId, stage });
  };
  const byBuilder = new Map();
  for (const task of tasks ?? []) {
    put(task.session_id, task.id, "build");
    put(task.plan?.session_id, task.id, "plan");
    put(task.review?.session_id, task.id, "review");
    put(task.review_session_id, task.id, "review");
    put(task.qa?.session_id, task.id, "test");
    put(task.publish_run?.session_id, task.id, "publish");
    put(task.docs_run?.session_id, task.id, "document");
    if (task.session_id) byBuilder.set(task.session_id, task);
  }
  const byTitle = new Map();
  for (const task of tasks ?? []) {
    for (const [prefix, stage] of TITLED) byTitle.set(`${task.project_id}\n${prefix}${task.title}`, [task.id, stage]);
  }
  for (const item of sessions ?? []) {
    if (index.has(item.id)) continue;
    if (item.task_id) put(item.id, item.task_id, "build");
    else if (item.review_of && byBuilder.has(item.review_of)) put(item.id, byBuilder.get(item.review_of).id, "review");
    else if (item.qa_of && (tasks ?? []).some((task) => task.id === item.qa_of)) put(item.id, item.qa_of, "test");
    else {
      const titled = byTitle.get(`${item.project_id}\n${item.title}`);
      if (titled) put(item.id, ...titled);
    }
  }
  return index;
}
