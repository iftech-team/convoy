// Tabs group by project in the order projects first appear, a folded group
// hides its tabs but never the one being looked at, and a drag stays inside
// its own group.
import { test } from "node:test";
import assert from "node:assert/strict";
import { groupBounds, groupTabs, normalizeOrder, visibleTabs } from "../../src/tab-groups.js";

const owner = { a1: "A", a2: "A", b1: "B", b2: "B", c1: "C", loose: null };
const projectOf = (id) => owner[id];

test("groups follow the order projects first appear in", () => {
  const groups = groupTabs(["b1", "a1", "b2", "a2"], projectOf);
  assert.deepEqual(groups, [
    { projectId: "B", tabs: ["b1", "b2"] },
    { projectId: "A", tabs: ["a1", "a2"] },
  ]);
});

test("tabs without a project share one group", () => {
  const groups = groupTabs(["loose", "a1", "missing"], projectOf);
  assert.deepEqual(groups[0], { projectId: null, tabs: ["loose", "missing"] });
});

test("normalizing puts each project's tabs together", () => {
  assert.deepEqual(normalizeOrder(["b1", "a1", "b2", "c1", "a2"], projectOf), ["b1", "b2", "a1", "a2", "c1"]);
});

test("normalizing an order that is already grouped returns it unchanged", () => {
  const tabs = ["a1", "a2", "b1"];
  assert.equal(normalizeOrder(tabs, projectOf), tabs);
});

test("a folded group's tabs are hidden", () => {
  const groups = groupTabs(["a1", "a2", "b1", "b2"], projectOf);
  assert.deepEqual(visibleTabs(groups, new Set(["A"]), "b1"), ["b1", "b2"]);
});

test("the tab being looked at stays visible in a folded group", () => {
  const groups = groupTabs(["a1", "a2", "b1"], projectOf);
  assert.deepEqual(visibleTabs(groups, new Set(["A"]), "a2"), ["a2", "b1"]);
});

test("the group without a project never folds", () => {
  const groups = groupTabs(["loose", "a1"], projectOf);
  assert.deepEqual(visibleTabs(groups, new Set([null, "A"]), null), ["loose"]);
});

test("a drag is bounded by its own group", () => {
  const tabs = ["a1", "a2", "b1", "b2", "c1"];
  assert.deepEqual(groupBounds(tabs, projectOf, "b2"), [2, 3]);
  assert.deepEqual(groupBounds(tabs, projectOf, "c1"), [4, 4]);
});
