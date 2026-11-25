// The agents a picker offers: Claude Code and Codex, and every agent Convoy
// reaches through the Agent Client Protocol (Antigravity, opencode — and
// through it DeepSeek and the rest — Gemini CLI, and any the user adds).
// An ACP agent is named `acp:<id>` everywhere: sessions, tasks, settings.

import { agentIcon } from "./icons.js";
import { registerAgentNames } from "./task-defaults.js";
import { call, state } from "./state.js";

export const NATIVE_AGENTS = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
];

export const isAcp = (agent) => typeof agent === "string" && agent.startsWith("acp:");

/// The ACP agents as last read, with whether each is installed.
export const acpAgents = () => state.acpAgents ?? [];

export const acpAgent = (key) => acpAgents().find((agent) => agent.key === key) ?? null;

/// Reads the ACP agents once; `force` reads them again after a change.
let loading = null;
export function loadAcpAgents(force = false) {
  if (state.acpAgents && !force) return Promise.resolve(state.acpAgents);
  loading ??= call("acp_agents_list").then((agents) => {
    loading = null;
    state.acpAgents = agents ?? [];
    registerAgentNames(Object.fromEntries(state.acpAgents.map((agent) => [agent.key, agent.name])));
    return state.acpAgents;
  });
  return loading;
}

/// Whether the agent can open in a terminal: Claude, Codex, and ACP agents
/// with a terminal UI of their own.
export const hasTerminal = (agent) => !isAcp(agent) || !!acpAgent(agent)?.terminal;

/// Picker rows: Claude and Codex (those installed, when known), then the
/// installed ACP agents — and always the one already chosen, so a choice
/// never disappears from under the picker. `acp: false` leaves ACP agents
/// out, for roles only Claude and Codex can fill.
export function agentOptions({ chosen = "", installed = null, acp = true } = {}) {
  const natives = NATIVE_AGENTS.filter(([agent]) => !installed?.length || installed.includes(agent) || agent === chosen);
  const others = acp ? acpAgents().filter((agent) => agent.installed || agent.key === chosen).map((agent) => [agent.key, agent.name]) : [];
  const rows = [...(natives.length ? natives : NATIVE_AGENTS), ...others];
  return rows.map(([value, name]) => [value, name, agentIcon(value, 15)]);
}

/// A command line as typed, split into words: spaces separate, quotes keep
/// a word together. No shell runs it, so nothing else is special.
export function splitCommand(text) {
  const words = [];
  let word = "";
  let quote = "";
  let started = false;
  for (const char of String(text ?? "")) {
    if (quote) {
      if (char === quote) quote = "";
      else word += char;
    } else if (char === '"' || char === "'") {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started || word) words.push(word);
      word = "";
      started = false;
    } else {
      word += char;
    }
  }
  if (started || word) words.push(word);
  return words;
}
