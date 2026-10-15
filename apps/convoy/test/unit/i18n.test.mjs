// Every string the window shows has its Russian. Keys written as t("…") are
// found in the source; keys kept in data tables are checked by the tests of
// the modules that hold them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import ru from "../../src/locales/ru.js";

const src = new URL("../../src/", import.meta.url);

test("every t() key is in ru.js", () => {
  const missing = [];
  for (const file of readdirSync(src).filter((name) => name.endsWith(".js") && name !== "i18n.js")) {
    const text = readFileSync(new URL(file, src), "utf8");
    for (const match of text.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)) {
      const key = JSON.parse(`"${match[1]}"`);
      if (!(key in ru.strings)) missing.push(`${file}: ${key}`);
    }
  }
  assert.deepEqual(missing, []);
});

test("every settings section has its Russian", () => {
  const text = readFileSync(new URL("views-settings.js", src), "utf8");
  const block = text.slice(text.indexOf("export const SECTIONS = ["), text.indexOf("];", text.indexOf("export const SECTIONS = [")));
  const names = [...block.matchAll(/^\s+\["([^"]+)", "/gm)].map((match) => match[1]);
  assert.equal(names.length, 10);
  assert.deepEqual(names.filter((name) => !(name in ru.strings)), []);
});
