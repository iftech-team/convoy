// The terminals.
//
// One xterm per session, created once and kept. The element lives outside the
// rendered tree and is moved into place: rebuilding it would detach xterm from
// its canvas and lose the scrollback, which in a session that has been running
// for an hour is the whole history.

import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { SearchAddon } from "@xterm/addon-search";
import { call, done, state, changed, loadWorkspace, toast } from "./state.js";

const terminals = new Map();

function theme() {
  const style = getComputedStyle(document.documentElement);
  const read = (name) => style.getPropertyValue(name).trim();
  return {
    background: read("--term-bg"),
    foreground: read("--term-fg"),
    cursor: read("--term-cursor"),
    selectionBackground: read("--term-selection"),
  };
}

// ------------------------------------------------------------------- zoom --

// Each terminal's zoom, as an offset from the Settings size so a change there
// still moves every terminal. Kept per session across launches.
const ZOOM_KEY = "convoy.zoom";
let zooms = {};
try {
  zooms = JSON.parse(localStorage.getItem(ZOOM_KEY) ?? "{}") ?? {};
} catch {
  zooms = {};
}

const sizeFor = (id) => Math.max(8, Math.min(32, state.settings.font_size + (zooms[id] ?? 0)));

/// ⌘+ / ⌘− / ⌘0: one terminal larger, smaller, or back to the Settings size.
export function zoom(id, step) {
  const entry = terminals.get(id);
  if (!entry) return;
  zooms[id] = step === 0 ? 0 : (zooms[id] ?? 0) + step;
  if (!zooms[id]) delete zooms[id];
  try {
    localStorage.setItem(ZOOM_KEY, JSON.stringify(zooms));
  } catch {
    /* not remembered */
  }
  entry.terminal.options.fontSize = sizeFor(id);
  if (entry.host.isConnected) entry.fit.fit();
  return entry.terminal.options.fontSize;
}

/// Claude's words when a session's conversation is gone.
const MISSING = /no conversation found with session id/i;

export function terminalFor(id) {
  if (terminals.has(id)) return terminals.get(id);

  const host = document.createElement("div");
  host.className = "terminal__surface";

  const style = getComputedStyle(document.documentElement);
  const terminal = new Terminal({
    fontFamily: style.getPropertyValue("--font-mono").trim(),
    fontSize: sizeFor(id),
    // The font's own line height, as SwiftTerm draws it in the macOS app.
    lineHeight: 1,
    cursorBlink: true,
    allowProposedApi: true,
    scrollback: state.settings.scrollback,
    theme: theme(),
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.loadAddon(new WebLinksAddon());
  const search = new SearchAddon();
  terminal.loadAddon(search);
  search.onDidChangeResults(({ resultIndex, resultCount }) => findListener(id, resultIndex, resultCount));

  terminal.onData((data) => call("terminal_write", { id, data }));
  terminal.onResize(({ cols, rows }) => call("terminal_resize", { id, cols, rows }));

  // `restored`: saved output is already shown. `saved`: the text last kept.
  // `missing`: the agent said the conversation it was asked to resume is gone.
  const entry = { terminal, fit, host, search, opened: false, restored: false, saved: "", missing: false };
  terminals.set(id, entry);
  return entry;
}

export function mount(id, slotSelector = "#terminal-host") {
  const slot = document.querySelector(slotSelector);
  if (!slot) return;
  const entry = terminalFor(id);
  slot.append(entry.host);
  if (!entry.opened) {
    entry.terminal.open(entry.host);
    entry.opened = true;
  }
  requestAnimationFrame(() => {
    entry.fit.fit();
    // A field outside the terminal keeps its caret — the find bar above all.
    const active = document.activeElement;
    const typing = active && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName) && !active.closest(".xterm");
    if (!state.dialog && !state.menu && state.sessionId === id && !typing) entry.terminal.focus();
  });
}

// ------------------------------------------------------------------- find --

let findListener = () => {};
/// Told "match n of m" whenever a search's results change.
export const onFindResults = (handler) => {
  findListener = handler;
};

/// Finds `term` in one terminal's scrollback, highlighting every match.
/// `incremental` keeps the current match while the term is still being typed.
export function find(id, term, { previous = false, caseSensitive = false, incremental = false } = {}) {
  const entry = terminals.get(id);
  if (!entry) return false;
  if (!term) {
    entry.search.clearDecorations();
    findListener(id, -1, 0);
    return false;
  }
  const style = getComputedStyle(document.documentElement);
  const read = (name) => style.getPropertyValue(name).trim();
  const options = {
    caseSensitive,
    incremental,
    decorations: {
      matchBackground: read("--term-find"),
      matchOverviewRuler: read("--term-find"),
      activeMatchBackground: read("--term-find-active"),
      activeMatchColorOverviewRuler: read("--term-find-active"),
    },
  };
  return previous ? entry.search.findPrevious(term, options) : entry.search.findNext(term, options);
}

export function endFind(id) {
  const entry = terminals.get(id);
  if (!entry) return;
  entry.search.clearDecorations();
  entry.terminal.clearSelection();
  entry.terminal.focus();
}

/// Types text into the terminal the way a paste does: to the agent, not run.
export function insert(id, text) {
  const entry = terminals.get(id);
  if (!entry) return;
  entry.terminal.paste(text);
  entry.terminal.focus();
}

export function refit() {
  for (const entry of terminals.values()) {
    if (entry.host.isConnected) entry.fit.fit();
  }
}

/// Font size, scrollback and colours reach terminals that are already open, so
/// a settings change is visible without restarting a session.
export function applySettings() {
  for (const [id, entry] of terminals) {
    entry.terminal.options.fontSize = sizeFor(id);
    entry.terminal.options.scrollback = state.settings.scrollback;
    entry.terminal.options.theme = theme();
    if (entry.host.isConnected) entry.fit.fit();
  }
}

export function forget(id) {
  const entry = terminals.get(id);
  if (!entry) return;
  entry.terminal.dispose();
  entry.host.remove();
  terminals.delete(id);
}

export async function start(id, { background = false } = {}) {
  const { terminal, fit } = terminalFor(id);
  if (!background) { state.sessionId = id; changed(); fit.fit(); }
  const started = await done("session_start", { id, cols: terminal.cols, rows: terminal.rows });
  await loadWorkspace();
  return started;
}

export async function stop(id) {
  await done("session_stop", { id });
}

/// Stopping an agent that has gone idle since it reported a finished turn.
/// The process ends the same way, but the task it belongs to goes to review
/// rather than being recorded as a failure.
export async function hibernate(id) {
  await done("session_hibernate", { id });
}

/// Text typed into a running agent, bracketed and without an Enter unless
/// asked for.
export async function paste(id, text, submit = false) {
  return done("terminal_paste", { id, text, submit });
}

let exitHandler = () => {};
export const onExit = (handler) => {
  exitHandler = handler;
};

export async function wire() {
  await listen("terminal:data", ({ payload }) => {
    const entry = terminalFor(payload.id);
    entry.restored = true;
    entry.terminal.write(payload.data);
    if (!entry.missing && MISSING.test(payload.data)) {
      entry.missing = true;
      changed();
    }
  });

  // The exit rules are applied in the backend, where they do not depend on a
  // window being there to answer. If one of them fails, say so.
  await listen("terminal:trouble", ({ payload }) => toast(String(payload), "bad"));

  await listen("terminal:exit", async ({ payload }) => {
    const entry = terminals.get(payload.id);
    if (entry) {
      const how =
        payload.cause === "hibernated"
          ? "hibernated"
          : payload.cause === "stopped"
            ? "stopped"
            : `exited with code ${payload.code}`;
      entry.terminal.write(`\r\n\x1b[2m── session ${how} ──\x1b[0m\r\n`);
    }
    state.agentState.delete(payload.id);
    await snapshot(payload.id);
    await loadWorkspace();
    exitHandler(payload);
  });
}

/// A stopped session's saved output, shown where its terminal would be — as
/// the macOS app does — until the agent is resumed.
export function restore(id, text) {
  const entry = terminalFor(id);
  if (entry.restored) return;
  entry.restored = true;
  entry.saved = text;
  if (!text) return;
  if (MISSING.test(text)) entry.missing = true;
  entry.terminal.write(`\x1b[2m── saved output · resume to reconnect ──\x1b[0m\r\n${text.replace(/\r?\n/g, "\r\n")}\r\n`);
}

export const missingConversation = (id) => !!terminals.get(id)?.missing;

/// Keeps what a terminal shows, as rendered, so reviews, the saved output and
/// the next launch have it. Only when it changed.
export async function snapshot(id) {
  const entry = terminals.get(id);
  if (!entry || !entry.opened) return;
  const text = buffer(id, 1500);
  if (!text || text === entry.saved) return;
  entry.saved = text;
  await call("session_snapshot", { id, text });
}

export async function snapshotRunning() {
  for (const id of state.running) await snapshot(id);
}

/// The buffer, or its last `limit` lines, for anything that quotes the agent.
export function buffer(id, limit = Infinity) {
  const entry = terminals.get(id);
  if (!entry) return "";
  const lines = [];
  const active = entry.terminal.buffer.active;
  for (let row = Math.max(0, active.length - limit); row < active.length; row += 1) {
    lines.push(active.getLine(row)?.translateToString(true) ?? "");
  }
  return lines.join("\n").trimEnd();
}

export { toast };
