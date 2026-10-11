// Conversations the agents saved for this project, in its folder and its
// worktrees, that are not sessions yet. They sit under the sessions rather
// than in a tab of their own: once one is resumed it is a session, so two
// lists would only show the same work twice.

import { agentIcon, icons } from "./icons.js";
import { button, escape } from "./ui.js";
import { project, state } from "./state.js";
import { language, t } from "./i18n.js";

const COLLAPSED = "convoy.notInConvoyCollapsed";

export const historyCollapsed = () => {
  try {
    return localStorage.getItem(COLLAPSED) === "1";
  } catch {
    return false;
  }
};

export function toggleHistoryCollapsed() {
  try {
    localStorage.setItem(COLLAPSED, historyCollapsed() ? "0" : "1");
  } catch {
    /* not remembered */
  }
}

export const when = (ms) => {
  const date = new Date(ms);
  const today = new Date().toDateString() === date.toDateString();
  return today
    ? date.toLocaleTimeString(language(), { hour: "numeric", minute: "2-digit" })
    : date.toLocaleDateString(language(), { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
};

/// When a session's conversation was last written to, from the same scan;
/// undefined until it has been read or when the agent saved nothing yet.
export function lastActive(item) {
  const history = state.history;
  if (!history || history.projectId !== state.projectId) return undefined;
  return history.entries.find((entry) => entry.session_id === item.id)?.at;
}

/// The project's saved conversations that no session holds yet.
export function unlinkedConversations() {
  const history = state.history;
  if (!history || history.projectId !== state.projectId) return [];
  return history.entries.filter((entry) => !entry.session_id);
}

/// The saved conversations the list shows: the session filter narrows them,
/// and a folded section shows none.
export function shownConversations() {
  if (historyCollapsed()) return [];
  const needle = state.filter.trim().toLowerCase();
  return unlinkedConversations().filter(
    (entry) => !needle || `${entry.title} ${entry.provider_id}`.toLowerCase().includes(needle),
  );
}

/// How a saved conversation is picked for deleting, beside `s:<id>` for a session.
export const conversationKey = (entry) => `c:${entry.agent}:${entry.provider_id}`;

/// A row's checkbox while picking rows to delete.
export const pickBox = (key, label, disabled = false) =>
  `<input class="row__pick" type="checkbox" data-pick="${escape(key)}" aria-label="${escape(label)}"
          ${state.picked?.has(key) ? " checked" : ""}${disabled ? " disabled" : ""} />`;

/// The "Not in Convoy yet" section under the sessions; empty when there is
/// nothing to resume. The session filter narrows it too.
export function historySection() {
  const entries = unlinkedConversations();
  if (!entries.length) return "";
  const root = project()?.path ?? "";
  const shown = shownConversations();
  const collapsed = historyCollapsed();
  const picking = !!state.picked;
  const rows = shown
    .map(
      (entry) => `
        <div class="row row--saved${picking ? " row--picking" : ""}"${picking ? ` data-pick-row="${escape(conversationKey(entry))}"` : ""}>
          ${picking ? pickBox(conversationKey(entry), entry.title || t("Untitled conversation")) : ""}
          <span class="row__mark">${agentIcon(entry.agent, 16)}</span>
          <div class="row__body">
            <div class="row__title">${escape(entry.title || t("Untitled conversation"))}</div>
            <div class="row__meta">
              ${escape(
                [
                  when(entry.at),
                  entry.profile ? t("account {name}", { name: entry.profile }) : "",
                  entry.directory !== root ? t("worktree {name}", { name: entry.directory.split(/[\\/]/).pop() }) : "",
                ]
                  .filter(Boolean)
                  .join(" · "),
              )}
            </div>
          </div>
          ${
            picking
              ? ""
              : `<div class="row__actions">
            ${button({ icon: "copy", kind: "quiet", title: t("Copy ID"), data: { "history-copy": entry.provider_id } })}
            ${button({ icon: "trash", kind: "quiet", title: t("Delete…"), data: { "history-delete": conversationKey(entry) } })}
            ${button({ label: t("Resume"), icon: "play", kind: "quiet", data: { "history-resume": entry.provider_id } })}
          </div>`
          }
        </div>`,
    )
    .join("");
  return `
    <div class="section section--sub">
      <h2 class="section__title">
        <button class="section__fold" data-action="history-fold" aria-expanded="${!collapsed}">
          ${collapsed ? icons.chevronRight : icons.chevronDown}${escape(t("Not in Convoy yet"))}
        </button>
        <span class="section__count">${entries.length}</span>
      </h2>
    </div>
    ${
      collapsed
        ? ""
        : `<p class="section__hint">${escape(t("Saved by Claude Code or Codex for this folder. Resume one to add it to your sessions."))}</p>
           <div class="list">
             ${rows || `<div class="list__none">${escape(t("Nothing matches “{filter}”.", { filter: state.filter }))}</div>`}
           </div>`
    }`;
}
