import { test } from "node:test";
import assert from "node:assert/strict";
import { foldStages, stageIndex } from "../../src/stages.js";

const tasks = [
  {
    id: "t1", project_id: "p", title: "Fix login", session_id: "b1",
    plan: { session_id: "plan1" }, review: { session_id: "rev2" }, qa: { session_id: "qa2" },
    publish_run: { session_id: "pr1" }, docs_run: { session_id: "doc1" },
  },
  { id: "t2", project_id: "p", title: "Add export", session_id: null, plan: { session_id: "plan2" } },
];
const session = (id, extra = {}) => ({ id, project_id: "p", title: id, ...extra });

test("a task's sessions, from its runs, links and titles", () => {
  const sessions = [
    session("b1"), session("rev1", { review_of: "b1" }), session("qa1", { qa_of: "t1" }),
    session("old-plan", { title: "Plan: Fix login" }), session("lone"), session("manual", { review_of: "lone" }),
  ];
  const index = stageIndex(tasks, sessions);
  const of = (id) => index.get(id);
  assert.deepEqual(of("b1"), { taskId: "t1", stage: "build" });
  assert.deepEqual(of("plan1"), { taskId: "t1", stage: "plan" });
  assert.deepEqual(of("rev2"), { taskId: "t1", stage: "review" });
  assert.deepEqual(of("qa2"), { taskId: "t1", stage: "test" });
  assert.deepEqual(of("pr1"), { taskId: "t1", stage: "publish" });
  assert.deepEqual(of("doc1"), { taskId: "t1", stage: "document" });
  // Older rounds: a reviewer of the builder, a QA agent of the task.
  assert.deepEqual(of("rev1"), { taskId: "t1", stage: "review" });
  assert.deepEqual(of("qa1"), { taskId: "t1", stage: "test" });
  assert.deepEqual(of("old-plan"), { taskId: "t1", stage: "plan" });
  // A session of no task, and a review of one, stay their own.
  assert.equal(of("lone"), undefined);
  assert.equal(of("manual"), undefined);
});

test("the sidebar folds a task's sessions into its builder's row", () => {
  const sessions = [session("b1"), session("rev2"), session("qa2"), session("lone"), session("plan2")];
  const rows = foldStages(sessions, stageIndex(tasks, sessions));
  assert.deepEqual(rows.map(({ item }) => item.id), ["b1", "lone", "plan2"]);
  assert.deepEqual(rows[0].task, { taskId: "t1", stages: [sessions[1], sessions[2]] });
  // Before it is built, a task's row is its newest session: here its planner.
  assert.deepEqual(rows[2].task, { taskId: "t2", stages: [] });
  assert.equal(rows[1].task, null);
});
