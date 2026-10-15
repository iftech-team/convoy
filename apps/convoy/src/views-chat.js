// Chat sessions: the same agent conversation as the terminal, drawn as
// messages, tool cards and approvals.
//
// The window re-renders from state on every change, but a chat cannot be
// rebuilt that often: a reply streams in a few words at a time, the composer
// holds what is being typed, and the log has a scroll position and folded
// cards. So, like the terminals, each chat lives in an element of its own that
// is created once and moved into place, and only the messages that changed are
// redrawn. The conversation itself is state (`state.chats`), fed by the
// backend's `chat:event`s.

import { call, changed, done, ensureModels, isRunning, loadWorkspace, anySession, rememberChoice, state, toast } from "./state.js";
import { EFFORT_NAMES } from "./dialogs.js";
import { escape } from "./ui.js";
import { t } from "./i18n.js";
import { icon, icons } from "./icons.js";
import { highlight, markdown } from "./views-files.js";
import { helperCards, helperOf } from "./views-helpers.js";
import { skillsChip } from "./views-skills.js";
import { agentName } from "./task-defaults.js";
import { missingConversation, thinOutput } from "./terminal.js";

// ------------------------------------------------------------------ views --

export const VIEWS = ["chat", "terminal"];

/// How a session is shown: its own choice, else the Settings default, else
/// the terminal.
export function viewOf(item) {
  const view = item?.view ?? state.settings.default_view;
  return view === "chat" ? "chat" : "terminal";
}

/// A stopped terminal session stays a terminal: what it printed in this
/// window, or its saved output. Only when there is next to nothing to show
/// (see `thinOutput`) does its conversation, read from the agent's own
/// transcript, stand in. A conversation the agent could not find keeps the
/// terminal and its warning.
export function shownAsTranscript(item) {
  return (
    !!item &&
    viewOf(item) === "terminal" &&
    !!item.started &&
    !isRunning(item.id) &&
    !missingConversation(item.id) &&
    thinOutput(item.id)
  );
}

/// The conversation is read again the next time it is shown: a terminal run
/// added to it without the chat seeing.
export function historyChanged(id) {
  const chat = state.chats.get(id);
  if (chat) chat.loaded = false;
}

// ------------------------------------------------------------------ state --

state.chats ??= new Map();

const blank = () => ({
  items: [],
  index: new Map(),
  turn: false,
  approvals: new Set(),
  /// Requests answered while this window listened, so a list read from the
  /// backend a moment earlier cannot bring one back.
  answered: new Set(),
  loaded: false,
  loading: false,
  images: [],
  usage: null,
});

export function chatOf(id) {
  let chat = state.chats.get(id);
  if (!chat) {
    chat = blank();
    state.chats.set(id, chat);
  }
  return chat;
}

const add = (chat, item) => {
  chat.items.push(item);
  chat.index.set(item.key, item);
  return item;
};

/// A `user` echo takes over the message the composer put up while sending.
function adoptEcho(chat, event) {
  const key = `u:${event.id}`;
  if (chat.index.has(key)) return;
  const pending = chat.items.find((item) => item.kind === "user" && item.pending && item.text.trim() === String(event.text ?? "").trim());
  if (pending) {
    chat.index.delete(pending.key);
    pending.key = key;
    pending.pending = false;
    chat.index.set(key, pending);
    return;
  }
  add(chat, { key, kind: "user", text: event.text ?? "", images: [] });
}

/// Applies one event. `replay` is history: it draws what happened but says
/// nothing about what is happening now.
export function reduce(chat, event, replay = false) {
  switch (event.type) {
    case "session":
      chat.providerId = event.provider_id;
      break;
    case "user":
      adoptEcho(chat, event);
      break;
    case "turn_started":
      if (!replay) chat.turn = true;
      break;
    case "text": {
      const key = `a:${event.id}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "assistant", text: "" });
      item.text += event.delta ?? "";
      break;
    }
    case "text_done": {
      const key = `a:${event.id}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "assistant", text: "" });
      item.text = event.text ?? item.text;
      break;
    }
    case "thinking": {
      const key = `th:${event.id}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "thinking", text: "" });
      item.text += event.delta ?? "";
      break;
    }
    case "tool": {
      const key = `t:${event.id}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "tool", ok: null, output: null });
      Object.assign(item, { tool: event.kind ?? "other", name: event.name ?? "", title: event.title ?? event.name ?? "", detail: event.detail ?? "" });
      break;
    }
    case "tool_result": {
      const key = `t:${event.id}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "tool", tool: "other", name: "", title: t("Tool"), detail: "" });
      item.ok = event.ok !== false;
      item.output = event.output ?? "";
      break;
    }
    case "file_change": {
      const key = `f:${event.id}:${event.path}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "file" });
      Object.assign(item, { path: event.path ?? "", diff: event.diff ?? "" });
      break;
    }
    case "approval": {
      const key = `p:${event.request}`;
      const item = chat.index.get(key) ?? add(chat, { key, kind: "approval", request: event.request });
      Object.assign(item, {
        approval: event.kind ?? "tool",
        title: event.title ?? "",
        detail: event.detail ?? "",
        always: !!event.always,
      });
      if (replay) item.resolved = item.resolved ?? "answered";
      else {
        item.resolved = null;
        // When it started waiting, for the inbox; asked again keeps it.
        item.at ??= Date.now();
        chat.approvals.add(event.request);
      }
      break;
    }
    case "approval_resolved": {
      const item = chat.index.get(`p:${event.request}`);
      if (item) {
        item.resolved = item.answer ?? "answered";
        item.busy = false;
      }
      chat.approvals.delete(event.request);
      chat.answered.add(event.request);
      break;
    }
    case "turn_ended":
      if (!replay) {
        chat.turn = false;
        chat.usage = event.usage ?? null;
        settle(chat, "expired");
      }
      if (event.error) add(chat, { key: `e:${chat.items.length}`, kind: "error", message: event.error });
      break;
    case "error":
      add(chat, { key: `e:${chat.items.length}`, kind: "error", message: event.message ?? "" });
      break;
    case "models":
      if (Array.isArray(event.models) && event.models.length) chat.models = event.models;
      break;
    default:
      break;
  }
}

/// Approvals nobody can answer any more: the turn or the process ended.
function settle(chat, how) {
  for (const request of chat.approvals) {
    const item = chat.index.get(`p:${request}`);
    if (item && !item.resolved) {
      item.resolved = how;
      item.busy = false;
    }
  }
  chat.approvals.clear();
}

/// One `chat:event` from the backend. Returns the session's chat.
export function chatEvent({ id, event }) {
  const chat = chatOf(id);
  reduce(chat, event);
  refresh(id);
  return chat;
}

/// Approvals the backend still holds, read when the window opens or reloads:
/// they reach the chats as if their events had just arrived, keeping the time
/// each one started waiting. Returns the sessions that have one.
export function adoptApprovals(list) {
  const sessions = new Set();
  for (const entry of Array.isArray(list) ? list : []) {
    const { id, at, event } = entry ?? {};
    if (!id || event?.type !== "approval" || !event.request) continue;
    const chat = chatOf(id);
    if (chat.answered.has(event.request) || chat.index.get(`p:${event.request}`)?.resolved) continue;
    reduce(chat, event);
    const item = chat.index.get(`p:${event.request}`);
    if (item && Number.isFinite(at)) item.at = at;
    sessions.add(id);
  }
  for (const id of sessions) refresh(id);
  return [...sessions];
}

/// Every approval a chat is waiting on, in any session, oldest first.
export function waitingApprovals() {
  const list = [];
  for (const [id, chat] of state.chats) {
    for (const request of chat.approvals) {
      const item = chat.index.get(`p:${request}`);
      if (item && !item.resolved) list.push({ id, item });
    }
  }
  return list.sort((a, b) => (a.item.at ?? 0) - (b.item.at ?? 0));
}

export const waitingFor = (id) => (state.chats.get(id)?.approvals.size ?? 0) > 0;

/// The process ended: whatever it was doing stopped with it.
export function chatExited(id, exit) {
  const chat = state.chats.get(id);
  if (!chat) return;
  chat.turn = false;
  settle(chat, "expired");
  const how =
    exit.cause === "hibernated"
      ? t("session hibernated")
      : exit.cause === "stopped"
        ? t("session stopped")
        : t("session exited with code {code}", { code: exit.code });
  add(chat, { key: `n:${chat.items.length}`, kind: "note", text: how });
  refresh(id);
}

/// The conversation so far, read from the agent's transcript once per window.
function loadHistory(id) {
  const chat = chatOf(id);
  if (chat.loaded || chat.loading) return;
  chat.loading = true;
  call("chat_history", { id }).then((events) => {
    chat.loading = false;
    chat.loaded = true;
    const fresh = blank();
    for (const event of Array.isArray(events) ? events : []) reduce(fresh, event, true);
    // What arrived live while history loaded comes after it.
    for (const item of chat.items) if (!fresh.index.has(item.key)) add(fresh, item);
    chat.items = fresh.items;
    chat.index = fresh.index;
    chat.providerId ??= fresh.providerId;
    refresh(id);
  });
}

/// The models the agent listed the last time this session's chat ran, for a
/// window opened since. Asked once; a live `models` event replaces it.
function loadModels(id) {
  const chat = chatOf(id);
  if (chat.modelsAsked) return;
  chat.modelsAsked = true;
  call("chat_models", { id }).then((event) => {
    if (event?.type === "models" && !chat.models) {
      reduce(chat, event);
      refresh(id);
    }
  });
}

// ---------------------------------------------------------- model, effort --

/// What Claude Code takes when nothing more precise is known.
const CLAUDE_EFFORTS = ["low", "medium", "high", "xhigh", "max"];

const effortName = (level) => (EFFORT_NAMES[level] ? t(EFFORT_NAMES[level]) : level);

/// The models a chat offers: the agent's own list once it has reported it,
/// else the list read from its files for the session's account.
function modelsFor(id) {
  const chat = chatOf(id);
  if (chat.models?.length) return chat.models;
  const item = anySession(id);
  if (!item) return [];
  ensureModels(item.agent, item.profile_id ?? "");
  return state.models?.[`${item.agent}:${item.profile_id ?? ""}`] ?? [];
}

/// The levels `model` takes ("" is the default model). Before anything is
/// known, Claude's own levels; none when the model reports none.
function effortsFor(id, model) {
  const models = modelsFor(id);
  const chosen = model ? models.find((entry) => entry.id === model) : models.find((entry) => entry.is_default);
  if (chosen) return chosen.efforts ?? [];
  if (!model && models.length) return [...new Set(models.flatMap((entry) => entry.efforts ?? []))];
  return anySession(id)?.agent === "codex" ? [] : CLAUDE_EFFORTS;
}

function chips(id, blocked) {
  const item = anySession(id);
  if (!item) return "";
  const models = modelsFor(id);
  const model = item.model ?? "";
  const fallback = models.find((entry) => entry.is_default);
  const modelOptions = [["", fallback ? `${t("Default")} · ${fallback.name}` : t("Default")], ...models.map((entry) => [entry.id, entry.name])];
  if (model && !modelOptions.some(([value]) => value === model)) modelOptions.push([model, model]);
  const efforts = effortsFor(id, model);
  const effort = item.effort ?? "";
  const chosen = model ? models.find((entry) => entry.id === model) : fallback;
  const level = chosen?.default_effort;
  const effortOptions = [["", level ? `${t("Default")} · ${effortName(level)}` : t("Default")], ...efforts.map((value) => [value, effortName(value)])];
  if (effort && !efforts.includes(effort)) effortOptions.push([effort, effortName(effort)]);
  const options = (list, selected) =>
    list.map(([value, label]) => `<option value="${escape(value)}"${value === selected ? " selected" : ""}>${escape(label)}</option>`).join("");
  const disabled = blocked ? "disabled" : "";
  const modelChip = `<select class="composer__chip" data-chat-model aria-label="${escape(t("Model"))}" title="${escape(t("Model"))}" ${disabled}>${options(modelOptions, model)}</select>`;
  // A model that takes no effort level has nothing to choose.
  const effortChip = efforts.length
    ? `<select class="composer__chip" data-chat-effort aria-label="${escape(t("Reasoning effort"))}" title="${escape(t("Reasoning effort"))}" ${disabled}>${options(effortOptions, effort)}</select>`
    : "";
  return `<span class="composer__chips">${modelChip}${effortChip}</span>`;
}

/// Updates the session as shown at once, and puts it back if refused.
function setChoice(id, field, value) {
  const items = [state.sessions, state.allSessions].flatMap((list) => (list ?? []).filter((entry) => entry.id === id));
  const before = items.map((entry) => entry[field]);
  for (const entry of items) entry[field] = value;
  refresh(id);
  return () => {
    items.forEach((entry, at) => (entry[field] = before[at]));
    refresh(id);
  };
}

async function chooseModel(id, model) {
  const item = anySession(id);
  if (!item || (item.model ?? "") === model) return;
  const undo = setChoice(id, "model", model);
  if (!(await done("chat_set_model", { id, model: model || null }))) return undo();
  // The next new session for this agent starts from it, as /model does.
  rememberChoice(item.agent, model, item.effort ?? "");
  // A level the new model does not take goes back to its default.
  const effort = item.effort ?? "";
  if (effort && !effortsFor(id, model).includes(effort)) await chooseEffort(id, "");
}

async function chooseEffort(id, effort) {
  const item = anySession(id);
  if (!item || (item.effort ?? "") === effort) return;
  const undo = setChoice(id, "effort", effort);
  if (!(await done("chat_set_effort", { id, effort: effort || null }))) return undo();
  rememberChoice(item.agent, item.model ?? "", effort);
}

/// The conversation as text, for the saved output and for reviews.
export function chatText(id, limit = 60000) {
  const chat = state.chats.get(id);
  if (!chat) return "";
  const lines = [];
  for (const item of chat.items) {
    if (item.kind === "user") lines.push(`> ${item.text}`);
    else if (item.kind === "assistant") lines.push(item.text);
    else if (item.kind === "tool") lines.push(`• ${item.title}${item.ok === false ? " (failed)" : ""}`);
    else if (item.kind === "file") lines.push(`• ${t("Edited {path}", { path: item.path })}`);
    else if (item.kind === "error") lines.push(`! ${item.message}`);
    lines.push("");
  }
  return lines.join("\n").trim().slice(-limit);
}

const snapshots = new Map();
/// Keeps the chat as the session's saved output, as the terminal does.
export async function chatSnapshot(id) {
  const text = chatText(id);
  if (!text || snapshots.get(id) === text) return;
  snapshots.set(id, text);
  await call("session_snapshot", { id, text });
}

// --------------------------------------------------------------- markup ---

const glyphs = {
  send: icon('<path d="M8 13V3.5M3.8 7.5 8 3.3l4.2 4.2"/>', 15),
  image: icon('<rect x="2" y="3" width="12" height="10" rx="1.6"/><circle cx="6" cy="6.6" r="1.2"/><path d="m2.5 12 3.8-3.6 2.6 2.4 1.9-1.6 2.7 2.6"/>', 15),
  globe: icon('<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"/>', 13),
  thinking: icon('<path d="M8 2.5a4 4 0 0 0-2.3 7.3V11.5h4.6V9.8A4 4 0 0 0 8 2.5zM6.2 13.5h3.6"/>', 13),
  fail: icon('<circle cx="8" cy="8" r="5.5"/><path d="M6 6l4 4M10 6l-4 4"/>', 13),
  ok: icon('<path d="m3.5 8.5 3 3 6-7"/>', 13),
  chevron: icon('<path d="m6 4 4 4-4 4"/>', 12),
};

const TOOL_ICONS = {
  command: icons.terminal,
  edit: icons.edit,
  read: icons.doc,
  search: icons.search,
  web: glyphs.globe,
  task: icons.sparkles,
  other: icons.bolt,
};

/// `code` in a short line, the rest escaped.
const inline = (text) => escape(text).replace(/`([^`]+)`/g, "<code>$1</code>");

const diffCount = (diff) => {
  let added = 0;
  let removed = 0;
  for (const line of String(diff).split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) added += 1;
    else if (line.startsWith("-") && !line.startsWith("---")) removed += 1;
  }
  return { added, removed };
};

function toolCard(item, open, running) {
  const status =
    item.ok === null || item.ok === undefined
      ? running
        ? `<span class="chat-spinner" aria-label="${escape(t("Running"))}"></span>`
        : ""
      : item.ok
        ? `<span class="tool__status tool__status--ok" title="${escape(t("Done"))}">${glyphs.ok}</span>`
        : `<span class="tool__status tool__status--fail">${glyphs.fail} ${escape(t("Failed"))}</span>`;
  const command = item.tool === "command";
  const body = [
    item.detail
      ? `<pre class="tool__detail${command ? " tool__detail--command" : ""}">${command ? '<span class="tool__prompt">$</span> ' : ""}${escape(item.detail)}</pre>`
      : "",
    item.output !== null && item.output !== undefined
      ? item.output
        ? `<pre class="tool__output">${escape(item.output)}</pre>`
        : `<div class="tool__empty">${escape(t("No output"))}</div>`
      : "",
  ].join("");
  return `
    <details class="tool tool--${escape(item.tool)}${item.ok === false ? " tool--failed" : ""}" data-chat-key="${escape(item.key)}" ${open ? "open" : ""}>
      <summary class="tool__head">
        <span class="tool__chevron">${glyphs.chevron}</span>
        <span class="tool__icon">${TOOL_ICONS[item.tool] ?? TOOL_ICONS.other}</span>
        <span class="tool__title">${inline(item.title || item.name)}</span>
        ${status}
      </summary>
      ${body ? `<div class="tool__body">${body}</div>` : ""}
    </details>`;
}

/// The card names the file, so the diff's own header lines are left out.
function hunks(diff) {
  const lines = String(diff).split("\n");
  const first = lines.findIndex((line) => line.startsWith("@@"));
  if (first <= 0) return lines.join("\n");
  return [...lines.slice(0, first).filter((line) => !/^(diff |index |--- |\+\+\+ )/.test(line)), ...lines.slice(first)].join("\n");
}

function fileCard(item, open) {
  const { added, removed } = diffCount(item.diff);
  const slash = item.path.lastIndexOf("/");
  return `
    <details class="tool tool--file" data-chat-key="${escape(item.key)}" ${open ? "open" : ""}>
      <summary class="tool__head">
        <span class="tool__chevron">${glyphs.chevron}</span>
        <span class="tool__icon">${icons.edit}</span>
        <span class="tool__title tool__path" title="${escape(item.path)}"><span class="tool__dir">${escape(item.path.slice(0, slash + 1))}</span>${escape(item.path.slice(slash + 1))}</span>
        <span class="tool__counts"><span class="tool__added">+${added}</span> <span class="tool__removed">−${removed}</span></span>
      </summary>
      <div class="tool__body"><pre class="tool__diff">${highlight(hunks(item.diff), "diff").replaceAll("</span>\n<span", "</span><span")}</pre></div>
    </details>`;
}

const APPROVAL_ASKS = {
  command: "Run this command?",
  edit: "Make this change?",
  tool: "Use this tool?",
};

function approvalCard(item) {
  const answered = {
    allow: "Allowed",
    always: "Allowed for this session",
    deny: "Denied",
    answered: "Answered",
    expired: "No longer waiting",
  }[item.resolved];
  const actions = item.resolved
    ? `<div class="approval__answer approval__answer--${escape(item.resolved)}">${escape(t(answered ?? "Answered"))}</div>`
    : `<div class="approval__actions">
        <button class="button button--primary" data-chat-answer="allow" data-chat-request="${escape(item.request)}" ${item.busy ? "disabled" : ""}>${escape(t("Allow"))}</button>
        ${item.always ? `<button class="button" data-chat-answer="always" data-chat-request="${escape(item.request)}" ${item.busy ? "disabled" : ""}>${escape(t("Allow for session"))}</button>` : ""}
        <button class="button button--quiet approval__deny" data-chat-answer="deny" data-chat-request="${escape(item.request)}" ${item.busy ? "disabled" : ""}>${escape(t("Deny"))}</button>
      </div>`;
  return `
    <div class="approval${item.resolved ? " approval--resolved" : ""}" data-chat-key="${escape(item.key)}">
      <div class="approval__ask">${icons.warn}<span>${escape(t(APPROVAL_ASKS[item.approval] ?? APPROVAL_ASKS.tool))}</span></div>
      ${item.title ? `<div class="approval__title">${inline(item.title)}</div>` : ""}
      ${item.detail && !String(item.title).includes(item.detail.trim()) ? `<pre class="approval__detail">${escape(item.detail)}</pre>` : ""}
      ${actions}
    </div>`;
}

function itemMarkup(item, { open, last, turn }) {
  switch (item.kind) {
    case "user":
      return `
        <div class="msg msg--user${item.pending ? " msg--pending" : ""}">
          ${item.images?.length ? `<div class="msg__images">${item.images.map((image) => `<img src="data:${escape(image.media_type)};base64,${image.data}" alt="" />`).join("")}</div>` : ""}
          ${item.text ? `<div class="msg__bubble">${escape(item.text)}</div>` : ""}
        </div>`;
    case "assistant":
      return `<div class="msg msg--assistant${last && turn ? " msg--streaming" : ""}"><div class="md">${markdown(item.text)}</div></div>`;
    case "thinking":
      return `
        <details class="thinking" data-chat-key="${escape(item.key)}" ${open ? "open" : ""}>
          <summary>${glyphs.chevron}${glyphs.thinking}<span>${escape(t(last && turn ? "Thinking…" : "Thought"))}</span></summary>
          <div class="thinking__text">${escape(item.text)}</div>
        </details>`;
    case "tool":
      return toolCard(item, open, turn);
    case "file":
      return fileCard(item, open);
    case "approval":
      return approvalCard(item);
    case "error":
      return `<div class="chat-error" role="alert">${icons.warn}<span>${escape(item.message)}</span></div>`;
    case "note":
      return `<div class="chat-note"><span>${escape(item.text)}</span></div>`;
    default:
      return "";
  }
}

/// Folded by default: tools and thinking. Open by default: file changes.
const OPEN_BY_DEFAULT = new Set(["file"]);

function entries(id, host) {
  const chat = chatOf(id);
  const out = [];
  if (!chat.items.length) {
    const item = anySession(id);
    out.push({
      key: "__empty",
      html: chat.loading
        ? `<div class="chat-empty"><span class="chat-spinner"></span>${escape(t("Loading the conversation…"))}</div>`
        : `<div class="chat-empty">
             <div class="chat-empty__title">${escape(t(item?.started ? "No messages to show yet" : "Start the conversation"))}</div>
             <div class="chat-empty__text">${escape(t("Ask for a change, a fix or an explanation. The agent works in this session's folder."))}</div>
           </div>`,
    });
  }
  chat.items.forEach((item, at) => {
    const toggled = host.toggled.get(item.key);
    const open = toggled ?? OPEN_BY_DEFAULT.has(item.kind);
    out.push({ key: item.key, html: itemMarkup(item, { open, last: at === chat.items.length - 1, turn: chat.turn }) });
  });
  if (chat.turn) {
    const waiting = chat.approvals.size > 0;
    out.push({
      key: "__working",
      html: `<div class="chat-working${waiting ? " chat-working--waiting" : ""}" role="status">
               ${waiting ? icons.warn : '<span class="chat-dots"><i></i><i></i><i></i></span>'}
               <span>${escape(t(waiting ? "Waiting for your answer" : "Working…"))}</span>
             </div>`,
    });
  }
  return out;
}

// ------------------------------------------------------------------ hosts --

const hosts = new Map();

function composerBar(id) {
  const chat = chatOf(id);
  const item = anySession(id);
  const blocked = !!(item?.archived || item?.worktree_removed);
  return `
    <button class="composer__tool" data-chat-attach title="${escape(t("Attach images"))}" ${blocked ? "disabled" : ""}>${glyphs.image}</button>
    <button class="composer__tool" data-action="quick-menu" title="${escape(t("Quick commands"))}">${icons.bolt}</button>
    ${chips(id, blocked)}
    ${skillsChip(id, "composer")}
    <span class="composer__hint">${escape(t("Enter to send · Shift+Enter for a new line"))}</span>
    <span class="composer__spacer"></span>
    ${
      chat.turn
        ? `<button class="button composer__stop" data-chat-stop title="${escape(t("Stop the current turn"))}">${icons.stop}${escape(t("Stop"))}</button>`
        : `<button class="button button--primary composer__send" data-chat-send ${blocked ? "disabled" : ""} title="${escape(t("Send"))}">${glyphs.send}${escape(t("Send"))}</button>`
    }`;
}

function tray(id) {
  const chat = chatOf(id);
  return chat.images
    .map(
      (image, at) => `
      <span class="composer__image">
        <img src="data:${escape(image.media_type)};base64,${image.data}" alt="${escape(image.name ?? "")}" />
        <button data-chat-unattach="${at}" title="${escape(t("Remove"))}">${icons.close}</button>
      </span>`,
    )
    .join("");
}

function hostFor(id) {
  if (hosts.has(id)) return hosts.get(id);
  const root = document.createElement("div");
  root.className = "chat";
  root.dataset.chat = id;
  root.innerHTML = `
    <div class="chat__log"><div class="chat__inner" role="log" aria-live="polite"></div></div>
    <div class="chat__helpers" hidden></div>
    <div class="chat__composer">
      <div class="composer">
        <div class="composer__tray"></div>
        <textarea class="composer__input" id="chat-input-${escape(id).replace(/[^\w-]/g, "_")}" rows="1" spellcheck="true"></textarea>
        <div class="composer__bar"></div>
        <input type="file" accept="image/*" multiple hidden class="composer__file" />
      </div>
    </div>`;
  const host = {
    id,
    root,
    log: root.querySelector(".chat__log"),
    inner: root.querySelector(".chat__inner"),
    input: root.querySelector(".composer__input"),
    tray: root.querySelector(".composer__tray"),
    bar: root.querySelector(".composer__bar"),
    helpers: root.querySelector(".chat__helpers"),
    helpersHtml: "",
    file: root.querySelector(".composer__file"),
    rendered: new Map(),
    toggled: new Map(),
    stick: true,
    top: 0,
    barHtml: "",
    trayHtml: "",
    history: false,
  };
  hosts.set(id, host);
  wireHost(host);
  return host;
}

function wireHost(host) {
  const { id, root, log, input } = host;
  log.addEventListener("scroll", () => {
    if (!root.isConnected) return;
    host.top = log.scrollTop;
    host.stick = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
  });
  // `toggle` does not bubble; the capture phase sees every card's.
  root.addEventListener(
    "toggle",
    (event) => {
      const key = event.target.dataset?.chatKey;
      if (key) host.toggled.set(key, event.target.open);
    },
    true,
  );
  input.addEventListener("input", () => grow(host));
  input.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing || event.altKey) return;
    event.preventDefault();
    send(id);
  });
  input.addEventListener("paste", (event) => {
    const files = [...(event.clipboardData?.files ?? [])].filter((file) => file.type.startsWith("image/"));
    if (!files.length) return;
    if (!event.clipboardData.getData("text/plain")) event.preventDefault();
    attach(id, files);
  });
  root.addEventListener("dragover", (event) => {
    if ([...(event.dataTransfer?.types ?? [])].includes("Files")) event.preventDefault();
  });
  root.addEventListener("drop", (event) => {
    const files = [...(event.dataTransfer?.files ?? [])].filter((file) => file.type.startsWith("image/"));
    if (!files.length) return;
    event.preventDefault();
    attach(id, files);
  });
  host.file.addEventListener("change", () => {
    attach(id, [...host.file.files]);
    host.file.value = "";
  });
  root.addEventListener("change", (event) => {
    if (event.target.matches("[data-chat-model]")) chooseModel(id, event.target.value);
    else if (event.target.matches("[data-chat-effort]")) chooseEffort(id, event.target.value);
  });
  root.addEventListener("click", (event) => {
    const target = event.target.closest("[data-chat-send],[data-chat-stop],[data-chat-answer],[data-chat-attach],[data-chat-unattach]");
    if (!target) return;
    const data = target.dataset;
    if (data.chatSend !== undefined) return send(id);
    if (data.chatStop !== undefined) return done("chat_interrupt", { id });
    if (data.chatAttach !== undefined) return host.file.click();
    if (data.chatUnattach !== undefined) {
      chatOf(id).images.splice(Number(data.chatUnattach), 1);
      return sync(host);
    }
    if (data.chatAnswer) return answer(id, data.chatRequest, data.chatAnswer);
  });
}

function grow(host) {
  const { input } = host;
  input.style.height = "auto";
  input.style.height = `${Math.min(input.scrollHeight, 220)}px`;
}

const MAX_IMAGE = 5 * 1024 * 1024;

function attach(id, files) {
  for (const file of files) {
    if (!file.type.startsWith("image/")) continue;
    if (file.size > MAX_IMAGE) {
      toast(t("{name} is larger than 5 MB.", { name: file.name || t("The image") }), "error");
      continue;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const [head, data] = String(reader.result).split(",", 2);
      const media_type = /data:([^;]+)/.exec(head)?.[1] ?? file.type;
      chatOf(id).images.push({ media_type, data, name: file.name });
      refresh(id);
    };
    reader.readAsDataURL(file);
  }
}

async function send(id) {
  const host = hostFor(id);
  const chat = chatOf(id);
  const text = host.input.value.trim();
  const images = chat.images;
  if ((!text && !images.length) || chat.turn || chat.sending) return;
  const item = anySession(id);
  if (item?.archived || item?.worktree_removed) return;
  chat.sending = true;
  const local = add(chat, { key: `local:${chat.items.length}:${Date.now()}`, kind: "user", text, images, pending: true });
  chat.images = [];
  host.input.value = "";
  grow(host);
  host.stick = true;
  refresh(id);
  const sent = await done("chat_send", { id, text, images: images.map(({ media_type, data }) => ({ media_type, data })) });
  chat.sending = false;
  if (!sent) {
    // Nothing went: the message comes back to the composer.
    chat.items = chat.items.filter((entry) => entry !== local);
    chat.index.delete(local.key);
    if (!host.input.value) host.input.value = text;
    if (!chat.images.length) chat.images = images;
    grow(host);
    refresh(id);
    return;
  }
  if (!isRunning(id)) await loadWorkspace();
}

/// Answers an approval, from its card or from the inbox: both read the same
/// item, so either shows the other's answer. The card stays busy until the
/// backend's `approval_resolved`; a failure is a toast and the card is free
/// again. The whole window redraws after each step, for the inbox and badge.
export async function answer(id, request, decision) {
  const chat = chatOf(id);
  const item = chat.index.get(`p:${request}`);
  if (!item || item.resolved || item.busy) return false;
  item.busy = true;
  item.answer = decision;
  refresh(id);
  changed();
  if (!(await done("chat_answer", { id, request, decision }))) {
    item.busy = false;
    item.answer = null;
    refresh(id);
    changed();
    return false;
  }
  return true;
}

/// Text put into a chat's composer, not sent: dropped paths, feedback, a
/// quick command that does not submit.
export function insertDraft(id, text, { replace = false } = {}) {
  const host = hostFor(id);
  const input = host.input;
  if (replace || !input.value) input.value = text;
  else {
    const start = input.selectionStart ?? input.value.length;
    const end = input.selectionEnd ?? start;
    input.value = input.value.slice(0, start) + text + input.value.slice(end);
    input.setSelectionRange(start + text.length, start + text.length);
  }
  grow(host);
  if (host.root.isConnected) input.focus();
}

/// Sends text as the user would, from a quick command.
export async function sendText(id, text) {
  const host = hostFor(id);
  const kept = host.input.value;
  host.input.value = text;
  await send(id);
  if (!host.input.value) host.input.value = kept;
  grow(host);
}

/// Redraws what changed in one chat's messages and composer.
function sync(host) {
  const { id, inner } = host;
  const chat = chatOf(id);
  const keep = new Set();
  let previous = null;
  for (const { key, html } of entries(id, host)) {
    keep.add(key);
    let record = host.rendered.get(key);
    if (!record || record.html !== html) {
      const template = document.createElement("template");
      template.innerHTML = html.trim();
      const element = template.content.firstElementChild ?? document.createElement("div");
      if (record) record.el.replaceWith(element);
      record = { el: element, html };
      host.rendered.set(key, record);
    }
    const wanted = previous ? previous.nextElementSibling : inner.firstElementChild;
    if (wanted !== record.el) inner.insertBefore(record.el, wanted);
    previous = record.el;
  }
  for (const [key, record] of host.rendered) {
    if (!keep.has(key)) {
      record.el.remove();
      host.rendered.delete(key);
    }
  }

  // A manager's helpers, live; a helper's manager, one click away.
  const helpers = helperCards(id) || helperOf(id);
  if (helpers !== host.helpersHtml) {
    host.helpersHtml = helpers;
    host.helpers.innerHTML = helpers;
    host.helpers.hidden = !helpers;
  }
  const bar = composerBar(id);
  if (bar !== host.barHtml) {
    host.barHtml = bar;
    host.bar.innerHTML = bar;
  }
  const images = tray(id);
  if (images !== host.trayHtml) {
    host.trayHtml = images;
    host.tray.innerHTML = images;
    host.tray.hidden = !images;
  }
  const item = anySession(id);
  const name = agentName(item?.agent);
  host.input.placeholder = item?.archived
    ? t("This session is archived. Restore it to continue.")
    : item?.worktree_removed
      ? t("This session's worktree was removed.")
      : chat.turn
        ? t("The agent is working. Stop it to change course.")
        : t("Message {agent}…", { agent: name });
  host.input.disabled = !!(item?.archived || item?.worktree_removed);
  if (host.stick) host.log.scrollTop = host.log.scrollHeight;
}

const pending = new Set();
/// Redraws a chat on the next frame, once however many events arrived.
export function refresh(id) {
  const host = hosts.get(id);
  if (!host || pending.has(id)) return;
  pending.add(id);
  requestAnimationFrame(() => {
    pending.delete(id);
    if (host.root.isConnected) sync(host);
  });
}

/// Puts a session's chat where the workbench left room for it.
export function mountChat(id, slotSelector = "#chat-host") {
  const slot = document.querySelector(slotSelector);
  if (!slot) return;
  const host = hostFor(id);
  loadHistory(id);
  loadModels(id);
  slot.append(host.root);
  sync(host);
  // A detached element forgets its scroll offset.
  host.log.scrollTop = host.stick ? host.log.scrollHeight : host.top;
  grow(host);
  requestAnimationFrame(() => {
    const active = document.activeElement;
    const typing = active && active !== document.body && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName);
    if (!state.dialog && !state.menu && state.sessionId === id && !typing && !host.input.disabled) host.input.focus();
  });
}
