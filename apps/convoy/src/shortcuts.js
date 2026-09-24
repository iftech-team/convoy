// Keyboard shortcuts.
//
// The workspace stores them in the form the Electron build writes —
// `mod+alt+n` — and every client reads that same file, so the stored notation
// is the one honoured here rather than a translation of it.

import { state } from "./state.js";

/// The actions, their defaults and what they do. The order is the order the
/// settings dialog lists them in, and it matches `SHORTCUT_ACTIONS` in the
/// core so a file written by either build reads the same.
export const SHORTCUTS = [
  ["palette", "mod+k", "Command palette"],
  ["newSession", "mod+n", "New session"],
  ["files", "mod+b", "Files & Changes"],
  ["next", "mod+alt+n", "Next session"],
  ["previous", "mod+alt+p", "Previous session"],
  ["settings", "mod+,", "Settings"],
  ["search", "mod+f", "Search projects and sessions"],
];

/// What is bound to an action right now: the saved value if there is one and
/// it is well formed, the default otherwise.
export function binding(action) {
  const saved = state.settings.shortcuts?.[action];
  const fallback = SHORTCUTS.find(([name]) => name === action)?.[1] ?? "";
  return saved && valid(saved) ? saved : fallback;
}

/// `/^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$/` — the same shape the core enforces,
/// so nothing this writes can be rejected when it is read back.
export const valid = (value) => /^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$/.test(value);

/// Whether a key press is the one an action is bound to. `mod` is Command on
/// macOS and Control elsewhere, which is what the name is for.
export function matches(event, stored) {
  if (!stored) return false;
  const parts = stored.split("+");
  if (parts.shift() !== "mod") return false;
  const wantsAlt = parts[0] === "alt" && parts.shift();
  const wantsShift = parts[0] === "shift" && parts.shift();
  const key = parts.join("+");
  if (!key) return false;

  const primary = event.metaKey || event.ctrlKey;
  return (
    primary &&
    !!event.altKey === !!wantsAlt &&
    !!event.shiftKey === !!wantsShift &&
    event.key.toLowerCase() === key
  );
}

/// A key press written in the stored form, or nothing when it is not a
/// binding — a bare letter, or a modifier on its own.
export function capture(event) {
  if (!(event.metaKey || event.ctrlKey)) return null;
  const key = event.key.toLowerCase();
  if (["control", "meta", "shift", "alt"].includes(key)) return null;
  if (!/^[a-z0-9,]+$/.test(key)) return null;
  return `mod+${event.altKey ? "alt+" : ""}${event.shiftKey ? "shift+" : ""}${key}`;
}

/// How a binding is shown. The stored form is for the file, not for reading.
export function label(stored) {
  if (!stored) return "unset";
  const mod = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl";
  return stored
    .replace("mod+", `${mod}+`)
    .replace("alt+", "Alt+")
    .replace("shift+", "Shift+")
    .replace(/\+([a-z0-9,])$/, (_, key) => `+${key.toUpperCase()}`);
}
