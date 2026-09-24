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

export function terminalFor(id) {
  if (terminals.has(id)) return terminals.get(id);

  const host = document.createElement("div");
  host.className = "terminal__surface";

  const style = getComputedStyle(document.documentElement);
  const terminal = new Terminal({
    fontFamily: style.getPropertyValue("--font-mono").trim(),
    fontSize: state.settings.font_size,
    lineHeight: 1.35,
    cursorBlink: true,
    allowProposedApi: true,
    scrollback: state.settings.scrollback,
    theme: theme(),
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.loadAddon(new WebLinksAddon());

  terminal.onData((data) => call("terminal_write", { id, data }));
  terminal.onResize(({ cols, rows }) => call("terminal_resize", { id, cols, rows }));

  const entry = { terminal, fit, host, opened: false };
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
    entry.terminal.focus();
  });
}

export function refit() {
  for (const entry of terminals.values()) {
    if (entry.host.isConnected) entry.fit.fit();
  }
}

/// Font size, scrollback and colours reach terminals that are already open, so
/// a settings change is visible without restarting a session.
export function applySettings() {
  for (const entry of terminals.values()) {
    entry.terminal.options.fontSize = state.settings.font_size;
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

export async function start(id) {
  const { terminal, fit } = terminalFor(id);
  state.sessionId = id;
  changed();
  fit.fit();
  await done("session_start", { id, cols: terminal.cols, rows: terminal.rows });
  await loadWorkspace();
}

export async function stop(id) {
  await done("session_stop", { id });
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
    terminalFor(payload.id).terminal.write(payload.data);
  });

  await listen("terminal:exit", async ({ payload }) => {
    const entry = terminals.get(payload.id);
    if (entry) {
      const how = payload.stopped
        ? "stopped"
        : `exited with code ${payload.code}`;
      entry.terminal.write(`\r\n\x1b[2m── session ${how} ──\x1b[0m\r\n`);
    }
    state.agentState.delete(payload.id);
    await loadWorkspace();
    exitHandler(payload);
  });
}

/// The visible buffer, for anything that wants to quote the agent.
export function buffer(id) {
  const entry = terminals.get(id);
  if (!entry) return "";
  const lines = [];
  const active = entry.terminal.buffer.active;
  for (let row = 0; row < active.length; row += 1) {
    lines.push(active.getLine(row)?.translateToString(true) ?? "");
  }
  return lines.join("\n").trimEnd();
}

export { toast };
