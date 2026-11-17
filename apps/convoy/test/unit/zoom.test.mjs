// The window's zoom moves through fixed steps, stops at either end, and
// treats anything it does not know as the default size.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ZOOMS, percent, stepFrom } from "../../src/zoom.js";

test("the steps include the default size and run small to large", () => {
  assert.ok(ZOOMS.includes(1));
  assert.deepEqual([...ZOOMS].sort((a, b) => a - b), ZOOMS);
});

test("a step moves to the neighbouring zoom", () => {
  assert.equal(stepFrom(1, 1), 1.1);
  assert.equal(stepFrom(1.1, 1), 1.25);
  assert.equal(stepFrom(1, -1), 0.9);
});

test("the ends hold", () => {
  assert.equal(stepFrom(ZOOMS.at(-1), 1), ZOOMS.at(-1));
  assert.equal(stepFrom(ZOOMS[0], -1), ZOOMS[0]);
});

test("zero, or a zoom that is not a step, starts from the default size", () => {
  assert.equal(stepFrom(1.75, 0), 1);
  assert.equal(stepFrom(1.33, 1), 1.1);
});

test("a zoom reads as a whole percentage", () => {
  assert.equal(percent(1.25), "125%");
  assert.equal(percent(0.9), "90%");
});
