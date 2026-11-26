// The terminal's font: each choice is its family first and the system's
// monospace fonts after, and a typed name cannot break out of the CSS.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { MONO_FALLBACK, MONO_FONTS, monoStack } from "../../src/fonts.js";
import { untranslated } from "./helpers.mjs";

test("a font leads its fallbacks; the system choice is the fallbacks alone", () => {
  assert.equal(monoStack("system"), MONO_FALLBACK);
  assert.equal(monoStack("jetbrains-mono"), `"JetBrains Mono", ${MONO_FALLBACK}`);
  assert.equal(monoStack("no-such-font"), MONO_FALLBACK);
});

test("an installed font is named, and an empty name is the system's", () => {
  assert.equal(monoStack("custom", " Iosevka "), `"Iosevka", ${MONO_FALLBACK}`);
  assert.equal(monoStack("custom", ""), MONO_FALLBACK);
  assert.equal(monoStack("custom", 'x"; } body { color: red'), `"x  body  color: red", ${MONO_FALLBACK}`);
});

test("every shipped font is imported by fonts.css", () => {
  const css = readFileSync(new URL("../../src/styles/fonts.css", import.meta.url), "utf8");
  for (const id of ["jetbrains-mono", "fira-code", "cascadia-code", "ibm-plex-mono"]) {
    assert.ok(MONO_FONTS.some((font) => font.id === id), id);
    for (const subset of ["latin", "cyrillic"]) assert.ok(css.includes(`@fontsource/${id}/${subset}-400.css`), `${id} ${subset}`);
  }
});

test("the font settings have their Russian", () => {
  assert.deepEqual(
    untranslated(["Font", "Other installed font", "Font name, e.g. Iosevka", "Terminals, diffs and code. JetBrains Mono, Fira Code, Cascadia Code and IBM Plex Mono ship with Convoy."]),
    [],
  );
});
