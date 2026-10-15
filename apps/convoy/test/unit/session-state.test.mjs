import { test } from "node:test";
import assert from "node:assert/strict";
import { SESSION_STATES, sessionLabel, sessionStateOf, sleptBy } from "../../src/session-state.js";
import { untranslated } from "./helpers.mjs";

test("a session that is not running", () => {
  assert.equal(sessionStateOf({ started: false }, { running: false }), "not_started");
  assert.equal(sessionStateOf({ started: false }, { running: false, asleep: true }), "not_started");
  assert.equal(sessionStateOf({ started: true }, { running: false }), "stopped");
  assert.equal(sessionStateOf({ started: true }, { running: false, asleep: true }), "asleep");
  // What the agent last said does not outlive its process.
  assert.equal(sessionStateOf({ started: true }, { running: false, agent: "waiting" }), "stopped");
});

test("a running session, by what its agent reported", () => {
  const of = (agent) => sessionStateOf({ started: true }, { running: true, agent });
  assert.equal(of("waiting"), "needs_you");
  assert.equal(of("done"), "finished");
  assert.equal(of("idle"), "idle");
  assert.equal(of("ended"), "idle");
  assert.equal(of("working"), "working");
  // Codex in a terminal reports nothing: it is working.
  assert.equal(of(undefined), "working");
  // Running wins over a stale sleep.
  assert.equal(sessionStateOf({ started: true }, { running: true, asleep: true }), "working");
});

test("raw reports never show as labels", () => {
  for (const raw of ["idle", "working", "waiting", "done", "ended", "bogus"]) {
    const label = sessionLabel(sessionStateOf({ started: true }, { running: true, agent: raw }));
    assert.ok(Object.values(SESSION_STATES).some(([name]) => name === label), label);
  }
});

test("every state has its Russian", () => {
  assert.deepEqual(untranslated(Object.values(SESSION_STATES).map(([label]) => label)), []);
});

test("sleep is read from the activity feed", () => {
  assert.ok(sleptBy({ kind: "hibernated" }));
  // The text session.rs writes when Convoy puts a session to sleep.
  assert.ok(sleptBy({ kind: "exited", detail: "Hibernated after an idle period" }));
  assert.ok(!sleptBy({ kind: "exited", detail: "Exited with code 0" }));
  assert.ok(!sleptBy({ kind: "started" }));
  assert.ok(!sleptBy(undefined));
});
