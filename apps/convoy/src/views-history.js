// The project's conversation history: every conversation the agents saved for
// it, in its folder and its worktrees, ready to resume — the macOS app's
// History panel.

import { agentIcon } from "./icons.js";
import { button, empty, escape } from "./ui.js";
import { project, state } from "./state.js";

const when = (ms) => {
  const date = new Date(ms);
  const today = new Date().toDateString() === date.toDateString();
  return today
    ? date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : date.toLocaleDateString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
};

export function historyView() {
  const history = state.history;
  if (!history || history.projectId !== state.projectId) return '<div class="list__none">Reading…</div>';
  const needle = (state.historyFilter ?? "").toLowerCase();
  const root = project()?.path ?? "";
  const entries = history.entries.filter(
    (entry) => !needle || `${entry.title} ${entry.provider_id}`.toLowerCase().includes(needle),
  );
  const head = `
    <div class="section">
      <h2 class="section__title">History
        <span class="section__count">${history.entries.length}</span>
      </h2>
      <input id="history-filter" class="section__filter" type="search" placeholder="Filter conversations"
             value="${escape(state.historyFilter ?? "")}" spellcheck="false" />
      <span class="section__spacer"></span>
      ${button({ label: "Refresh", icon: "refresh", action: "history-refresh" })}
    </div>`;
  if (!history.entries.length) {
    return `${head}${empty("history", "No saved conversations", "Claude Code and Codex conversations held in this folder or its worktrees show here.")}`;
  }
  return `${head}
    <div class="list">
      ${entries
        .map(
          (entry) => `
        <div class="row">
          <span class="row__mark">${agentIcon(entry.agent, 16)}</span>
          <div class="row__body">
            <div class="row__title">${escape(entry.title || "Untitled conversation")}</div>
            <div class="row__meta">
              ${escape(
                [
                  when(entry.at),
                  entry.profile ? `account ${entry.profile}` : "",
                  entry.directory !== root ? `worktree ${entry.directory.split(/[\\/]/).pop()}` : "",
                  entry.session_id ? "in sidebar" : "",
                ]
                  .filter(Boolean)
                  .join(" · "),
              )}
            </div>
          </div>
          <div class="row__actions">
            ${button({ label: "Copy ID", kind: "quiet", data: { "history-copy": entry.provider_id } })}
            ${button({ label: entry.session_id ? "Open" : "Resume", icon: "play", data: { "history-resume": entry.provider_id } })}
          </div>
        </div>`,
        )
        .join("") || '<div class="list__none">Nothing matches.</div>'}
    </div>`;
}
