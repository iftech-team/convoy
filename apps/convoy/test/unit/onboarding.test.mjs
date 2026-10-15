import { test } from "node:test";
import assert from "node:assert/strict";
import { CHECKLIST_LABELS, checklist } from "../../src/onboarding.js";
import { untranslated } from "./helpers.mjs";

const done = (list) => Object.fromEntries(list.items.map((item) => [item.key, item.done]));

test("a first run: nothing read yet, nothing done", () => {
  const list = checklist({});
  assert.deepEqual(done(list), { agent: false, login: false, project: false, task: false });
  assert.equal(list.complete, false);
});

test("an agent found and logged in, a project, a task", () => {
  const list = checklist({
    diagnostics: [{ name: "claude", found: true }, { name: "gh", found: false }],
    setupChecks: [{ id: "login.claude", status: "ok" }],
    projects: [{}],
    tasks: [{}],
  });
  assert.deepEqual(done(list), { agent: true, login: true, project: true, task: true });
  assert.equal(list.complete, true);
  assert.deepEqual(list.items[0].detail, ["Found {agents}.", { agents: "Claude Code" }]);
});

test("found but not logged in, or still checking", () => {
  const diagnostics = [{ name: "codex", found: true }];
  assert.equal(done(checklist({ diagnostics, setupChecks: [{ id: "login.codex", status: "failed" }] })).login, false);
  assert.deepEqual(checklist({ diagnostics }).items[1].detail, ["Checking…", {}]);
  // Gemini has no login check: found is enough.
  assert.equal(done(checklist({ diagnostics: [{ name: "gemini", found: true }], setupChecks: [] })).login, true);
});

test("a found agent that is not signed in is signed in from the window", () => {
  const login = checklist({ diagnostics: [{ name: "claude", found: true }], setupChecks: [{ id: "login.claude", status: "failed" }] }).items[1];
  assert.equal(login.action, "login");
  assert.equal(login.agent, "claude");
  assert.deepEqual(login.button, ["Log in to {agent}", { agent: "Claude Code" }]);
  // Nothing found yet: the step points at setup instead.
  assert.equal(checklist({ diagnostics: [], setupChecks: [] }).items[1].action, "setup");
});

test("every checklist label has its Russian", () => {
  assert.deepEqual(untranslated(CHECKLIST_LABELS), []);
});
