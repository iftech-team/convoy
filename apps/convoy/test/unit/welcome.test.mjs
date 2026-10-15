import { test } from "node:test";
import assert from "node:assert/strict";
import { FLOW, WELCOME_LABELS } from "../../src/onboarding.js";
import { untranslated } from "./helpers.mjs";

test("a task moves through three steps", () => {
  assert.equal(FLOW.length, 3);
});

test("every welcome label has its Russian", () => {
  assert.deepEqual(untranslated(WELCOME_LABELS), []);
});
