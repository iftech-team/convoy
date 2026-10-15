import { test } from "node:test";
import assert from "node:assert/strict";
import { PHASES, PHASE_LABELS, STEP_STATES, dropStatus, movable, prNumber, taskPhase, taskSteps } from "../../src/phase.js";
import { untranslated } from "./helpers.mjs";

const task = (status, extra = {}) => ({ id: "t", status, round: 0, max_rounds: 2, session_id: "builder", ...extra });
// Sessions as the window sees them: running, started, what the agent said.
const live = (sessions) => ({ session: (id) => sessions[id] ?? null });
const at = (phase, label, extra = {}) => ({ phase, label, ...extra });
const check = (got, want) => {
  for (const [key, value] of Object.entries(want)) assert.deepEqual(got[key], value, `${key}: ${JSON.stringify(got)}`);
};

test("done, failed and plans", () => {
  check(taskPhase(task("done")), at("done", "Done"));
  check(taskPhase(task("done", { pr_url: "https://github.com/a/b/pull/42" })), at("done", "Done · PR #{number}", { vars: { number: "42" } }));
  check(taskPhase(task("failed")), at("needs_you", "Builder stopped · run it again", { action: "run" }));
  check(taskPhase(task("plan_review", { plan: { status: "waiting", session_id: "p" } })), at("needs_you", "Approve the plan", { action: "approve-plan", step: "plan" }));
  check(taskPhase(task("planning", { plan: { status: "drafting", session_id: "p" } })), at("working", "Planning", { step: "plan" }));
  check(
    taskPhase(task("planning", { plan: { status: "drafting", session_id: "p" } }), live({ p: { running: true, started: true, agent: "waiting" } })),
    at("needs_you", "Planner is asking you"),
  );
  check(
    taskPhase(task("planning", { plan: { status: "drafting", session_id: "p" } }), live({ p: { running: false, started: true } })),
    at("needs_you", "Planner stopped · resume it"),
  );
});

test("waiting to start", () => {
  check(taskPhase(task("queued")), at("todo", "Waiting to start", { step: "build" }));
  check(taskPhase(task("queued", { plan_first: true })), at("todo", "Waiting to start", { step: "plan" }));
  check(taskPhase(task("queued", { blocked_by: ["A", "B"] })), at("todo", "Waits for {tasks}", { vars: { tasks: "A, B" } }));
  check(taskPhase(task("queued", { plan_first: true, plan: { status: "approved" } })), at("todo", "Plan approved · waiting to build", { step: "build" }));
  check(taskPhase(task("queued", { plan_first: true, plan: { status: "changes_requested" } })), at("todo", "Task changed · will plan again"));
});

test("building and fixing", () => {
  check(taskPhase(task("building")), at("working", "Building"));
  check(taskPhase(task("building", { round: 1 })), at("working", "Fixing · round {round}/{max}", { vars: { round: 1, max: 2 } }));
  check(taskPhase(task("building"), live({ builder: { running: true, started: true, agent: "waiting" } })), at("needs_you", "Builder is asking you"));
  // Sent back and its builder resuming: being fixed. Not running: nothing
  // picks it up but a person.
  check(taskPhase(task("changes", { round: 1 }), live({ builder: { running: true, started: true } })), at("working", "Fixing · round {round}/{max}"));
  check(taskPhase(task("changes"), live({ builder: { running: true, started: true } })), at("working", "Fixing"));
  check(taskPhase(task("changes", { findings: "Fix the test" }), live({ builder: { running: false, started: true } })), at("needs_you", "Fixes waiting · run to apply", { action: "run" }));
  check(taskPhase(task("changes")), at("needs_you", "Sent back · run it again"));
});

test("a run at work in review, pr or testing is working", () => {
  for (const status of ["review", "pr", "testing"]) {
    check(taskPhase(task(status, { publish_run: { status: "writing", session_id: "w" } })), at("working", "Writing the pull request", { step: "publish" }));
    check(taskPhase(task(status, { publish_run: { status: "publishing" } })), at("working", "Opening the pull request"));
    check(taskPhase(task(status, { docs_run: { status: "writing", session_id: "d" } })), at("working", "Writing the document", { step: "document" }));
    check(taskPhase(task(status, { qa: { status: "checking" } })), at("working", "Running checks", { step: "test" }));
    check(taskPhase(task(status, { qa: { status: "queued" } })), at("working", "Waiting for the test environment"));
    check(taskPhase(task(status, { qa: { status: "testing", session_id: "q" } })), at("working", "QA agent testing"));
    check(taskPhase(task(status, { review: { status: "reviewing", session_id: "r", round: 0 } })), at("working", "Reviewing", { step: "review" }));
    check(taskPhase(task(status, { round: 1, review: { status: "reviewing", session_id: "r", round: 1 } })), at("working", "Reviewing · fix {round}/{max}"));
    check(taskPhase(task(status, { qa: { status: "waiting" } })), at("working", "Tests after the review"));
  }
  // The app was closed while QA ran: it waits for someone to resume it.
  check(
    taskPhase(task("testing", { qa: { status: "testing", session_id: "q" } }), live({ q: { running: false, started: true } })),
    at("needs_you", "QA agent stopped · resume it", { session: "q", action: "open-session" }),
  );
  check(
    taskPhase(task("review", { review: { status: "reviewing", session_id: "r", round: 0 } }), live({ r: { running: true, started: true, agent: "waiting" } })),
    at("needs_you", "Reviewer is asking you"),
  );
  // A reviewer not started yet is not stalled.
  check(taskPhase(task("review", { review: { status: "reviewing", session_id: "r", round: 0 } }), live({ r: { running: false, started: false } })), at("working", "Reviewing"));
});

test("everything passed: ready, with what it went through", () => {
  check(taskPhase(task("review")), at("ready", "Built · check it", { action: "mark-done" }));
  const ready = taskPhase(task("testing", { pr_url: "https://gitlab.example.com/g/a/-/merge_requests/7", qa: { status: "passed" }, review: { status: "passed", round: 0 }, docs: [{}] }));
  check(ready, { phase: "ready", step: "publish" });
  assert.deepEqual(ready.parts.map(([label]) => label), ["PR #{number} open", "QA passed", "Reviewed", "Documented"]);
  assert.deepEqual(ready.parts[0][1], { number: "7" });
  check(taskPhase(task("review", { review: { status: "skipped", round: 0 } })), at("ready", "Review skipped", { step: "review" }));
  check(taskPhase(task("pr", { pr_url: "https://example.com/somewhere" })), at("ready", "PR open"));
  // A failed review of an earlier round is history, not a problem now.
  check(taskPhase(task("review", { round: 1, review: { status: "failed", round: 0 } })), at("ready", "Built · check it"));
  check(taskPhase(task("review", { review: { status: "failed", round: 0 } })), at("needs_you", "Review failed"));
  check(taskPhase(task("review", { review: { status: "no_verdict", round: 0 } })), at("needs_you", "Reviewer gave no verdict"));
});

test("needs attention says why, from the run that failed last", () => {
  check(taskPhase(task("attention", { plan: { status: "drafting" } })), at("needs_you", "Planner stopped without a plan", { step: "plan" }));
  check(taskPhase(task("attention", { publish_run: { status: "failed", finished: "2026-10-15T10:00:00Z" } })), at("needs_you", "Not published", { step: "publish" }));
  check(taskPhase(task("attention", { docs_run: { status: "failed", finished: "2026-10-15T10:00:00Z" } })), at("needs_you", "No document written"));
  check(taskPhase(task("attention", { qa: { status: "no_verdict", finished: "2026-10-15T10:00:00Z" } })), at("needs_you", "QA gave no verdict", { step: "test" }));
  check(
    taskPhase(task("attention", { round: 2, review: { status: "failed", finished: "2026-10-15T09:00:00Z" }, qa: { status: "failed", finished: "2026-10-15T11:00:00Z" } })),
    at("needs_you", "Out of fix rounds ({round}/{max})", { step: "test", vars: { round: 2, max: 2 } }),
  );
  check(
    taskPhase(task("attention", { review: { status: "no_verdict", finished: "2026-10-15T12:00:00Z" }, qa: { status: "failed", finished: "2026-10-15T11:00:00Z" } })),
    at("needs_you", "Reviewer gave no verdict"),
  );
  check(taskPhase(task("attention")), at("needs_you", "Needs a look"));
  check(taskPhase(task("bogus")), at("needs_you", "Unknown status {status}"));
});

test("moving by hand", () => {
  assert.equal(movable(taskPhase(task("building"))), false);
  assert.equal(movable(taskPhase(task("review"))), true);
  assert.equal(dropStatus(task("review"), "todo"), "queued");
  assert.equal(dropStatus(task("failed"), "ready"), "review");
  assert.equal(dropStatus(task("failed", { pr_url: "x" }), "ready"), "pr");
  assert.equal(dropStatus(task("review"), "done"), "done");
  assert.equal(dropStatus(task("review"), "needs_you"), null);
  assert.equal(dropStatus(task("review"), "working"), null);
});

test("pull request numbers", () => {
  assert.equal(prNumber("https://github.com/acme/app/pull/42"), "42");
  assert.equal(prNumber("https://gitlab.com/acme/app/-/merge_requests/7"), "7");
  assert.equal(prNumber("https://gitea.example.org/me/tool/pulls/4"), "4");
  assert.equal(prNumber("https://bitbucket.example.com/projects/AB/repos/app/pull-requests/12"), "12");
  assert.equal(prNumber(null), null);
});

test("every phase label has its Russian", () => {
  assert.equal(PHASES.length, 5);
  assert.deepEqual(untranslated(PHASE_LABELS), []);
});

test("a task's steps: which it uses, how far each got, which is current", () => {
  const states = (steps) => Object.fromEntries(steps.map((step) => [step.key, step.state]));
  // A plain task: only Build, not yet.
  assert.deepEqual(states(taskSteps(task("queued", { mode: "none" }))), { plan: "off", build: "pending", review: "off", test: "off", publish: "off", document: "off" });
  // Planned, reviewed, tested, published, documented.
  const full = taskSteps(
    task("testing", { mode: "pr", plan_first: true, auto_review: true, plan: { status: "approved" }, review: { status: "passed", round: 0 }, qa: { status: "passed" }, pr_url: "https://github.com/a/b/pull/1", docs: [{}] }),
    { qa: true, docs_auto: true },
  );
  assert.deepEqual(states(full), { plan: "passed", build: "passed", review: "passed", test: "passed", publish: "passed", document: "passed" });
  // The plan waits for you, and that step is the current one.
  const plan = taskSteps(task("plan_review", { plan_first: true, plan: { status: "waiting", session_id: "p" } }));
  assert.equal(plan[0].state, "you");
  assert.equal(plan.find((step) => step.current).key, "plan");
  // A QA agent that stopped shows as failed where it stopped.
  const stalled = taskSteps(task("testing", { qa: { status: "testing", session_id: "q" } }), { qa: true }, live({ q: { running: false, started: true } }));
  assert.equal(stalled.find((step) => step.key === "test").state, "failed");
  // Sent back and not running: Build waits for you.
  assert.equal(taskSteps(task("changes")).find((step) => step.key === "build").state, "you");
});

test("every step state has its Russian", () => {
  assert.deepEqual(untranslated(Object.values(STEP_STATES)), []);
});
