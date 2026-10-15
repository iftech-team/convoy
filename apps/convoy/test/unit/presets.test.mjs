import { test } from "node:test";
import assert from "node:assert/strict";
import { CUSTOM, PRESETS, presetOf, presetPatch } from "../../src/presets.js";
import { untranslated } from "./helpers.mjs";

const bare = { is_repository: true };

test("a new project is Simple", () => {
  assert.equal(presetOf(bare), "simple");
  assert.equal(presetOf({ ...bare, task_mode: "" }), "simple");
  assert.equal(presetOf({}), "simple");
});

test("each preset's change makes the project that preset", () => {
  for (const repository of [true, false]) {
    for (const testing of [true, false]) {
      const project = { is_repository: repository, test_commands: testing ? "npm test" : "" };
      for (const name of ["simple", "reviewed", "autopilot"]) {
        assert.equal(presetOf({ ...project, ...presetPatch(name, project) }), name, `${name}, repository ${repository}, testing ${testing}`);
      }
    }
  }
});

test("what a preset leaves alone it does not compare", () => {
  // No checks to run: Reviewed neither needs nor sets automatic testing.
  assert.equal("qa_auto" in presetPatch("reviewed", bare), false);
  assert.equal(presetOf({ ...bare, ...presetPatch("reviewed", bare), qa_auto: true }), "reviewed");
  // A folder of repositories: no worktrees, no publishing by Convoy.
  const folder = { is_repository: false };
  assert.equal("task_worktrees" in presetPatch("autopilot", folder), false);
  assert.equal(presetPatch("autopilot", folder).max_parallel, 1);
  assert.equal(presetPatch("autopilot", bare).max_parallel, 2);
  // How many at once is set, never compared.
  assert.equal(presetOf({ ...bare, ...presetPatch("autopilot", bare), max_parallel: 4 }), "autopilot");
});

test("anything else is Custom", () => {
  assert.equal(presetOf({ ...bare, qa_auto: true }), "custom");
  assert.equal(presetOf({ ...bare, task_mode: "push" }), "custom");
  assert.equal(presetOf({ ...bare, ...presetPatch("reviewed", bare), plan_first_default: true }), "custom");
  assert.equal(presetPatch("bogus", bare), null);
});

test("every preset has its Russian", () => {
  assert.deepEqual(untranslated([...Object.values(PRESETS).flat(), ...CUSTOM]), []);
});
