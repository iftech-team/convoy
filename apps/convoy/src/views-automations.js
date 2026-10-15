// The Automations tab and its editor: a prompt a project's agent runs on its
// own, daily, on chosen weekdays or every few hours. When one is due, what it
// does and how a run went are the backend's rules (convoy-core); this draws
// them.

import { button, choice, escape, field, modal, setting, textArea, textInput, toggle } from "./ui.js";
import { agentIcon, icons } from "./icons.js";
import { language, t } from "./i18n.js";
import { state } from "./state.js";
import { accountOptions, effortSelect, modelSelect } from "./dialogs.js";
import { skillPicker } from "./views-skills.js";

export const DAYS = [
  ["mon", "Mon"],
  ["tue", "Tue"],
  ["wed", "Wed"],
  ["thu", "Thu"],
  ["fri", "Fri"],
  ["sat", "Sat"],
  ["sun", "Sun"],
];
const WORKDAYS = ["mon", "tue", "wed", "thu", "fri"];
const HOURS = [1, 2, 3, 4, 6, 8, 12, 24];

const STATUS = {
  started: ["Running", "running"],
  done: ["Done", "done"],
  failed: ["Failed", "failed"],
  skipped: ["Skipped", "idle"],
};

/// A time as the user's locale writes it, from `HH:MM`.
function clock(hhmm) {
  const [hours, minutes] = String(hhmm ?? "").split(":").map(Number);
  if (!Number.isFinite(hours) || !Number.isFinite(minutes)) return hhmm ?? "";
  const date = new Date(2026, 0, 5, hours, minutes);
  return date.toLocaleTimeString(language(), { hour: "2-digit", minute: "2-digit" });
}

/// "Weekdays at 09:00", "Every 4 hours from 09:00".
export function scheduleText(schedule) {
  if (!schedule) return "";
  if (schedule.kind === "daily") return t("Daily at {time}", { time: clock(schedule.at) });
  if (schedule.kind === "weekdays") {
    const days = schedule.days ?? [];
    const sorted = DAYS.map(([key]) => key).filter((key) => days.includes(key));
    const time = clock(schedule.at);
    if (sorted.length === 7) return t("Daily at {time}", { time });
    if (sorted.length === 5 && WORKDAYS.every((day) => sorted.includes(day))) return t("Weekdays at {time}", { time });
    if (sorted.length === 2 && sorted.includes("sat") && sorted.includes("sun")) return t("Weekends at {time}", { time });
    const names = sorted.map((key) => t(DAYS.find(([day]) => day === key)[1])).join(", ");
    return t("{days} at {time}", { days: names, time });
  }
  const hours = Number(schedule.hours) || 1;
  const every = hours === 1 ? t("Every hour") : t("Every {count} hours", { count: hours });
  return schedule.from && schedule.from !== "00:00" ? t("{every} from {time}", { every, time: clock(schedule.from) }) : every;
}

/// "in 5 min", "in 3 h", "in 2 days"; "now" when it is owed.
export function relative(seconds, now = Date.now()) {
  const minutes = Math.round((seconds * 1000 - now) / 60000);
  if (minutes <= 0) return t("now");
  if (minutes < 60) return t("in {count} min", { count: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 36) return t("in {count} h", { count: hours });
  return t("in {count} days", { count: Math.round(hours / 24) });
}

/// The exact time, for a tooltip or the editor's preview.
export const exact = (seconds) =>
  new Date(seconds * 1000).toLocaleString(language(), { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

// ------------------------------------------------------------------ list --

function lastResult(item) {
  const run = item.last_run;
  if (!run) return `<span class="auto-last auto-last--none">${escape(t("Never run"))}</span>`;
  const [label, tone] = STATUS[run.status] ?? [run.status, "idle"];
  const when = new Date(run.at * 1000).toLocaleString(language(), { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  const tip = [when, run.note ? t(run.note) : "", run.manual ? t("Run now") : ""].filter(Boolean).join(" · ");
  const chip = `<span class="state state--${tone} auto-last__chip" title="${escape(tip)}"><span class="state__dot"></span>${escape(t(label))}</span>`;
  const known = run.session_id && state.allSessions.some((session) => session.id === run.session_id);
  const link = known
    ? `<button class="auto-last__open" data-open="${escape(run.session_id)}" title="${escape(t("Open its session"))}">${escape(when)}</button>`
    : `<span class="auto-last__when">${escape(when)}</span>`;
  return `<span class="auto-last">${chip}${link}</span>`;
}

function automationRow(item) {
  const next = item.enabled && item.next_run
    ? `<span title="${escape(exact(item.next_run))}">${escape(t("next {when}", { when: relative(item.next_run) }))}</span>`
    : `<span>${escape(item.enabled ? t("not scheduled") : t("paused"))}</span>`;
  const firstLine = String(item.prompt ?? "").split("\n").find((line) => line.trim()) ?? "";
  return `
    <div class="row auto-row${item.enabled ? "" : " auto-row--off"}" data-automation="${escape(item.id)}">
      <span class="row__mark">${agentIcon(item.agent, 16)}</span>
      <div class="row__body">
        <div class="row__title">${escape(item.title)}
          ${item.worktree ? `<span class="badge badge--muted auto-row__tag">${icons.branch}${escape(t("worktree"))}</span>` : ""}
          ${item.view === "chat" ? `<span class="badge badge--muted auto-row__tag">${escape(t("Chat"))}</span>` : ""}
        </div>
        <div class="row__meta auto-row__schedule">${icons.clock}<span>${escape(scheduleText(item.schedule))}</span><span aria-hidden="true">·</span>${next}</div>
        <div class="row__meta auto-row__prompt" title="${escape(item.prompt)}">${escape(firstLine)}</div>
      </div>
      <div class="auto-row__last">${lastResult(item)}</div>
      <div class="row__actions">
        ${toggle(`auto-enabled:${item.id}`, item.enabled, item.enabled ? t("Pause automation") : t("Turn automation on"))}
        ${button({ label: item.running ? t("Running…") : t("Run now"), icon: "play", data: { auto: "run", "auto-id": item.id }, disabled: item.running })}
        ${button({ icon: "edit", kind: "quiet", title: t("Edit"), data: { auto: "edit", "auto-id": item.id } })}
        ${button({ icon: "trash", kind: "quiet", title: t("Delete"), data: { auto: "delete", "auto-id": item.id } })}
      </div>
    </div>`;
}

export function automationsView() {
  if (!state.automationsLoaded) return `<div class="list__none">${escape(t("Reading…"))}</div>`;
  const list = (state.allAutomations ?? []).filter((item) => item.project_id === state.projectId);
  const create = button({ label: t("New automation"), icon: "plus", data: { auto: "new" }, kind: "primary" });
  if (!list.length) {
    return `
      <div class="auto-empty">
        <span class="auto-empty__mark">${icons.clock}</span>
        <h2 class="auto-empty__title">${escape(t("No automations yet"))}</h2>
        <p class="auto-empty__text">${escape(t("An automation runs a prompt on a schedule — a morning triage, a nightly dependency check — each run in a new session you can open later."))}</p>
        <p class="auto-empty__text auto-empty__text--faint">${escape(t("They run while Convoy is open, even with its window closed. A time missed while it was quit can run once when it opens."))}</p>
        <div class="centre-action">${create}</div>
      </div>`;
  }
  return `
    <div class="section">
      <h2 class="section__title">${escape(t("Automations"))}
        <span class="section__count">${list.length}</span>
      </h2>
      <span class="section__spacer"></span>
      ${create}
    </div>
    <p class="section__hint">${escape(t("Each run starts a new session in this project. They run while Convoy is open, even with its window closed."))}</p>
    <div class="list" data-scroll="automations">${list.map(automationRow).join("")}</div>`;
}

// ---------------------------------------------------------------- editor --

/// The schedule the editor's fields describe, as the backend reads it.
export function draftSchedule(dialog) {
  if (dialog.schedule_kind === "weekdays") return { kind: "weekdays", at: dialog.at, days: DAYS.map(([key]) => key).filter((key) => dialog.days.includes(key)) };
  if (dialog.schedule_kind === "every") {
    const from = dialog.from && dialog.from !== "00:00" ? { from: dialog.from } : {};
    return { kind: "every", hours: Number(dialog.hours) || 0, ...from };
  }
  return { kind: "daily", at: dialog.at };
}

export function previewBlock(dialog) {
  const body = dialog.previewError
    ? `<span class="auto-preview__error">${escape(t(dialog.previewError))}</span>`
    : dialog.preview?.length
      ? `<ol>${dialog.preview.map((at) => `<li><span>${escape(exact(at))}</span><span class="auto-preview__in">${escape(relative(at))}</span></li>`).join("")}</ol>`
      : `<span class="auto-preview__none">${escape(t("Working it out…"))}</span>`;
  return `<div class="auto-preview" aria-live="polite"><div class="auto-preview__label">${escape(t("Next runs"))}</div>${body}</div>`;
}

export function automationEditor(dialog) {
  const target = state.projects.find((item) => item.id === dialog.project_id);
  const agents = [
    ["claude", "Claude Code", agentIcon("claude", 15)],
    ["codex", "Codex", agentIcon("codex", 15)],
    ["gemini", "Gemini", agentIcon("gemini", 15)],
  ];
  const kind = dialog.schedule_kind;
  const timeInput = (id, value) => `<input id="${id}" type="time" value="${escape(value ?? "")}" required />`;
  const scheduleFields =
    kind === "every"
      ? `<div class="field-row">
           ${field(t("Every"), `<select id="auto-hours" class="field__select">${HOURS.map((hours) => `<option value="${hours}"${Number(dialog.hours) === hours ? " selected" : ""}>${escape(hours === 1 ? t("hour") : t("{count} hours", { count: hours }))}</option>`).join("")}</select>`)}
           ${field(t("Starting at"), timeInput("auto-from", dialog.from), t("The count starts again each day."))}
         </div>`
      : `${field(t("At"), timeInput("auto-at", dialog.at))}
         ${
           kind === "weekdays"
             ? `<div class="field"><span class="field__label">${escape(t("On"))}</span>
                 <div class="auto-days" role="group" aria-label="${escape(t("Days"))}">${DAYS.map(
                   ([key, name]) => `<button type="button" class="auto-day" data-auto-day="${key}" aria-pressed="${dialog.days.includes(key)}">${escape(t(name))}</button>`,
                 ).join("")}</div></div>`
             : ""
         }`;
  return modal({
    title: dialog.id ? t("Edit automation") : t("New automation"),
    hint: target ? target.title : "",
    wide: true,
    body: `
      ${field(t("Title"), textInput("auto-title", dialog.title, t("Morning triage")))}
      ${field(t("Prompt"), textArea("auto-prompt", dialog.prompt, t("What the agent does each run. It starts fresh every time, so say everything it needs."), 5))}
      <div class="field-row field-row--agent">
        ${field(t("Agent"), choice("auto-agent", agents, dialog.agent))}
        ${field(t("Open in"), choice("auto-view", [["chat", t("Chat")], ["terminal", t("Terminal")]], dialog.view))}
      </div>
      <div class="field-row field-row--three">
        ${field(t("Account"), `<select id="auto-account" class="field__select">${accountOptions(dialog.agent).map(([value, label]) => `<option value="${escape(value)}"${value === (dialog.account ?? "") ? " selected" : ""}>${escape(label)}</option>`).join("")}</select>`)}
        ${field(t("Model"), modelSelect("auto-model", dialog.agent, dialog.account ?? "", dialog.model ?? ""))}
        ${field(t("Effort"), effortSelect("auto-effort", dialog.agent, dialog.account ?? "", dialog.model ?? "", dialog.effort ?? ""))}
      </div>
      ${skillPicker(dialog.skills ?? [], dialog.agent, dialog.project_id)}
      <div class="sheet-box auto-schedule">
        <div class="auto-schedule__head">
          <span class="field__label">${escape(t("Schedule"))}</span>
          ${choice("auto-kind", [["daily", t("Daily")], ["weekdays", t("On days")], ["every", t("Every N hours")]], kind)}
        </div>
        <div class="auto-schedule__body">
          <div class="auto-schedule__fields">${scheduleFields}</div>
          ${previewBlock(dialog)}
        </div>
      </div>
      <div class="sheet-box">
        ${setting(t("Run in a fresh worktree"), t("Each run gets its own checkout and branch (automation/…), so it never touches your working copy. Shared files are copied; the setup command is not run."), toggle("auto-worktree", dialog.worktree, t("Run in a fresh worktree")))}
        ${setting(t("Run missed times when Convoy opens"), t("If Convoy was quit or the computer asleep at the time, run once when it is back — never more than once."), toggle("auto-catchup", dialog.catch_up, t("Run missed times when Convoy opens")))}
      </div>`,
    note: escape(t("Runs use the agent's approval settings: without Yolo in Settings → Agents, a run waits for you when it asks.")),
    foot: `
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${button({ label: dialog.busy ? t("Saving…") : dialog.id ? t("Save") : t("Create automation"), action: "automation-save", kind: "primary", disabled: !!dialog.busy })}`,
  });
}

// --------------------------------------------------------------- activity --

const ACTIVITY = [
  [/^Automation run: (.*)$/s, "automation started"],
  [/^Automation finished: (.*)$/s, "automation finished"],
  [/^Automation failed: (.*)$/s, "automation failed"],
];

/// `[label, detail]` for an automation's activity row, or null.
export function automationActivity(detail) {
  for (const [pattern, label] of ACTIVITY) {
    const found = pattern.exec(String(detail ?? ""));
    if (found) return [label, found[1]];
  }
  return null;
}

/// The soonest scheduled run of any project, for Home.
export function nextAutomation() {
  const upcoming = (state.allAutomations ?? []).filter((item) => item.enabled && item.next_run);
  upcoming.sort((a, b) => a.next_run - b.next_run);
  return upcoming[0] ?? null;
}
