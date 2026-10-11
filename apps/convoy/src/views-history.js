// The project's conversation history: every conversation the agents saved for
// it, in its folder and its worktrees, ready to resume — the macOS app's
// History panel.

import { agentIcon } from "./icons.js";
import { button, empty, escape } from "./ui.js";
import { project, state } from "./state.js";
import { language, t } from "./i18n.js";

const when = (ms) => {
  const date = new Date(ms);
  const today = new Date().toDateString() === date.toDateString();
  return today
    ? date.toLocaleTimeString(language(), { hour: "numeric", minute: "2-digit" })
    : date.toLocaleDateString(language(), { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
};

export function historyView() {
  const history = state.history;
  if (!history || history.projectId !== state.projectId) return `<div class="list__none">${escape(t("Reading…"))}</div>`;
  const needle = (state.historyFilter ?? "").toLowerCase();
  const root = project()?.path ?? "";
  const entries = history.entries.filter(
    (entry) => !needle || `${entry.title} ${entry.provider_id}`.toLowerCase().includes(needle),
  );
  const head = `
    <div class="section">
      <h2 class="section__title">${escape(t("History"))}
        <span class="section__count">${history.entries.length}</span>
      </h2>
      <input id="history-filter" class="section__filter" type="search" placeholder="${escape(t("Filter conversations"))}"
             value="${escape(state.historyFilter ?? "")}" spellcheck="false" />
      <span class="section__spacer"></span>
      ${button({ label: t("Refresh"), icon: "refresh", action: "history-refresh" })}
    </div>`;
  if (!history.entries.length) {
    return `${head}${empty("history", t("No saved conversations"), t("Claude Code and Codex conversations held in this folder or its worktrees show here."))}`;
  }
  return `${head}
    <div class="list">
      ${entries
        .map(
          (entry) => `
        <div class="row">
          <span class="row__mark">${agentIcon(entry.agent, 16)}</span>
          <div class="row__body">
            <div class="row__title">${escape(entry.title || t("Untitled conversation"))}</div>
            <div class="row__meta">
              ${escape(
                [
                  when(entry.at),
                  entry.profile ? t("account {name}", { name: entry.profile }) : "",
                  entry.directory !== root ? t("worktree {name}", { name: entry.directory.split(/[\\/]/).pop() }) : "",
                  entry.session_id ? t("in sidebar") : "",
                ]
                  .filter(Boolean)
                  .join(" · "),
              )}
            </div>
          </div>
          <div class="row__actions">
            ${button({ label: t("Copy ID"), kind: "quiet", data: { "history-copy": entry.provider_id } })}
            ${button({ label: entry.session_id ? t("Open") : t("Resume"), icon: "play", data: { "history-resume": entry.provider_id } })}
          </div>
        </div>`,
        )
        .join("") || `<div class="list__none">${escape(t("Nothing matches."))}</div>`}
    </div>`;
}
