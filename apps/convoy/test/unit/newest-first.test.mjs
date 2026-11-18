import { test } from "node:test";
import assert from "node:assert/strict";
import { byNewest } from "../../src/session-state.js";

test("sessions list newest first, a just-made one on top", () => {
  const sessions = [{ id: "old" }, { id: "mid" }, { id: "fresh" }, { id: "new" }];
  const written = { old: 100, mid: 200, new: 300 };
  const ids = (list) => list.map((item) => item.id);
  assert.deepEqual(ids(byNewest(sessions, (item) => written[item.id], sessions)), ["fresh", "new", "mid", "old"]);
  // Before the conversations are read, the later added comes first.
  assert.deepEqual(ids(byNewest(sessions, () => undefined, sessions)), ["new", "fresh", "mid", "old"]);
});
