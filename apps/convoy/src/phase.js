// Where a task stands, in five phases a person can follow: To do, Working,
// Needs you, Ready, Done. The backend keeps its finer statuses (the pipeline
// and the tracker mapping need them); this is how the window shows them.
//
// A status alone does not say it: "review" is both a reviewer at work and a
// task that passed everything and waits for a person. So the phase reads the
// task's runs too — its plan, review, QA, publication and document — and
// whether their sessions still run.
//
// Pure: no imports. Labels are English keys with values; views call t().

/// The board's columns, left to right: [key, label, tone].
export const PHASES = [
  ["todo", "To do", "idle"],
  ["working", "Working", "running"],
  ["needs_you", "Needs you", "review"],
  ["ready", "Ready", "testing"],
  ["done", "Done", "done"],
];

/// The list's groups, what needs a person first.
export const LIST_ORDER = ["needs_you", "working", "ready", "todo", "done"];

/// The steps a task can go through, in order: [key, label].
export const STEPS = [
  ["plan", "Plan"],
  ["build", "Build"],
  ["review", "Review"],
  ["test", "Test"],
  ["publish", "Publish"],
  ["document", "Document"],
];

/// What each step's session is called when it stalls or asks something.
const STALLED = {
  plan: ["Planner stopped · resume it", "Planner is asking you"],
  build: ["Builder stopped · run it again", "Builder is asking you"],
  review: ["Reviewer stopped · resume it", "Reviewer is asking you"],
  test: ["QA agent stopped · resume it", "QA agent is asking you"],
  publish: ["PR author stopped · resume it", "PR author is asking you"],
  document: ["Doc writer stopped · resume it", "Doc writer is asking you"],
};

/// Every label a phase can carry, for the translation check.
export const PHASE_LABELS = [
  ...PHASES.map(([, label]) => label),
  ...STEPS.map(([, label]) => label),
  ...Object.values(STALLED).flat(),
  "Done · PR #{number}",
  "Planner stopped without a plan",
  "Not published",
  "No document written",
  "Reviewer gave no verdict",
  "QA gave no verdict",
  "Out of fix rounds ({round}/{max})",
  "Review failed",
  "Needs a look",
  "Approve the plan",
  "Planning",
  "Waits for {tasks}",
  "Plan approved · waiting to build",
  "Task changed · will plan again",
  "Waiting to start",
  "Building",
  "Fixing",
  "Fixing · round {round}/{max}",
  "Fixes waiting · run to apply",
  "Sent back · run it again",
  "Writing the pull request",
  "Opening the pull request",
  "Writing the document",
  "Running checks",
  "Waiting for the test environment",
  "QA agent testing",
  "Reviewing",
  "Reviewing · fix {round}/{max}",
  "Tests after the review",
  "PR #{number} open",
  "PR open",
  "QA passed",
  "Reviewed",
  "Review skipped",
  "Documented",
  "Built · check it",
  "Unknown status {status}",
];

/// The pull request's number, from a GitHub, GitLab or Gitea link.
export const prNumber = (url) => String(url ?? "").match(/\/(?:pull|pulls|merge_requests|pull-requests)\/(\d+)/)?.[1] ?? null;

const result = (phase, step, label, vars = {}, tone = null, extra = {}) => ({
  phase,
  step,
  label,
  vars,
  tone: tone ?? PHASES.find(([key]) => key === phase)[2],
  session: null,
  action: null,
  ...extra,
});

/// A stage whose session should be at work: stalled when its process ended,
/// asking when its agent waits. Otherwise null, and the stage is working.
function stageTrouble(step, sessionId, live) {
  if (!sessionId) return null;
  const facts = live?.session?.(sessionId);
  if (!facts) return null;
  const [stalled, asking] = STALLED[step];
  if (facts.started && !facts.running) return result("needs_you", step, stalled, {}, "failed", { session: sessionId, action: "open-session" });
  if (facts.running && facts.agent === "waiting") return result("needs_you", step, asking, {}, "review", { session: sessionId, action: "open-session" });
  return null;
}

/// A stage at work, unless its session stalled or asks something.
const working = (step, label, sessionId, live, vars = {}) =>
  stageTrouble(step, sessionId, live) ?? result("working", step, label, vars, null, { session: sessionId ?? null, action: sessionId ? "open-session" : null });

const rounds = (task) => ({ round: task.round ?? 0, max: task.max_rounds ?? 2 });

/// Why a task needs attention: the run that failed last.
function attention(task) {
  if (task.plan?.status === "drafting") return result("needs_you", "plan", "Planner stopped without a plan", {}, "failed", { session: task.plan.session_id ?? null, action: "open-session" });
  const candidates = [];
  if (task.publish_run?.status === "failed") candidates.push([task.publish_run.finished, "publish", "Not published", {}]);
  if (task.docs_run?.status === "failed") candidates.push([task.docs_run.finished, "document", "No document written", {}]);
  if (task.review?.status === "no_verdict") candidates.push([task.review.finished, "review", "Reviewer gave no verdict", {}]);
  if (task.review?.status === "failed") candidates.push([task.review.finished, "review", "Out of fix rounds ({round}/{max})", rounds(task)]);
  if (task.qa?.status === "no_verdict") candidates.push([task.qa.finished, "test", "QA gave no verdict", {}]);
  if (task.qa?.status === "failed") candidates.push([task.qa.finished, "test", "Out of fix rounds ({round}/{max})", rounds(task)]);
  if (!candidates.length) return result("needs_you", "build", "Needs a look", {}, "failed", { action: "open-task" });
  const [newest] = candidates.sort((a, b) => String(b[0] ?? "").localeCompare(String(a[0] ?? "")));
  return result("needs_you", newest[1], newest[2], newest[3], "failed", { action: "open-task" });
}

/// Everything automatic is over and the task waits for a person to check it
/// and mark it done: what it went through, in a few words.
function ready(task) {
  const parts = [];
  const number = prNumber(task.pr_url);
  if (task.pr_url) parts.push(number ? ["PR #{number} open", { number }] : ["PR open", {}]);
  if (task.qa?.status === "passed") parts.push(["QA passed", {}]);
  if (task.review?.status === "passed") parts.push(["Reviewed", {}]);
  else if (task.review?.status === "skipped") parts.push(["Review skipped", {}]);
  if (task.docs?.length) parts.push(["Documented", {}]);
  const step = task.pr_url ? "publish" : task.qa?.status === "passed" ? "test" : task.review ? "review" : "build";
  if (!parts.length) return result("ready", step, "Built · check it", {}, null, { action: "mark-done" });
  return result("ready", step, parts[0][0], parts[0][1], null, { action: "mark-done", parts });
}

/// Where a task stands. `live.session(id)` answers, for a session the window
/// knows, `{ running, started, agent }`; without it nothing is called stalled.
export function taskPhase(task, live = null) {
  switch (task.status) {
    case "done": {
      const number = prNumber(task.pr_url);
      return number ? result("done", null, "Done · PR #{number}", { number }) : result("done", null, "Done");
    }
    case "failed":
      return result("needs_you", "build", "Builder stopped · run it again", {}, "failed", { action: "run", session: task.session_id ?? null });
    case "attention":
      return attention(task);
    case "plan_review":
      return result("needs_you", "plan", "Approve the plan", {}, "review", { action: "approve-plan", session: task.plan?.session_id ?? null });
    case "planning":
      return working("plan", "Planning", task.plan?.session_id, live);
    case "queued": {
      const step = task.plan_first && task.plan?.status !== "approved" ? "plan" : "build";
      if (task.blocked_by?.length) return result("todo", step, "Waits for {tasks}", { tasks: task.blocked_by.join(", ") }, null, { action: "run" });
      if (task.plan?.status === "approved") return result("todo", "build", "Plan approved · waiting to build", {}, null, { action: "run" });
      if (task.plan?.status === "changes_requested") return result("todo", "plan", "Task changed · will plan again", {}, null, { action: "run" });
      return result("todo", step, "Waiting to start", {}, null, { action: "run" });
    }
    case "building":
      return task.round > 0
        ? working("build", "Fixing · round {round}/{max}", task.session_id, live, rounds(task))
        : working("build", "Building", task.session_id, live);
    case "changes": {
      if (live?.session?.(task.session_id)?.running) {
        return task.round > 0 ? working("build", "Fixing · round {round}/{max}", task.session_id, live, rounds(task)) : working("build", "Fixing", task.session_id, live);
      }
      const label = task.findings ? "Fixes waiting · run to apply" : "Sent back · run it again";
      return result("needs_you", "build", label, {}, "open", { action: "run", session: task.session_id ?? null });
    }
    case "review":
    case "pr":
    case "testing": {
      const publish = task.publish_run?.status;
      if (publish === "writing") return working("publish", "Writing the pull request", task.publish_run.session_id, live);
      if (publish === "publishing") return result("working", "publish", "Opening the pull request");
      if (task.docs_run?.status === "writing") return working("document", "Writing the document", task.docs_run.session_id, live);
      const qa = task.qa?.status;
      if (qa === "checking") return result("working", "test", "Running checks");
      if (qa === "queued") return result("working", "test", "Waiting for the test environment");
      if (qa === "testing") return working("test", "QA agent testing", task.qa.session_id, live);
      if (task.review?.status === "reviewing") {
        const label = task.round > 0 ? "Reviewing · fix {round}/{max}" : "Reviewing";
        return working("review", label, task.review.session_id, live, rounds(task));
      }
      if (qa === "waiting") return result("working", "test", "Tests after the review");
      // A review of this round that failed or said nothing, and nothing
      // running after it: the fix round did not start.
      if (task.review && (task.review.round ?? 0) === (task.round ?? 0)) {
        if (task.review.status === "failed") return result("needs_you", "review", "Review failed", {}, "failed", { action: "open-task" });
        if (task.review.status === "no_verdict") return result("needs_you", "review", "Reviewer gave no verdict", {}, "failed", { action: "open-task" });
      }
      return ready(task);
    }
    default:
      return result("needs_you", null, "Unknown status {status}", { status: task.status }, "failed");
  }
}

/// Whether a person may move a task by hand: not while something works on it.
export const movable = (phase) => phase.phase !== "working";

/// The status a drop on a column sets, or null when that column takes no
/// drop. Working runs the task instead; that is the caller's to do.
export function dropStatus(task, target) {
  switch (target) {
    case "todo":
      return "queued";
    case "ready":
      return task.pr_url ? "pr" : "review";
    case "done":
      return "done";
    default:
      return null;
  }
}
