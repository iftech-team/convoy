import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_TASK_MODE, TASK_MODES, agentName, newTaskDefaults, taskModeLabel, taskModeOf } from "../../src/task-defaults.js";
import { untranslated } from "./helpers.mjs";

test("a project that never chose does not publish", () => {
  assert.equal(DEFAULT_TASK_MODE, "none");
  assert.equal(taskModeOf({}), "none");
  assert.equal(taskModeOf({ task_mode: "" }), "none");
  assert.equal(taskModeOf(null), "none");
  assert.equal(taskModeOf({ task_mode: "bogus" }), "none");
  assert.equal(taskModeOf({ task_mode: "pr" }), "pr");
  assert.equal(taskModeOf({ task_mode: "push" }), "push");
});

test("new tasks take the project's defaults, then the app's agent", () => {
  assert.deepEqual(newTaskDefaults({ default_agent: "codex", task_mode: "pr", review_auto: true, plan_first_default: true }, { default_agent: "claude" }), {
    agent: "codex",
    mode: "pr",
    auto_review: true,
    plan_first: true,
  });
  assert.deepEqual(newTaskDefaults({ default_agent: "" }, { default_agent: "gemini" }), { agent: "gemini", mode: "none", auto_review: false, plan_first: false });
  assert.equal(newTaskDefaults(undefined, undefined).agent, "claude");
});

test("each mode has one label, translated", () => {
  assert.equal(taskModeLabel("none"), "Don't publish");
  assert.equal(taskModeLabel("pr"), "Pull request");
  assert.equal(taskModeLabel(undefined), "Don't publish");
  assert.deepEqual(untranslated(TASK_MODES.map(([, name]) => name)), []);
});

test("agents are named, Gemini included", () => {
  assert.equal(agentName("claude"), "Claude Code");
  assert.equal(agentName("codex"), "Codex");
  assert.equal(agentName("gemini"), "Gemini CLI");
});
