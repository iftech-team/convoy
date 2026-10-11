// The approvals inbox: everything an agent is waiting on you for, in every
// project, in one place.
//
// Chat sessions ask through events, and their approvals are the same items
// the chat's cards draw (`state.chats`), so answering here or there is one
// answer. Terminal sessions only say that they wait, through their hooks;
// they are listed too, with nothing to do but open them.

import { anySession, state } from "./state.js";
import { escape } from "./ui.js";
import { t } from "./i18n.js";
import { icons } from "./icons.js";
import { binding, label } from "./shortcuts.js";
import { viewOf, waitingApprovals } from "./views-chat.js";

const ASKS = {
  command: "Run this command?",
  edit: "Make this change?",
  tool: "Use this tool?",
};

const KIND_ICONS = { command: "terminal", edit: "edit", tool: "bolt" };

/// What waits for you, newest first: chat approvals with their answers, then
/// terminal sessions in the waiting state, in one list by time.
export function inboxItems() {
  const rows = waitingApprovals().map(({ id, item }) => ({
    type: "chat",
    key: `${id} ${item.request}`,
    id,
    item,
    at: item.at ?? Date.now(),
  }));
  for (const id of state.running) {
    if (state.agentState.get(id) !== "waiting" || viewOf(anySession(id)) === "chat") continue;
    rows.push({ type: "terminal", key: `${id} terminal`, id, at: state.waitingSince.get(id) ?? Date.now() });
  }
  return rows.sort((a, b) => b.at - a.at);
}

/// "just now", "4 min", "2 h", "3 d".
export function waited(at, now = Date.now()) {
  const minutes = Math.floor(Math.max(0, now - at) / 60000);
  if (minutes < 1) return t("just now");
  if (minutes < 60) return t("{count} min", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("{count} h", { count: hours });
  return t("{count} d", { count: Math.floor(hours / 24) });
}

/// A key as an element id: what is left after anything an id selector
/// would need escaped.
const domId = (key, part) => `inbox-${key.replace(/[^A-Za-z0-9_-]/g, "_")}-${part}`;

const inline = (text) => escape(text).replace(/`([^`]+)`/g, "<code>$1</code>");

/// The tab bar's button: muted with nothing waiting, a count otherwise.
export function approvalsButton() {
  const count = inboxItems().length;
  const shortcut = binding("approvals");
  const name = t("Approvals");
  return `
    <button class="button button--icon counted inbox-button${count ? " inbox-button--waiting" : ""}" data-action="approvals"
            aria-pressed="${state.menu?.kind === "approvals"}" aria-haspopup="dialog"
            aria-label="${escape(count ? t("Approvals: {count} waiting", { count }) : name)}"
            title="${escape(shortcut ? `${name} (${label(shortcut)})` : name)}">
      ${icons.approvals}
      ${count ? `<span class="counted__badge inbox-button__badge">${Math.min(99, count)}</span>` : ""}
    </button>`;
}

function where(id) {
  const item = anySession(id);
  const owner = state.projects.find((entry) => entry.id === item?.project_id);
  return {
    project: owner?.title ?? "",
    title: item?.title || t("Untitled session"),
  };
}

function chatRow(row) {
  const { id, item, key } = row;
  const busy = !!item.busy;
  const detail = String(item.detail ?? "").trim();
  const shown = detail && !String(item.title ?? "").includes(detail) ? detail : "";
  const firstLine = shown.split("\n")[0];
  const short = firstLine.length > 96 ? `${firstLine.slice(0, 95)}…` : firstLine;
  const more = shown && (shown.includes("\n") || firstLine.length > 96);
  const open = more && state.inboxExpanded.has(key);
  const attrs = (decision) =>
    `id="${domId(key, decision)}" data-inbox-answer="${decision}" data-inbox-session="${escape(id)}" data-inbox-request="${escape(item.request)}" ${busy ? "disabled" : ""}`;
  return `
    <li class="inbox-row${busy ? " inbox-row--busy" : ""}" data-inbox-row="${escape(key)}" aria-busy="${busy}">
      ${rowHead(row, KIND_ICONS[item.approval] ?? "bolt", true)}
      <div class="inbox-row__ask">${escape(t(ASKS[item.approval] ?? ASKS.tool))}</div>
      ${item.title ? `<div class="inbox-row__title">${inline(item.title)}</div>` : ""}
      ${
        shown
          ? `${open ? `<pre class="inbox-row__full" data-scroll="inbox-${escape(key)}">${escape(shown)}</pre>` : ""}
             <div class="inbox-row__detail">
               ${open ? '<span class="inbox-row__spacer"></span>' : `<code class="inbox-row__short">${escape(short)}</code>`}
               ${
                 more
                   ? `<button class="inbox-row__more" id="${domId(key, "more")}" data-inbox-expand="${escape(key)}" aria-expanded="${open}">${escape(open ? t("Less") : t("More"))}</button>`
                   : ""
               }
             </div>`
          : ""
      }
      <div class="inbox-row__actions">
        <button class="button button--primary" ${attrs("allow")}>${escape(t("Allow"))}</button>
        ${item.always ? `<button class="button" ${attrs("always")}>${escape(t("Allow for session"))}</button>` : ""}
        <button class="button button--quiet inbox-row__deny" ${attrs("deny")}>${escape(t("Deny"))}</button>
        ${busy ? `<span class="inbox-row__spacer"></span><span class="inbox-row__busy">${escape(t("Sending…"))}</span>` : ""}
      </div>
    </li>`;
}

function terminalRow(row) {
  const { id, key } = row;
  return `
    <li class="inbox-row inbox-row--terminal" data-inbox-row="${escape(key)}">
      ${rowHead(row, "terminal")}
      <div class="inbox-row__ask">${escape(t("Waiting in the terminal"))}</div>
      <div class="inbox-row__hint">${escape(t("A terminal session is answered in its own terminal. Open it to reply."))}</div>
      <div class="inbox-row__actions">
        <span class="inbox-row__spacer"></span>
        <button class="button inbox-row__open" id="${domId(key, "open")}" data-inbox-open="${escape(id)}">${icons.open}${escape(t("Open"))}</button>
      </div>
    </li>`;
}

/// Who asks and how long ago. A chat row opens from here, leaving its
/// actions row to the answers.
function rowHead(row, glyph, jump = false) {
  const place = where(row.id);
  const stamp = new Date(row.at);
  return `
    <div class="inbox-row__head">
      <span class="inbox-row__icon">${icons[glyph] ?? icons.warn}</span>
      <span class="inbox-row__place">
        ${place.project ? `<span class="inbox-row__project">${escape(place.project)}</span><span class="inbox-row__dot">·</span>` : ""}
        <span class="inbox-row__session">${escape(place.title)}</span>
      </span>
      <time class="inbox-row__time" datetime="${escape(stamp.toISOString())}"
            title="${escape(t("Waiting since {time}", { time: stamp.toLocaleTimeString() }))}">${escape(waited(row.at))}</time>
      ${
        jump
          ? `<button class="inbox-row__jump" id="${domId(row.key, "open")}" data-inbox-open="${escape(row.id)}"
                     title="${escape(t("Open {session}", { session: place.title }))}">${icons.open}<span>${escape(t("Open"))}</span></button>`
          : ""
      }
    </div>`;
}

/// The panel under the tab bar's button. Esc closes it, as any menu.
export function approvalsPanel() {
  const rows = inboxItems();
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <section class="menu inbox" role="dialog" aria-label="${escape(t("Approvals"))}" tabindex="-1">
        <header class="inbox__head">
          <strong>${escape(t("Approvals"))}</strong>
          ${rows.length ? `<span class="inbox__count">${rows.length}</span>` : ""}
          <span class="section__spacer"></span>
          <span class="inbox__hint">${escape(t("Newest first"))}</span>
        </header>
        ${
          rows.length
            ? `<ul class="inbox__list" data-scroll="inbox">${rows.map((row) => (row.type === "chat" ? chatRow(row) : terminalRow(row))).join("")}</ul>`
            : `<div class="inbox__none">
                 <span class="inbox__none-icon">${icons.approvals}</span>
                 <strong>${escape(t("Nothing is waiting for you"))}</strong>
                 <span>${escape(t("When an agent asks to run a command or change a file, it shows here."))}</span>
               </div>`
        }
      </section>
    </div>`;
}
