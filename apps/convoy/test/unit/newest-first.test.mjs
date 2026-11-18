import { test } from "node:test";
import assert from "node:assert/strict";
import { byNewest } from "../../src/session-state.js";

const ids = (list) => list.map((item) => item.id);

test("sessions list by when they were last touched, newest first", () => {
  const sessions = [
    { id: "old" },
    { id: "gone" },
    { id: "made", created_at: "2026-11-18T10:00:00.000Z" },
    { id: "used" },
    { id: "lost" },
  ];
  const written = { old: Date.parse("2026-10-01"), used: Date.parse("2026-11-18T12:00:00Z") };
  // "gone" and "lost" have no time at all: last, the later added first.
  assert.deepEqual(ids(byNewest(sessions, (item) => written[item.id], sessions)), ["used", "made", "old", "lost", "gone"]);
});

test("a session made long ago but written today counts as today", () => {
  const sessions = [{ id: "a", created_at: "2026-10-01T00:00:00Z" }, { id: "b", created_at: "2026-11-01T00:00:00Z" }];
  assert.deepEqual(ids(byNewest(sessions, (item) => (item.id === "a" ? Date.parse("2026-11-18") : undefined), sessions)), ["a", "b"]);
});
