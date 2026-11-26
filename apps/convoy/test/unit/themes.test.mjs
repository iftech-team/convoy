// Every terminal theme sets all 16 ANSI colours for its own background, the
// default stands in for an unknown choice, and Convoy's own themes keep their
// colours readable.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ANSI_NAMES, DEFAULT_THEME, TERMINAL_THEMES, themeFor, themesOf, uiVariables } from "../../src/themes.js";
import { untranslated } from "./helpers.mjs";

const HEX = /^#[0-9a-f]{6}$/i;

const luminance = (hex) => {
  const [r, g, b] = [1, 3, 5].map((at) => {
    const c = parseInt(hex.slice(at, at + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

test("each theme has a unique id, a kind and a full palette", () => {
  assert.equal(new Set(TERMINAL_THEMES.map((theme) => theme.id)).size, TERMINAL_THEMES.length);
  for (const theme of TERMINAL_THEMES) {
    assert.ok(["light", "dark"].includes(theme.kind), theme.id);
    for (const key of ["background", "foreground", "cursor", "cursorAccent", ...ANSI_NAMES]) {
      assert.match(theme.colors[key], HEX, `${theme.id}.${key}`);
    }
    assert.ok(theme.colors.selectionBackground, theme.id);
  }
});

test("a light theme is light and a dark theme is dark", () => {
  for (const theme of TERMINAL_THEMES) {
    const light = luminance(theme.colors.background) > 0.5;
    assert.equal(light, theme.kind === "light", theme.id);
  }
});

test("several themes of each kind, the default among them", () => {
  for (const kind of ["light", "dark"]) {
    assert.ok(themesOf(kind).length >= 3, kind);
    assert.ok(themesOf(kind).some((theme) => theme.id === DEFAULT_THEME[kind]), kind);
  }
});

test("an unknown or other-kind choice falls back to the default", () => {
  assert.equal(themeFor("light", "nord").id, DEFAULT_THEME.light);
  assert.equal(themeFor("dark", "no-such-theme").id, DEFAULT_THEME.dark);
  assert.equal(themeFor("dark", null).id, DEFAULT_THEME.dark);
  assert.equal(themeFor("dark", "dracula").id, "dracula");
});

test("Convoy's own themes keep text and colours readable on their background", () => {
  for (const theme of TERMINAL_THEMES.filter((each) => each.own)) {
    const { background, foreground } = theme.colors;
    assert.ok(contrast(foreground, background) >= 7, `${theme.id} foreground`);
    // The shade that matches the background's own end is for backgrounds.
    const skip = theme.kind === "light" ? ["white", "brightWhite"] : ["black"];
    for (const name of ANSI_NAMES.filter((each) => !skip.includes(each))) {
      assert.ok(contrast(theme.colors[name], background) >= 3, `${theme.id}.${name}: ${contrast(theme.colors[name], background).toFixed(2)}`);
    }
  }
});

test("the theme settings have their Russian", () => {
  assert.deepEqual(untranslated(["Light theme", "Dark theme", "The window and its terminals when it is light.", "The window and its terminals when it is dark."]), []);
});

test("a theme that colours the window keeps its text and accent readable", () => {
  for (const theme of TERMINAL_THEMES.filter((each) => each.ui)) {
    const { ui } = theme;
    for (const key of ["bg", "subtle", "sunken", "hover", "active", "border", "borderStrong", "text", "muted", "faint", "accent", "accentText"]) {
      assert.match(ui[key], HEX, `${theme.id}.ui.${key}`);
    }
    assert.ok(contrast(ui.text, ui.bg) >= 7, `${theme.id} text: ${contrast(ui.text, ui.bg).toFixed(2)}`);
    assert.ok(contrast(ui.muted, ui.bg) >= 4.5, `${theme.id} muted: ${contrast(ui.muted, ui.bg).toFixed(2)}`);
    assert.ok(contrast(ui.faint, ui.bg) >= 2.5, `${theme.id} faint: ${contrast(ui.faint, ui.bg).toFixed(2)}`);
    assert.ok(contrast(ui.accentText, ui.bg) >= 4.5, `${theme.id} accent text: ${contrast(ui.accentText, ui.bg).toFixed(2)}`);
    // Primary buttons draw white text on the accent.
    assert.ok(contrast("#ffffff", ui.accent) >= 3, `${theme.id} button: ${contrast("#ffffff", ui.accent).toFixed(2)}`);
    // The terminal sits on the window's own background.
    assert.equal(theme.colors.background, ui.bg, theme.id);
  }
});

test("Catppuccin comes in all four flavours, Latte light and the rest dark", () => {
  const flavours = TERMINAL_THEMES.filter((theme) => theme.id.startsWith("catppuccin-"));
  assert.deepEqual(
    flavours.map((theme) => [theme.id, theme.kind]),
    [["catppuccin-latte", "light"], ["catppuccin-frappe", "dark"], ["catppuccin-macchiato", "dark"], ["catppuccin-mocha", "dark"]],
  );
});

test("Convoy's own look stays in tokens.css", () => {
  for (const id of Object.values(DEFAULT_THEME)) {
    assert.deepEqual(uiVariables(themeFor(id.endsWith("light") ? "light" : "dark", id)), {});
  }
});
