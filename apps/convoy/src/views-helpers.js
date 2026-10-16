// A manager's helpers: the sessions it started from its own shell with
// `convoy --session spawn`. They nest under it in the sidebar, show as cards
// in its chat, and as a menu in its terminal's header.

import { agentIcon, icons } from "./icons.js";
import { escape, plural } from "./ui.js";
import { t } from "./i18n.js";
import { sessionState, state } from "./state.js";
import { sessionLabel, sessionTone } from "./session-state.js";

/// Every session the window knows, each once: the open project's list is the
/// fresher one.
function known() {
  const seen = new Map();
  for (const item of [...(state.sessions ?? []), ...(state.allSessions ?? [])]) {
    if (!seen.has(item.id)) seen.set(item.id, item);
  }
  return [...seen.values()];
}

/// A manager's helpers that are not archived, in the order they started.
export const helpersOf = (id) => known().filter((item) => item.parent_id === id && !item.archived);

/// Where a helper stands, in the vocabulary every session uses. A helper
/// the manager just started has not run yet: it is starting, not idle.
export function helperState(item) {
  const now = sessionState(item, !!(item.running || state.running?.includes(item.id)));
  return now === "not_started" ? "starting" : now;
}

const label = (key) => (key === "starting" ? "Starting" : sessionLabel(key));
const dot = (key) => (key === "starting" ? "idle" : sessionTone(key));

/// Busy helpers: working, waiting on an approval, or about to start.
export const busyCount = (list) => list.filter((item) => ["working", "needs_you", "starting"].includes(helperState(item))).length;

/// "2 helpers · 1 running", or nothing for a session without helpers.
export function helpersSummary(id) {
  const list = helpersOf(id);
  if (!list.length) return "";
  const busy = busyCount(list);
  return busy
    ? `${plural(list.length, "helper")} · ${t("{count} working", { count: busy })}`
    : plural(list.length, "helper");
}

/// The cards above a manager's composer, one per helper.
export function helperCards(id) {
  const list = helpersOf(id);
  if (!list.length) return "";
  const busy = busyCount(list);
  return `
    <div class="helpers" role="group" aria-label="${escape(t("Helpers"))}">
      <div class="helpers__head">
        ${icons.helpers}
        <span>${escape(t("Helpers"))}</span>
        <span class="helpers__count">${escape(busy ? `${list.length} · ${t("{count} working", { count: busy })}` : String(list.length))}</span>
      </div>
      <div class="helpers__list" data-scroll="helpers-${escape(id)}">
        ${list.map(card).join("")}
      </div>
    </div>`;
}

function card(item) {
  const tone = helperState(item);
  return `
    <div class="helper-card helper-card--${dot(tone)}" data-helper="${escape(item.id)}">
      <span class="dot dot--${dot(tone)}"></span>
      ${agentIcon(item.agent, 12)}
      <span class="helper-card__text">
        <span class="helper-card__title" title="${escape(item.title)}">${escape(item.title)}</span>
        <span class="helper-card__status">${escape(t(label(tone)))}${item.branch ? ` · ${escape(item.branch)}` : ""}</span>
      </span>
      <button class="helper-card__open" data-open="${escape(item.id)}" title="${escape(t("Open {session}", { session: item.title }))}">${escape(t("Open"))}</button>
    </div>`;
}

/// Above a helper's own composer: who started it, one click away.
export function helperOf(id) {
  const item = known().find((entry) => entry.id === id);
  const parent = item?.parent_id ? known().find((entry) => entry.id === item.parent_id) : null;
  if (!parent) return "";
  return `
    <div class="helpers helpers--of">
      <span class="helpers__branch">↳</span>
      <span>${escape(t("Helper of"))}</span>
      <button class="helpers__parent" data-open="${escape(parent.id)}">${agentIcon(parent.agent, 11)}${escape(parent.title)}</button>
    </div>`;
}

/// The header's helpers button, for a manager shown as a terminal.
export function helpersButton(id) {
  const list = helpersOf(id);
  if (!list.length) return "";
  const busy = busyCount(list);
  return `
    <button class="bar-helpers" data-action="helpers-menu" title="${escape(helpersSummary(id))}" aria-haspopup="menu">
      ${icons.helpers}<span>${escape(plural(list.length, "helper"))}</span>${busy ? '<span class="dot dot--running"></span>' : ""}
    </button>`;
}

/// The menu that button opens: each helper, where it stands, and a way in.
export function helpersMenu(id, anchor) {
  const list = helpersOf(id);
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--anchored helpers-menu" role="menu" style="${anchor}">
        <div class="helpers-menu__label">${escape(t("Helpers"))}</div>
        ${list
          .map((item) => {
            const tone = helperState(item);
            return `
              <button class="menu__item helpers-menu__item" role="menuitem" data-open="${escape(item.id)}">
                <span class="dot dot--${dot(tone)}"></span>
                ${agentIcon(item.agent, 12)}
                <span class="helpers-menu__title">${escape(item.title)}</span>
                <span class="menu__hint">${escape(t(label(tone)))}</span>
              </button>`;
          })
          .join("")}
      </div>
    </div>`;
}

/// An activity row about a helper, as its manager records it.
const ACTIVITY = [
  [/^Started helper: (.*)$/s, "started helper"],
  [/^Helper finished: (.*)$/s, "helper finished"],
  [/^Helper failed: (.*)$/s, "helper failed"],
  [/^Helper stopped: (.*)$/s, "helper stopped"],
];

/// `[label, detail]` for a helper's activity row, or null for any other.
export function helperActivity(detail) {
  for (const [pattern, label] of ACTIVITY) {
    const found = pattern.exec(String(detail ?? ""));
    if (found) return [label, found[1]];
  }
  return null;
}
