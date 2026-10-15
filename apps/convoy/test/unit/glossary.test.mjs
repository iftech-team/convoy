import { test } from "node:test";
import assert from "node:assert/strict";
import { GLOSSARY, GLOSSARY_LABELS, GLOSSARY_ORDER } from "../../src/glossary.js";
import { untranslated } from "./helpers.mjs";

test("every term is listed once, with a title and a line", () => {
  assert.deepEqual([...GLOSSARY_ORDER].sort(), Object.keys(GLOSSARY).sort());
  for (const [title, line] of Object.values(GLOSSARY)) {
    assert.ok(title && line);
    assert.ok(line.endsWith("."), line);
  }
});

test("every glossary label has its Russian", () => {
  assert.deepEqual(untranslated(GLOSSARY_LABELS), []);
});
