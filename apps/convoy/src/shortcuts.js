// Keyboard shortcuts.
//
// The workspace stores them in the form the Electron build writes —
// `mod+alt+n` — and every client reads that same file, so the stored notation
// is the one honoured here rather than a translation of it.

import { state } from "./state.js";

/// The actions, their defaults, what they do and where the settings page
/// lists them. Mirrors `SHORTCUT_ACTIONS`, `DEFAULTS` and `DESCRIPTIONS` in
/// convoy-core, and follows the macOS app's bindings where they do not
/// collide with the seven originals.
export const SHORTCUTS = [
  ["palette", "mod+k", "Command palette", "General"],
  ["search", "mod+f", "Find in terminal, or find a project", "General"],
  ["settings", "mod+,", "Settings", "General"],
  ["sidebar", "mod+b", "Toggle sidebar", "General"],
  ["home", "mod+shift+h", "Home", "General"],
  ["dashboard", "mod+alt+d", "Agent Dashboard", "General"],
  ["files", "mod+shift+g", "Files & Changes", "General"],
  ["theme", "mod+alt+t", "Cycle theme", "General"],
  ["wake", "mod+alt+k", "Toggle keep awake", "General"],
  ["newSession", "mod+n", "New session", "Sessions"],
  ["next", "ctrl+tab", "Next tab", "Sessions"],
  ["previous", "ctrl+shift+tab", "Previous tab", "Sessions"],
  ["closeTab", "mod+w", "Close tab", "Sessions"],
  ["switcher", "mod+e", "Switch terminal, recent first", "Sessions"],
  ["reopenTab", "mod+shift+t", "Reopen closed tab", "Sessions"],
  ["resume", "mod+shift+r", "Resume session", "Sessions"],
  ["stop", "mod+.", "Stop session", "Sessions"],
  ["sleep", "mod+alt+z", "Sleep session", "Sessions"],
  ["pin", "mod+alt+p", "Pin or unpin session", "Sessions"],
  ["edit", "mod+i", "Edit name and notes", "Sessions"],
  ["review", "mod+alt+r", "Start review", "Sessions"],
  ["feedback", "mod+shift+b", "Send feedback to builder", "Sessions"],
  ["quick", "mod+/", "Quick commands", "Sessions"],
  ["split", "mod+\\", "Toggle two panes", "Panes"],
  ["layout1", "ctrl+shift+1", "One pane", "Panes"],
  ["layout2", "ctrl+shift+2", "Two panes", "Panes"],
  ["layout4", "ctrl+shift+4", "Four panes", "Panes"],
  ["paneNext", "mod+alt+right", "Focus next pane", "Panes"],
  ["panePrevious", "mod+alt+left", "Focus previous pane", "Panes"],
  ["paneClose", "mod+shift+w", "Close focused pane", "Panes"],
  ["openFolder", "mod+o", "Open folder", "Project"],
  ["newTask", "mod+shift+n", "New task", "Project"],
  ["newSpec", "mod+alt+n", "New specification", "Project"],
  ["importIssues", "mod+shift+i", "Import Linear or Jira issues", "Project"],
  ["sessionsTab", "mod+alt+1", "Sessions", "Project"],
  ["reviewsTab", "mod+alt+2", "Reviews", "Project"],
  ["specsTab", "mod+alt+3", "Specs", "Project"],
  ["tasksTab", "mod+alt+4", "Tasks", "Project"],
  ["docsTab", "mod+alt+5", "Docs & specs", "Project"],
  ["activity", "mod+shift+a", "Activity feed", "General"],
  ["nextTab", "mod+shift+]", "Next section", "Project"],
  ["previousTab", "mod+shift+[", "Previous section", "Project"],
  ["reveal", "mod+alt+f", "Show project folder", "Project"],
  ["projectRefresh", "mod+alt+shift+r", "Refresh projects in this folder", "Project"],
  ["copyPath", "mod+alt+c", "Copy folder path", "Project"],
  ["gitRefresh", "mod+alt+g", "Refresh Git status", "Project"],
  ["limits", "mod+shift+l", "AI Limits", "Limits"],
  ["limitsRefresh", "mod+alt+l", "Refresh AI limits", "Limits"],
];

export const SHORTCUT_GROUPS = ["General", "Sessions", "Panes", "Project", "Limits"];

/// What is bound to an action right now: the saved value if there is one and
/// it is well formed, the default otherwise.
export function binding(action) {
  const saved = state.settings.shortcuts?.[action];
  const fallback = SHORTCUTS.find(([name]) => name === action)?.[1] ?? "";
  return saved && valid(saved) ? saved : fallback;
}

/// The shape the core enforces, so nothing this writes is rejected on read.
/// `mod` (⌘ on macOS, Ctrl elsewhere) with optional ctrl/alt/shift, or `ctrl`
/// alone — but never Ctrl with a letter, which belongs to the terminal.
export const valid = (value) =>
  /^(mod\+(ctrl\+)?(alt\+)?(shift\+)?([a-z0-9,./;\[\]\\]|left|right|up|down|tab)|ctrl\+(alt\+)?(shift\+)?([0-9,./;\[\]\\]|left|right|up|down|tab))$/.test(value);

const isMac = () => /mac/i.test(navigator.platform);

/// The key a press names, independent of what the modifiers type: on a Mac,
/// Option turns N into "˜" and Shift turns ] into "}", so `event.key` alone
/// never matched an Alt or Shift binding.
const CODES = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  Tab: "tab",
};

export function keyOf(event) {
  const code = event.code ?? "";
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return code.slice(6);
  if (CODES[code]) return CODES[code];
  return (event.key ?? "").toLowerCase();
}

/// Whether a key press is the one an action is bound to. `mod` is Command
/// on macOS and Control elsewhere; `ctrl` is Control everywhere. Every
/// modifier must match exactly, so ⌘⇧N never fires ⌘N.
export function matches(event, stored) {
  if (!stored) return false;
  const parts = stored.split("+");
  const key = parts.pop();
  const wants = new Set(parts);
  if (!wants.has("mod") && !wants.has("ctrl")) return false;
  const command = isMac() ? event.metaKey : false;
  const control = event.ctrlKey;
  const wantsCommand = isMac() && wants.has("mod");
  const wantsControl = wants.has("ctrl") || (!isMac() && wants.has("mod"));
  return (
    command === wantsCommand &&
    control === wantsControl &&
    !!event.altKey === wants.has("alt") &&
    !!event.shiftKey === wants.has("shift") &&
    keyOf(event) === key
  );
}

/// A key press written in the stored form, or nothing when it is not a
/// binding — a bare key, a modifier on its own, or Ctrl with a letter.
export function capture(event) {
  if (["Control", "Meta", "Shift", "Alt"].includes(event.key)) return null;
  const mac = isMac();
  const mod = mac ? event.metaKey : event.ctrlKey;
  const ctrl = mac && event.ctrlKey;
  if (!mod && !event.ctrlKey) return null;
  const prefix = mod ? `mod+${ctrl ? "ctrl+" : ""}` : "ctrl+";
  const stored = `${prefix}${event.altKey ? "alt+" : ""}${event.shiftKey ? "shift+" : ""}${keyOf(event)}`;
  return valid(stored) ? stored : null;
}

/// How a binding is shown. The stored form is for the file, not for reading.
export function label(stored) {
  if (!stored) return "unset";
  const mac = isMac();
  const names = mac
    ? { mod: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" }
    : { mod: "Ctrl", ctrl: "Ctrl", alt: "Alt", shift: "Shift" };
  const keys = { left: "←", right: "→", up: "↑", down: "↓", tab: "Tab" };
  const parts = stored.split("+");
  const key = parts.pop();
  // macOS lists modifiers ⌃⌥⇧⌘; elsewhere they read left to right with a +.
  const order = mac ? ["ctrl", "alt", "shift", "mod"] : ["mod", "ctrl", "alt", "shift"];
  const modifiers = [...new Set(order.filter((name) => parts.includes(name)).map((name) => names[name]))];
  const shown = keys[key] ?? key.toUpperCase();
  return mac ? `${modifiers.join("")}${shown}` : [...modifiers, shown].join("+");
}
