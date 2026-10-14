// Every dialog. `state.dialog` holds the kind and its draft; this renders it.

import {
  button,
  choice,
  escape,
  field,
  group,
  modal,
  plural,
  setting,
  textArea,
  textInput,
  toggle,
} from "./ui.js";
import { markdown } from "./views-files.js";
import { TASK_STATUSES, syncState, syncSummary } from "./views-planning.js";
import { when } from "./views-history.js";
import { state, project, session } from "./state.js";
import { agentIcon, icons } from "./icons.js";
import { t } from "./i18n.js";
import { automationEditor } from "./views-automations.js";
import { autoSkills, skillEditor, skillPicker } from "./views-skills.js";

const AGENTS = [
  ["claude", "Claude Code", agentIcon("claude", 15)],
  ["codex", "Codex", agentIcon("codex", 15)],
];

export function dialogView() {
  const dialog = state.dialog;
  if (!dialog) return "";
  const render = VIEWS[dialog.kind];
  return render ? render(dialog) : "";
}

const foot = (confirmLabel, action, kind = "primary") => `
  ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
  ${button({ label: confirmLabel, action, kind })}`;

// ---------------------------------------------------------------- sessions --

/// The account a new session signs in with: the system login or a profile.
export const accountOptions = (agent) => [
  ["", t("System login")],
  ...(state.profiles ?? []).filter((item) => item.agent === agent).map((item) => [item.id, item.label]),
];

const select = (id, options, selected, disabled = false) =>
  `<select id="${id}" class="field__select"${disabled ? " disabled" : ""}>${options
    .map(([value, label]) => `<option value="${escape(value)}"${value === selected ? " selected" : ""}>${escape(label)}</option>`)
    .join("")}</select>`;

/// The agent's models, read from its own files for the chosen account, with
/// the CLI's default first. A model set earlier by name stays listed.
export function modelSelect(id, agent, profileId, value) {
  const models = state.models?.[`${agent}:${profileId}`];
  const options = [["", t("Default")], ...(models ?? []).map((model) => [model.id, model.name])];
  if (value && !options.some(([option]) => option === value)) options.push([value, value]);
  if (!models) options.push(["__loading", t("Loading models…")]);
  return select(id, options, value);
}

/// How hard the agent thinks, from the levels the chosen model takes as the
/// agent itself lists them; with the default model, every level any of its
/// models takes. A level the model does not take falls back to the default.
export const EFFORT_NAMES = {
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
  ultra: "Ultra",
};

export function effortLevels(agent, profileId, modelId) {
  const models = state.models?.[`${agent}:${profileId}`] ?? [];
  const chosen = models.find((model) => model.id === modelId);
  const levels = chosen ? chosen.efforts ?? [] : models.flatMap((model) => model.efforts ?? []);
  return Object.keys(EFFORT_NAMES).filter((level) => levels.includes(level))
    .concat(levels.filter((level) => !(level in EFFORT_NAMES)))
    .filter((level, index, all) => all.indexOf(level) === index);
}

export function effortSelect(id, agent, profileId, modelId, value) {
  const levels = effortLevels(agent, profileId, modelId);
  const options = [["", t("Default")], ...levels.map((level) => [level, EFFORT_NAMES[level] ? t(EFFORT_NAMES[level]) : level])];
  return select(id, options, levels.includes(value) ? value : "", levels.length === 0);
}

/// As the macOS app's sheet: any project, an optional name, an isolated
/// worktree from a chosen ref, the agent and its account, and a first
/// message. Creating it starts it.
/// Earlier conversations picked as background for a new session: chips for
/// those picked, and a menu of this project's sessions, its saved
/// conversations and other projects' sessions.
function knowledgeField(dialog) {
  const picked = dialog.knowledge ?? [];
  const taken = new Set(picked.map((item) => item.key));
  const names = new Map(state.projects.map((item) => [item.id, item.title]));
  const sessions = state.allSessions.filter((item) => !taken.has(`s:${item.id}`));
  const option = (key, label) => `<option value="${escape(key)}">${escape(label)}</option>`;
  const group = (label, options) => (options.length ? `<optgroup label="${escape(label)}">${options.join("")}</optgroup>` : "");
  const here = sessions.filter((item) => item.project_id === dialog.projectId).map((item) => option(`s:${item.id}`, item.title));
  const saved = (dialog.saved ?? [])
    .filter((entry) => !taken.has(`c:${entry.agent}:${entry.provider_id}`))
    .map((entry) => option(`c:${entry.agent}:${entry.provider_id}`, entry.title || t("Untitled conversation")));
  const elsewhere = sessions
    .filter((item) => item.project_id !== dialog.projectId)
    .map((item) => option(`s:${item.id}`, `${names.get(item.project_id) ?? ""} · ${item.title}`));
  const chips = picked
    .map(
      (item) => `<span class="knowledge__chip">${agentIcon(item.agent, 13)}<span>${escape(item.title)}</span>
        <button type="button" data-knowledge-remove="${escape(item.key)}" aria-label="${escape(t("Remove {name}", { name: item.title }))}">×</button></span>`,
    )
    .join("");
  const choices = here.length + saved.length + elsewhere.length;
  return field(
    t("Knowledge from earlier sessions (optional)"),
    `${chips ? `<div class="knowledge">${chips}</div>` : ""}
     <select id="draft-knowledge" class="field__select"${choices ? "" : " disabled"}>
       <option value="">${escape(choices ? t("Add a conversation…") : t("No earlier conversations"))}</option>
       ${group(t("This project"), here)}${group(t("Not in Convoy yet"), saved)}${group(t("Other projects"), elsewhere)}
     </select>
     <span class="field__note">${escape(t("What was said in them is given to the agent as a file to read when relevant."))}</span>`,
  );
}

function newSession(dialog) {
  const target = state.projects.find((item) => item.id === dialog.projectId);
  const refs = dialog.refs ?? [];
  const agentName = dialog.agent === "claude" ? "Claude Code" : "Codex";
  const yolo = dialog.agent === "claude" ? state.settings.yolo_claude : state.settings.yolo_codex;
  const worktree = dialog.repo
    ? `<div class="sheet-box">
        ${setting(
          t("Isolated git worktree"),
          t("Own checkout and branch, so parallel sessions never touch each other's files."),
          toggle("worktree", dialog.worktree, t("Isolated git worktree")),
        )}
        ${
          dialog.worktree
            ? `<div class="field-row">
                ${field(
                  t("Create from"),
                  `<input id="draft-base" list="draft-refs" value="${escape(dialog.base)}"
                          placeholder="${escape(dialog.defaultBase || t("current branch"))}" spellcheck="false" />
                   <datalist id="draft-refs">${refs
                     .map((ref) => `<option value="${escape(ref.replace(/^tag:/, ""))}">${escape(t(ref.startsWith("tag:") ? "tag" : ref.startsWith("origin/") ? "remote" : "branch"))}</option>`)
                     .join("")}</datalist>`,
                )}
                ${field(t("Branch"), textInput("draft-branch", dialog.branch, "branch-name"))}
              </div>`
            : ""
        }
      </div>`
    : "";
  const advanced = dialog.advanced
    ? `<div class="sheet-box">
        ${field(t("Note"), textInput("draft-notes", dialog.notes, t("Where you left off, links, context for later")))}
        ${
          dialog.repo && dialog.worktree
            ? field(
                t("Worktree setup"),
                `${choice("setup", [["run", t("Run")], ["skip", t("Skip")]], dialog.setup)}
                 ${
                   dialog.setupPreview
                     ? `<pre class="sheet-preview">${escape(dialog.setupPreview)}</pre>`
                     : `<span class="field__note">${escape(t("None configured. Add it in Project settings or Settings → Git."))}</span>`
                 }`,
              )
            : ""
        }
        ${setting(t("Keep this dialog open to create more"), "", toggle("keepOpen", dialog.keepOpen, t("Keep open")))}
      </div>`
    : "";
  return modal({
    title: t("New session"),
    hint: target?.path ?? "",
    body: `
      ${field(
        t("Project"),
        select("draft-project", state.projects.map((item) => [item.id, item.title]), dialog.projectId),
      )}
      ${field(t("Name (optional)"), textInput("draft-title", dialog.title, t("Leave empty to name it after the first message")))}
      ${worktree}
      ${field(t("Agent"), choice("agent", AGENTS, dialog.agent))}
      ${field(t("Open in"), choice("view", [["chat", t("Chat")], ["terminal", t("Terminal")]], dialog.view ?? "terminal"))}
      ${skillPicker(dialog.skills ?? [], dialog.agent, dialog.projectId)}
      <div class="sheet-box sheet-box--manager">
        ${setting(
          t("Manager — can start helper sessions"),
          t("The agent can hand work to other agents in this project from its shell, wait for them and read their answers. At most 4 run at once."),
          toggle("manager", dialog.manager, t("Manager")),
        )}
      </div>
      ${field(t("Account"), select("draft-account", accountOptions(dialog.agent), dialog.account ?? ""))}
      <div class="field-row">
        ${field(t("Model"), modelSelect("draft-model", dialog.agent, dialog.account ?? "", dialog.model ?? ""))}
        ${field(t("Effort"), effortSelect("draft-effort", dialog.agent, dialog.account ?? "", dialog.model ?? "", dialog.effort ?? ""))}
      </div>
      ${field(
        t("First message (optional)"),
        textArea("draft-prompt", dialog.prompt, t("Sent once, when the agent starts. Resuming never replays it.")),
      )}
      ${knowledgeField(dialog)}
      <button class="sheet-disclosure" data-toggle="advanced" aria-expanded="${!!dialog.advanced}">
        ${dialog.advanced ? "▾" : "▸"} ${escape(t("Advanced"))}
      </button>
      ${advanced}`,
    note: escape(
      yolo
        ? t("Starts without permission prompts (Yolo is on in Settings → Agents).")
        : t("Uses your installed agent, its login and its approval settings."),
    ),
    foot: foot(dialog.busy ? t("Starting…") : t("Start {agent}", { agent: agentName }), "create-session"),
  });
}

const editSession = (dialog) =>
  modal({
    title: t("Edit session"),
    hint: dialog.running ? t("Running — the provider ID is fixed until it stops.") : "",
    body: `
      ${field(t("Name"), textInput("draft-title", dialog.title))}
      ${field(
        t("Provider session ID"),
        `<input id="draft-provider" value="${escape(dialog.provider_id)}"
                spellcheck="false" ${dialog.running ? "disabled" : ""} />`,
        t("What makes an exact resume possible."),
      )}
      ${field(t("Notes"), textArea("draft-notes", dialog.notes, "", 5))}`,
    foot: foot(t("Save"), "save-session"),
  });

/// Just the name, selected, so typing replaces it and Enter saves.
const renameSession = (dialog) =>
  modal({
    title: t("Rename session"),
    body: field(
      t("Name"),
      `<input id="rename-title" value="${escape(dialog.title)}" maxlength="200" spellcheck="false" autocomplete="off" />`,
    ),
    foot: foot(t("Rename"), "rename-session"),
  });

const startReview = (dialog) =>
  modal({
    title: t("Start review"),
    hint: t("A {agent} session will review this work in the same folder. It is told not to edit files.", {
      agent: dialog.agent === "claude" ? "Claude Code" : "Codex",
    }),
    wide: true,
    body: `
      ${field(t("Reviewer"), choice("agent", AGENTS, dialog.agent))}
      ${field(t("Account"), select("draft-account", accountOptions(dialog.agent), dialog.account ?? ""))}
      <div class="field-row">
        ${field(t("Model"), modelSelect("draft-model", dialog.agent, dialog.account ?? "", dialog.model ?? ""))}
        ${field(t("Effort"), effortSelect("draft-effort", dialog.agent, dialog.account ?? "", dialog.model ?? "", dialog.effort ?? ""))}
      </div>
      ${field(t("Brief"), textArea("draft-brief", dialog.brief, "", 14))}`,
    foot: foot(t("Create review"), "create-review"),
  });

const sendFeedback = (dialog) =>
  modal({
    title: t("Send feedback to builder"),
    hint: t("Trim the reviewer's output, then insert it. It is typed into the builder's terminal; nothing is submitted for you."),
    wide: true,
    body: field(t("Findings"), textArea("draft-feedback", dialog.feedback ?? "", "", 14)),
    foot: foot(t("Insert"), "insert-feedback"),
  });

const savedOutput = (dialog) =>
  modal({
    title: t("Saved output"),
    hint: t("{title} · bounded plain text, not a transcript", { title: dialog.title }),
    wide: true,
    body: `<pre class="preview__text preview__text--boxed">${escape(dialog.text)}</pre>`,
    foot: button({ label: t("Close"), data: { dismiss: "1" } }),
  });

const usage = (dialog) =>
  modal({
    title: t("Usage limits"),
    hint: dialog.loading ? t("Reading…") : "",
    body: dialog.loading
      ? `<p class="modal__hint">${escape(t("Asking the provider…"))}</p>`
      : dialog.windows?.length
        ? `<div class="group">${dialog.windows
            .map((window) =>
              setting(window.name, "", `<strong>${Math.round(window.percent)}%</strong>`),
            )
            .join("")}</div>`
        : `<p class="modal__hint">${escape(dialog.note ?? t("No quota reported."))}</p>`,
    foot: button({ label: t("Close"), data: { dismiss: "1" } }),
  });

// ---------------------------------------------------------------- projects --

const docSpec = (dialog) =>
  modal({
    title: t("New specification"),
    hint: t("In {project}. Starts from a template; draft it with AI afterwards, or write it yourself.", {
      project: project()?.title ?? "",
    }),
    body: field(
      t("Title"),
      textInput("draft-title", dialog.title, t("Feature or problem, e.g. Checkout retries")),
      t("Saved as .specdesk/specs/<slug>.md in the repository."),
    ),
    foot: foot(t("Create spec"), "doc-create-spec"),
  });

const removeProject = (dialog) =>
  modal({
    title: t("Remove project and its saved sessions?"),
    body: `<p class="modal__hint">${escape(t("Files, worktrees and provider conversations remain on disk."))}</p>
           ${dialog.children ? `<p class="modal__hint">${escape(t("The projects inside it stay, one level up."))}</p>` : ""}
           <p class="modal__hint"><code>${escape(dialog.path)}</code></p>`,
    foot: foot(t("Remove"), "confirm-remove-project", "danger-solid"),
  });

/// Every project at once. Nothing on disk goes, so the folders can be added
/// again; a running agent has to be stopped first.
const removeAllProjects = (dialog) =>
  modal({
    title: t("Remove all projects?"),
    body: `
      <p class="modal__hint">${escape(
        t("{projects} and {sessions} leave Convoy, with their specs and tasks.", {
          projects: plural(dialog.projects, "project"),
          sessions: plural(dialog.sessions, "session"),
        }),
      )}</p>
      <p class="modal__hint">${escape(t("Files, git repositories, worktrees and the agents' own conversations stay on disk. Add the folders again at any time."))}</p>
      ${
        dialog.running
          ? `<p class="modal__hint modal__hint--warn">${escape(t("Stop the running sessions first ({count} running).", { count: dialog.running }))}</p>`
          : ""
      }`,
    foot: `
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${button({ label: t("Remove all"), action: "confirm-remove-all-projects", kind: "danger-solid", disabled: dialog.running > 0 })}`,
  });

// --------------------------------------------------------------- worktrees --

const createWorktree = (dialog) =>
  modal({
    title: t("Create worktree"),
    hint: t("A separate checkout on a new branch. The project's own checkout stays where it is."),
    body: field(t("New branch"), textInput("draft-branch", dialog.branch)),
    foot: foot(t("Create"), "confirm-worktree"),
  });

const worktreeSetup = (dialog) =>
  modal({
    title: t("Run this project's worktree setup?"),
    body: `
      <p class="modal__hint">${escape(t("Copies: {files}", { files: dialog.shared.join(", ") || t("none") }))}</p>
      <p class="modal__hint">${escape(t("Command: {command}", { command: dialog.command || t("none") }))}</p>
      <p class="modal__hint"><code>${escape(dialog.directory)}</code></p>`,
    foot: `
      ${button({ label: t("Skip setup"), data: { dismiss: "1" } })}
      ${button({ label: t("Run setup"), action: "confirm-setup", kind: "primary" })}`,
  });

const removeWorktree = (dialog) =>
  modal({
    title: t("Remove this worktree?"),
    body: `<p class="modal__hint">
      ${t("Git removes {directory} only if it is clean. The branch is kept. {sessions} will be archived.", {
        directory: `<code>${escape(dialog.directory)}</code>`,
        sessions: escape(plural(dialog.linked, "linked session")),
      })}
    </p>`,
    foot: foot(t("Remove worktree"), "confirm-remove-worktree", "danger-solid"),
  });

// ---------------------------------------------------------------- accounts --

const accounts = (dialog) =>
  modal({
    title: t("Accounts"),
    hint: t("Sign in through the provider's own interface inside the terminal."),
    body: `
      ${group(
        t("Profiles"),
        (state.profiles ?? []).length
          ? state.profiles
              .map((profile) =>
                setting(
                  profile.label,
                  `${profile.agent === "claude" ? "Claude Code" : "Codex"} · ${
                    profile.sessions === 0
                      ? t("not in use")
                      : plural(profile.sessions, "session")
                  }`,
                  button({
                    icon: "trash",
                    kind: "quiet",
                    data: { "remove-profile": profile.id },
                    disabled: profile.sessions > 0,
                    title: profile.sessions ? t("Bound to saved sessions") : t("Remove this record"),
                  }),
                ),
              )
              .join("")
          : `<p class="modal__hint">${escape(t("No profiles yet. Without one, a session uses the provider's default home."))}</p>`,
      )}
      ${group(
        t("New profile"),
        `
        ${field(t("Label"), textInput("draft-label", dialog.label))}
        ${field(t("Provider"), choice("agent", AGENTS, dialog.agent))}`,
      )}`,
    note: escape(t("Removing a profile removes only Convoy's record of it. Credential files stay where they are.")),
    foot: `
      ${button({ label: t("Close"), data: { dismiss: "1" } })}
      ${button({ label: t("Add profile"), action: "add-profile", kind: "primary" })}`,
  });

const transcripts = (dialog) =>
  modal({
    title: t("Import provider history"),
    hint: t("Conversations the provider already saved for this folder."),
    wide: true,
    body: `
      ${field(t("Provider"), choice("agent", AGENTS, dialog.agent))}
      ${field(
        t("Account"),
        choice(
          "profile",
          [["", t("Default home")], ...(state.profiles ?? []).map((p) => [p.id, p.label])],
          dialog.profile ?? "",
        ),
      )}
      ${group(
        t("Conversations"),
        dialog.scanning
          ? `<p class="modal__hint">${escape(t("Looking…"))}</p>`
          : dialog.found === null
            ? `<p class="modal__hint">${escape(t("Press Scan to look for conversations in this folder."))}</p>`
            : dialog.found.length
              ? dialog.found
                  .map((item) =>
                    setting(
                      item.title,
                      item.provider_id,
                      button({ label: t("Import"), data: { import: item.provider_id, title: item.title } }),
                    ),
                  )
                  .join("")
              : `<p class="modal__hint">${escape(t("Nothing found. Conversations are matched by the folder the CLI ran in."))}</p>`,
      )}`,
    foot: `
      ${button({ label: t("Close"), data: { dismiss: "1" } })}
      ${button({ label: t("Scan"), action: "scan-transcripts", kind: "primary" })}`,
  });

const quickCommands = (dialog) =>
  modal({
    title: t("Quick commands"),
    hint: t("Saved text, sent into a running agent on demand."),
    wide: true,
    body: `
      ${group(
        t("Saved"),
        state.quickCommands.length
          ? state.quickCommands
              .map((command) =>
                setting(
                  command.title,
                  `${command.text.slice(0, 80)}${command.submit ? ` · ${t("sends Enter")}` : ""}`,
                  button({ icon: "edit", kind: "quiet", data: { "command-edit": command.id }, title: t("Edit") }) +
                    button({
                      icon: "trash",
                      kind: "quiet",
                      data: { "remove-command": command.id },
                      title: t("Remove"),
                    }),
                ),
              )
              .join("")
          : `<p class="modal__hint">${escape(t("Nothing saved yet."))}</p>`,
      )}
      ${group(
        dialog.editing ? t("Edit command") : t("New command"),
        `
        ${field(t("Name"), textInput("draft-title", dialog.title))}
        ${field(t("Text"), textArea("draft-text", dialog.text, "", 4))}
        ${setting(t("Send Enter"), t("Off by default: the text is typed, not submitted."), toggle("submit", dialog.submit, t("Send Enter")))}
        ${setting(t("Only this project"), "", toggle("scoped", dialog.scoped, t("Only this project")))}`,
      )}`,
    foot: `
      ${button({ label: t("Close"), data: { dismiss: "1" } })}
      ${button({ label: dialog.editing ? t("Save command") : t("Add command"), action: "add-command", kind: "primary" })}`,
  });

// ---------------------------------------------------------------- planning --

/// The spec's own work, as the macOS app's WorkTasks: each task with its
/// builder and reviewer sessions, its status, and the review brief.
function specWork(dialog) {
  const tasks = (state.planning?.tasks ?? []).filter((task) => task.spec_id === dialog.id);
  const sessions = state.allSessions.filter((item) => item.project_id === state.projectId);
  const title = (id) => sessions.find((item) => item.id === id)?.title ?? t("session");
  const rows = tasks
    .map(
      (task) => `
      <div class="spec-work">
        <div class="spec-work__head">
          <strong>${escape(task.title)}</strong>
          <span class="badge badge--muted">${escape(t(task.status))}</span>
        </div>
        <div class="spec-work__actions">
          ${task.session_id ? button({ label: t("Builder: {session}", { session: title(task.session_id) }), icon: "terminal", data: { open: task.session_id } }) : ""}
          ${task.review_session_id ? button({ label: t("Reviewer: {session}", { session: title(task.review_session_id) }), icon: "review", data: { open: task.review_session_id } }) : ""}
          ${task.session_id ? button({ label: t("Copy review brief"), icon: "copy", data: { "spec-brief": task.session_id } }) : ""}
          <select class="field__select spec-work__link" data-link-task="${escape(task.id)}" title="${escape(t("Link an existing session as the builder"))}">
            <option value="">${escape(t("Link existing session…"))}</option>
            ${sessions
              .filter((item) => item.id !== task.session_id)
              .map((item) => `<option value="${escape(item.id)}">${escape(item.title)}</option>`)
              .join("")}
          </select>
        </div>
        ${task.findings ? `<p class="spec-work__findings">${escape(task.findings)}</p>` : ""}
      </div>`,
    )
    .join("");
  return group(t("Work"), rows || `<p class="modal__hint">${escape(t("No tasks for this specification yet."))}</p>`);
}

const specForm = (dialog) =>
  modal({
    title: dialog.id ? t("Specification") : t("New specification"),
    hint: dialog.id
      ? t("Revision {revision} · {state}", { revision: dialog.revision, state: dialog.approved ? t("approved") : t("draft") })
      : "",
    wide: true,
    body: `
      ${field(t("Title"), textInput("spec-title", dialog.title))}
      ${field(t("Problem"), textArea("spec-problem", dialog.problem, "", 3))}
      ${field(t("Requirements"), textArea("spec-requirements", dialog.requirements, "", 4))}
      ${field(t("Acceptance"), textArea("spec-acceptance", dialog.acceptance, "", 3))}
      ${field(t("Constraints"), textArea("spec-constraints", dialog.constraints, "", 3))}
      ${field(t("Plan"), textArea("spec-plan", dialog.plan, "", 4))}
      ${dialog.id ? specWork(dialog) : ""}`,
    foot: `
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${dialog.id ? button({ label: t("New task for this spec"), icon: "plus", action: "spec-new-task" }) : ""}
      ${dialog.id && !dialog.approved ? button({ label: t("Approve revision"), action: "approve-spec" }) : ""}
      ${button({ label: t("Save"), action: "save-spec", kind: "primary" })}`,
  });

/// Which tasks of the project this one waits for. The queue passes it by
/// until they are done; a circle is refused when saving.
function blockedBy(dialog) {
  const others = (state.planning?.tasks ?? []).filter(
    (task) => task.id !== dialog.id && (task.project_id ?? state.projectId) === (dialog.project_id ?? state.projectId),
  );
  if (!others.length) return "";
  const chosen = new Set(dialog.depends_on ?? []);
  // A div, not field()'s label: each task is a label of its own, and labels
  // do not nest.
  return `
    <div class="field">
      <span class="field__label">${escape(t("Blocked by"))}</span>
      <div class="check-list">${others
        .map(
          (task) => `
        <label class="check">
          <input type="checkbox" data-depends-on="${escape(task.id)}" ${chosen.has(task.id) ? "checked" : ""} />
          <span class="check__text">${escape(task.title)}</span>
          ${task.status === "done" ? `<span class="badge badge--muted">${escape(t("done"))}</span>` : ""}
        </label>`,
        )
        .join("")}</div>
      <span class="field__note">${escape(t("The queue starts this task only after these are done."))}</span>
    </div>`;
}

/// Tasks of any project this one's agent reads before starting: each as a
/// chip, and a picker for more, this project's tasks first.
function relatedTasks(dialog) {
  const pool = new Map();
  for (const task of [...(state.allTasks ?? []), ...(state.planning?.tasks ?? [])]) pool.set(task.id, task);
  pool.delete(dialog.id);
  if (!pool.size) return "";
  const here = dialog.project_id ?? state.projectId;
  const chosen = (dialog.related ?? []).filter((id) => pool.has(id));
  const projectName = (id) => state.projects.find((item) => item.id === id)?.title ?? "";
  const tone = (status) => TASK_STATUSES.find(([key]) => key === status)?.[2] ?? "idle";
  const chips = chosen
    .map((id) => {
      const task = pool.get(id);
      const elsewhere = task.project_id !== here ? projectName(task.project_id) : "";
      return `
        <span class="related-chip" title="${escape(`${task.title} · ${t(task.status)}${elsewhere ? ` · ${elsewhere}` : ""}`)}">
          <span class="state__dot state__dot--${tone(task.status)}"></span>
          ${task.source?.key ? `<code class="related-chip__key">${escape(task.source.key)}</code>` : ""}
          <span class="related-chip__title">${escape(task.title)}</span>
          ${elsewhere ? `<span class="related-chip__project">${escape(elsewhere)}</span>` : ""}
          <button class="related-chip__remove" data-related-remove="${escape(id)}" title="${escape(t("Remove"))}">${icons.close}</button>
        </span>`;
    })
    .join("");
  const groups = new Map();
  for (const task of pool.values()) {
    if (chosen.includes(task.id)) continue;
    const key = task.project_id ?? here;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(task);
  }
  const order = [...groups.keys()].sort((a, b) => (a === here ? -1 : b === here ? 1 : projectName(a).localeCompare(projectName(b))));
  const options = order
    .map(
      (key) => `<optgroup label="${escape(projectName(key) || t("This project"))}">${groups
        .get(key)
        .map(
          (task) =>
            `<option value="${escape(task.id)}">${escape(`${task.source?.key ? `${task.source.key} · ` : ""}${task.title}`)}</option>`,
        )
        .join("")}</optgroup>`,
    )
    .join("");
  return `
    <div class="field">
      <span class="field__label">${escape(t("Related tasks"))}</span>
      ${chips ? `<div class="related-chips">${chips}</div>` : ""}
      ${options ? `<select class="related-add" data-related-add="1"><option value="">${escape(t("Add a related task…"))}</option>${options}</select>` : ""}
      <span class="field__note">${escape(t("Its agent reads these first: their details, branch and pull request."))}</span>
    </div>`;
}

const taskForm = (dialog) => {
  const specs = state.planning?.specs ?? [];
  const projectId = dialog.project_id ?? state.projectId;
  // A new task starts with the automatic skills.
  if (!Array.isArray(dialog.skills)) dialog.skills = autoSkills(dialog.agent, projectId);
  return modal({
    title: dialog.id ? t("Task") : t("New task"),
    hint: dialog.id ? `${t(dialog.status)}${dialog.last_error ? ` · ${dialog.last_error}` : ""}` : "",
    wide: true,
    body: `
      ${syncNotice(dialog)}
      ${planPanel(dialog)}
      ${reviewPanel(dialog)}
      ${qaPanel(dialog)}
      ${docsPanel(dialog)}
      ${field(t("Title"), textInput("task-title", dialog.title))}
      ${field(t("Agent"), choice("agent", AGENTS, dialog.agent))}
      ${skillPicker(dialog.skills, dialog.agent, projectId)}
      ${
        // A list, not a row of buttons: a project may have a hundred specs.
        specs.length
          ? field(
              t("Specification"),
              select(
                "task-spec",
                [["", t("None")], ...specs.map((spec) => [spec.id, `${spec.title}${spec.approved ? "" : ` · ${t("draft")}`}`])],
                dialog.spec_id ?? "",
                !!dialog.id,
              ),
              dialog.id ? t("Fixed once the task exists.") : "",
            )
          : ""
      }
      ${field(
        t("Publishing"),
        choice(
          "mode",
          [["none", t("Do not publish")], ["pr", t("Pull request")], ["push", t("Push")]],
          dialog.mode,
        ),
      )}
      ${setting(t("Review each round with the other agent"), t("Findings go back to the builder by themselves, a few rounds at most."), toggle("auto_review", dialog.auto_review, t("Automatic review")))}
      ${setting(t("Plan first"), t("A planner writes the plan; the task is built once it is approved."), toggle("plan_first", dialog.plan_first, t("Plan first")))}
      ${
        dialog.plan_first
          ? field(
              t("Plan approval"),
              select(
                "task-plan-auto",
                [
                  ["", t("As the project says")],
                  ["yes", t("Approve by itself")],
                  ["no", t("Wait for me")],
                ],
                dialog.plan_auto_approve === true ? "yes" : dialog.plan_auto_approve === false ? "no" : "",
              ),
            )
          : ""
      }
      ${blockedBy(dialog)}
      ${relatedTasks(dialog)}
      ${field(t("Details"), textArea("task-details", dialog.details, "", 4))}
      ${field(t("Findings"), textArea("task-findings", dialog.findings, "", 3))}
      <button class="sheet-disclosure" data-toggle="advanced" aria-expanded="${!!dialog.advanced}">
        ${dialog.advanced ? "▾" : "▸"} ${escape(t("Advanced"))}
      </button>
      ${
        dialog.advanced
          ? `<div class="sheet-box"><div class="field-row">
               ${field(t("Model"), modelSelect("task-model", dialog.agent, dialog.profile_id ?? "", dialog.model ?? ""))}
               ${field(t("Effort"), effortSelect("task-effort", dialog.agent, dialog.profile_id ?? "", dialog.model ?? "", dialog.effort ?? ""))}
             </div>
             ${field(t("Account"), select("task-account", accountOptions(dialog.agent), dialog.profile_id ?? ""))}
             </div>`
          : ""
      }`,
    foot: `
      ${dialog.id ? button({ icon: "trash", kind: "quiet", title: t("Delete task"), action: "delete-task", disabled: dialog.status === "building" }) : ""}
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${!dialog.id || ["queued", "failed", "changes", "attention"].includes(dialog.status) ? button({ label: dialog.plan_first && dialog.plan?.status !== "approved" ? t("Plan now") : t("Run now"), icon: "play", action: "run-task-now" }) : ""}
      ${button({ label: t("Save"), action: "save-task", kind: "primary" })}`,
  });
};

const PLAN_HEADLINES = {
  drafting: ["The planner is writing the plan…", "running"],
  waiting: ["The plan waits for your approval", "review"],
  approved: ["Plan approved", "done"],
  changes_requested: ["The task changed: the plan will be redone", "open"],
};

/// The plan: what the planner wrote, and approving it or asking for
/// changes.
function planPanel(task) {
  const plan = task.plan;
  if (!plan) return "";
  const [headline, tone] = PLAN_HEADLINES[plan.status] ?? [plan.status, "idle"];
  const waiting = plan.status === "waiting";
  const comments = (plan.comments ?? [])
    .map((comment) => `<li>${escape(comment.text)}</li>`)
    .join("");
  return `
    <section class="qa-panel qa-panel--${tone}" aria-live="polite">
      <div class="qa-panel__head">
        <span class="state__dot state__dot--${tone}"></span>
        <span class="qa-panel__title">${escape(t(headline))}</span>
      </div>
      ${plan.text ? `<div class="plan-text docs__render">${markdown(plan.text)}</div>` : ""}
      ${comments ? `<details><summary>${escape(t("Your comments"))}</summary><ul class="plain-list">${comments}</ul></details>` : ""}
      ${
        waiting
          ? `<textarea id="plan-comment" class="field__input plan-comment" rows="2" placeholder="${escape(t("What should change in the plan?"))}">${escape(task.plan_comment ?? "")}</textarea>`
          : ""
      }
      <div class="qa-panel__actions">
        ${waiting ? button({ label: t("Approve and build"), icon: "check", action: "plan-approve", kind: "primary" }) : ""}
        ${waiting ? button({ label: t("Ask for changes"), icon: "edit", action: "plan-changes" }) : ""}
        ${plan.session_id ? button({ label: t("Open planner"), icon: "terminal", action: "plan-open" }) : ""}
      </div>
    </section>`;
}

const REVIEW_HEADLINES = {
  reviewing: ["The other agent is reviewing…", "running"],
  passed: ["Review passed", "done"],
  failed: ["Review found problems", "failed"],
  no_verdict: ["The reviewer ended without a verdict", "idle"],
  skipped: ["Review skipped", "idle"],
};

/// The latest review, which round it was, and going on without it.
function reviewPanel(task) {
  const run = task.review;
  if (!run && !task.round) return "";
  const [headline, tone] = run ? REVIEW_HEADLINES[run.status] ?? [run.status, "idle"] : ["", "idle"];
  const round = task.round
    ? `<span class="badge badge--muted">${escape(t("Fix round {round} of {max}", { round: task.round, max: task.max_rounds }))}</span>`
    : "";
  const canSkip = run && ["reviewing", "no_verdict", "failed"].includes(run.status);
  return `
    <section class="qa-panel qa-panel--${tone}" aria-live="polite">
      <div class="qa-panel__head">
        <span class="state__dot state__dot--${tone}"></span>
        <span class="qa-panel__title">${escape(headline ? t(headline) : t("Fixing"))}</span>
        ${round}
      </div>
      ${run?.findings ? `<div class="qa-panel__findings">${escape(run.findings)}</div>` : ""}
      ${task.build_summary ? `<details><summary>${escape(t("What the builder reports"))}</summary><div class="qa-panel__findings">${escape(task.build_summary)}</div></details>` : ""}
      <div class="qa-panel__actions">
        ${task.review_session_id ? button({ label: t("Open reviewer"), icon: "terminal", action: "review-open" }) : ""}
        ${canSkip ? button({ label: t("Skip review"), icon: "chevronRight", action: "review-skip", kind: "quiet" }) : ""}
      </div>
    </section>`;
}

/// The documents written for the task, and writing one now.
function docsPanel(task) {
  if (!task.id) return "";
  const docs = task.docs ?? [];
  const writing = task.docs_run?.status === "writing";
  const ready = ["testing", "review", "pr", "done", "attention"].includes(task.status);
  if (!docs.length && !writing && !ready) return "";
  const list = docs
    .map(
      (doc) => `
        <li class="qa-check">
          <div class="qa-check__line">
            <span class="qa-check__mark">${doc.kind === "pdf" ? "PDF" : "MD"}</span>
            <code class="qa-check__command">${escape(doc.title)}</code>
            ${button({ icon: "open", kind: "quiet", title: t("Open in the system viewer"), data: { "task-doc-open": doc.path } })}
            ${button({ icon: "folder", kind: "quiet", title: t("Show in Finder"), data: { "task-doc-reveal": doc.path } })}
          </div>
        </li>`,
    )
    .join("");
  return `
    <section class="qa-panel qa-panel--${writing ? "running" : docs.length ? "done" : "idle"}">
      <div class="qa-panel__head">
        <span class="qa-panel__title">${escape(writing ? t("The document is being written…") : docs.length ? t("Task documents") : t("No document yet"))}</span>
      </div>
      ${list ? `<ul class="qa-panel__checks">${list}</ul>` : ""}
      <div class="qa-panel__actions">
        ${writing ? "" : button({ label: docs.length ? t("Write it again") : t("Write the document"), icon: "doc", action: "task-docs-write", kind: "quiet" })}
      </div>
    </section>`;
}

const QA_HEADLINES = {
  checking: ["Running the project's checks…", "running"],
  waiting: ["Checks passed. QA waits for the review.", "idle"],
  testing: ["Checks passed. A QA agent is testing.", "running"],
  passed: ["QA passed", "done"],
  failed: ["QA failed", "failed"],
  no_verdict: ["The QA agent ended without a verdict", "idle"],
};

/// The task's latest QA run: each check with its output, what the QA agent
/// found, and what to do next.
function qaPanel(task) {
  const run = task.qa;
  if (!run) return "";
  const [headline, tone] = QA_HEADLINES[run.status] ?? [run.status, "idle"];
  const busy = run.status === "checking" || run.status === "testing";
  const checks = (run.checks ?? [])
    .map(
      (check) => `
        <li class="qa-check${check.ok ? "" : " qa-check--failed"}">
          <div class="qa-check__line">
            <span class="qa-check__mark" aria-label="${escape(check.ok ? t("Passed") : t("Failed"))}">${check.ok ? "✓" : "✗"}</span>
            <code class="qa-check__command">${escape(check.command)}</code>
            <span class="qa-check__time">${escape(t("{seconds} s", { seconds: check.seconds }))}</span>
          </div>
          ${check.output ? `<details${check.ok ? "" : " open"}><summary>${escape(t("Output"))}</summary><pre class="qa-check__output">${escape(check.output)}</pre></details>` : ""}
        </li>`,
    )
    .join("");
  const actions = [
    run.status === "passed" && task.status !== "done" ? button({ label: t("Mark done"), icon: "check", action: "task-mark-done", kind: "primary" }) : "",
    run.session_id ? button({ label: t("Open QA session"), icon: "terminal", action: "task-qa-open" }) : "",
    busy ? "" : button({ label: t("Run QA again"), icon: "refresh", action: "task-qa-run", kind: "quiet" }),
  ].join("");
  return `
    <section class="qa-panel qa-panel--${tone}" aria-live="polite">
      <div class="qa-panel__head">
        <span class="state__dot state__dot--${tone}"></span>
        <span class="qa-panel__title">${escape(t(headline))}</span>
      </div>
      ${checks ? `<ul class="qa-panel__checks">${checks}</ul>` : ""}
      ${run.findings ? `<div class="qa-panel__findings">${escape(run.findings)}</div>` : ""}
      ${actions ? `<div class="qa-panel__actions">${actions}</div>` : ""}
    </section>`;
}

/// How the task's tracker heard of it last, and Retry when it did not.
function syncNotice(task) {
  const kind = syncState(task);
  if (!kind) return "";
  return `<div class="sync-notice sync-notice--${kind}">
      <span class="sync-notice__text">${escape(syncSummary(task))}</span>
      ${kind === "failed" ? button({ label: t("Retry"), icon: "refresh", action: "task-sync-retry", kind: "quiet" }) : ""}
    </div>`;
}

const queueConfirm = (dialog) =>
  modal({
    title: t("Run {tasks}?", { tasks: plural(dialog.summary.length, "queued task") }),
    body: `
      <ul class="plain-list">
        ${dialog.summary.map((line) => `<li>${escape(line)}</li>`).join("")}
      </ul>
      <p class="modal__hint">
        ${escape(t("Runs the installed agents. Publishing happens only for tasks configured for it. A failure pauses the queue."))}
      </p>`,
    foot: foot(t("Run queue"), "confirm-queue"),
  });

// ------------------------------------------------------------------- misc --

/// A new branch, from where HEAD is or from any branch, remote branch or tag.
const newBranch = (dialog) =>
  modal({
    title: t("New branch"),
    body: `
      ${field(t("Branch name"), textInput("draft-branch", dialog.branch))}
      ${field(
        t("Start from"),
        `<input id="draft-base" list="branch-bases" value="${escape(dialog.base ?? "")}" placeholder="${escape(t("Current branch"))}" spellcheck="false" />
         <datalist id="branch-bases">${(state.files?.snapshot?.branches ?? []).map((name) => `<option value="${escape(name)}"></option>`).join("")}</datalist>`,
        t("Empty starts from where you are now."),
      )}`,
    foot: foot(t("Create and switch"), "confirm-branch"),
  });

const hunkChoice = (dialog) =>
  modal({
    title: t("Discard which hunk?"),
    hint: dialog.path,
    body: choice(
      "hunk",
      dialog.hunks.map((label, index) => [String(index), `${index + 1}. ${label}`]),
      String(dialog.hunk ?? 0),
    ),
    foot: foot(t("Discard hunk"), "confirm-hunk", "danger-solid"),
  });

const confirmAction = (dialog) =>
  modal({
    title: dialog.title,
    body: `<p class="modal__hint">${escape(dialog.body)}</p>`,
    foot: foot(dialog.confirmLabel, "confirm-generic", dialog.danger ? "danger-solid" : "primary"),
  });

const palette = (dialog) => `
  <div class="scrim" data-dismiss="1">
    <div class="modal modal--palette" role="dialog" aria-modal="true" aria-label="${escape(t("Commands"))}">
      <div class="palette__search">
        <input id="palette-input" type="search"
               placeholder="${escape(
                 dialog.mode === "terminals"
                   ? t("Switch to an open terminal…")
                   : dialog.mode === "quick"
                     ? t("Send a quick command to this terminal…")
                     : dialog.mode === "search"
                       ? t("Search sessions and projects…")
                       : t("Jump to a session or project, or run a command…"),
               )}"
               value="${escape(dialog.query)}" spellcheck="false" />
      </div>
      <div class="palette__list">
        ${
          dialog.results.length
            ? dialog.results
                .map(
                  (entry, index) => `
          <button class="palette__row${index === dialog.index ? " palette__row--on" : ""}"
                  data-palette="${index}">
            <span class="palette__text">
              <span class="palette__title">${entry.needsYou ? '<span class="dot dot--waiting"></span>' : entry.running ? '<span class="dot dot--running"></span>' : ""}${escape(entry.title)}</span>
              ${entry.subtitle ? `<span class="palette__subtitle">${escape(entry.subtitle)}</span>` : ""}
            </span>
            <span class="palette__kind">${escape(entry.shortcut || t(entry.kind))}</span>
          </button>`,
                )
                .join("")
            : `<div class="menu__empty">${escape(
                dialog.mode === "terminals" && !dialog.query
                  ? t("No open terminals. Pick a session in the sidebar, or start a new one.")
                  : dialog.mode === "quick" && !dialog.query
                    ? t("No quick commands yet — add them in Settings → Quick Commands.")
                    : t("Nothing matches."),
              )}</div>`
        }
      </div>
    </div>
  </div>`;

const markdownPreview = (dialog) =>
  modal({
    title: dialog.title,
    wide: true,
    body: `<div class="preview__markdown">${markdown(dialog.text)}</div>`,
    foot: `
      ${button({ label: t("Close"), data: { dismiss: "1" } })}
      ${button({ label: t("Copy"), action: "copy-markdown", kind: "primary" })}`,
  });

/// Which session a pane shows: any session of any project, minus those
/// already on screen. A split is terminals side by side, not windows.
function splitPicker(dialog) {
  const pane = dialog.pane ?? state.focus;
  const shown = new Set(state.panes.filter((id, at) => id && at !== pane && at < state.layout));
  const others = state.allSessions.filter((item) => !shown.has(item.id));
  const name = (item) => state.projects.find((entry) => entry.id === item.project_id)?.title ?? "";
  return modal({
    title: t("Session for pane {number}", { number: pane + 1 }),
    hint: t("Any session, from any project. It keeps running when the pane is closed."),
    body: others.length
      ? `<div class="plain-list plain-list--rows">
           ${others
             .map(
               (item) => `
             <button class="menu__item" data-split="${escape(item.id)}">
               <span>${escape(item.title)}</span>
               <span class="menu__hint">${escape(name(item))} · ${escape(item.running ? t("running") : item.started ? t("stopped") : t("never started"))}</span>
             </button>`,
             )
             .join("")}
         </div>`
      : `<p class="modal__hint">${escape(t("There is no other session."))}</p>`,
    foot: button({ label: t("Cancel"), data: { dismiss: "1" } }),
  });
}

/// Closing the window ends every agent with it.
const quitting = (dialog) =>
  modal({
    title: dialog.running === 1 ? t("An agent is still running") : t("Agents are still running"),
    body: `<p class="modal__hint">
      ${escape(
        dialog.running === 1
          ? t("Closing Convoy stops it. Anything not written to disk by then is gone.")
          : t("Closing Convoy stops all {count}. Anything not written to disk by then is gone.", { count: dialog.running }),
      )}
    </p>`,
    foot: `
      ${button({ label: t("Stay open"), data: { dismiss: "1" } })}
      ${button({ label: t("Stop and quit"), action: "quit", kind: "danger-solid" })}`,
  });

/// The macOS app's workspace, counted before anything is written.
function macosImport(dialog) {
  const report = dialog.report;
  const counts = [
    [report.projects, "project"],
    [report.sessions, "session"],
    [report.tasks, "task"],
    [report.specs, "spec"],
    [report.quick_commands, "quick command"],
    [report.connections, "integration"],
    [report.activity, "activity event", "activity events"],
  ]
    .filter(([count]) => count > 0)
    .map(([count, one, many]) => `<li>${escape(plural(count, one, many))}</li>`)
    .join("");
  const existing = report.projects_existing
    ? `<p class="modal__hint">${escape(
        report.projects_existing === 1
          ? t("1 project is already here; it keeps its settings and gains the sessions.")
          : t("{projects} are already here; they keep their settings and gain the sessions.", {
              projects: plural(report.projects_existing, "project"),
            }),
      )}</p>`
    : "";
  const warnings = report.warnings.map((warning) => `<li>${escape(warning)}</li>`).join("");
  return modal({
    title: t("Import from the macOS app"),
    hint: t("Nothing in the macOS app changes. Sessions resume the same conversations."),
    body: `
      ${counts ? `<ul class="import-counts">${counts}</ul>` : `<p class="modal__hint">${escape(t("Everything the macOS app has is already here."))}</p>`}
      ${existing}
      ${setting(
        t("Settings and shortcuts"),
        t("Theme, notifications, awake mode, AI limits and your shortcuts. Replaces the ones set here."),
        toggle("settings", dialog.settings, t("Import settings and shortcuts")),
      )}
      ${warnings ? `<ul class="import-warnings">${warnings}</ul>` : ""}`,
    foot: `
      ${button({ label: dialog.offer ? t("Not now") : t("Cancel"), data: { dismiss: "1" } })}
      ${button({ label: dialog.busy ? t("Importing…") : t("Import"), action: "macos-import-run", kind: "primary", disabled: dialog.busy })}`,
  });
}

const macosImportDone = (dialog) =>
  modal({
    title: t("Imported, with notes"),
    body: `<ul class="import-warnings">${dialog.warnings.map((warning) => `<li>${escape(warning)}</li>`).join("")}</ul>`,
    foot: button({ label: t("Done"), kind: "primary", data: { dismiss: "1" } }),
  });

/// An account's login, in its own terminal.
const login = (dialog) =>
  modal({
    title: t("Log in: {label}", { label: dialog.label }),
    hint: t("{agent} runs its own login here. Nothing you type is kept by Convoy.", {
      agent: dialog.agent === "claude" ? "Claude Code" : "Codex",
    }),
    wide: true,
    body: '<div class="login-term" id="login-host"></div>',
    foot: button({ label: dialog.finished ? t("Done") : t("Close"), kind: dialog.finished ? "primary" : "", data: { dismiss: "1" } }),
  });

// ------------------------------------------------------ import all history --

const AGENT_NAMES = { claude: "Claude Code", codex: "Codex" };

/// The project a row will be imported into: the one picked, else the match.
export const sweepProject = (dialog, row) => dialog.picks[row.provider_id] || row.project_id || "";

/// What "Import" sends: every checked row that has a project.
export const sweepChoices = (dialog) =>
  (dialog.rows ?? [])
    .filter((row) => dialog.checked[row.provider_id] && sweepProject(dialog, row))
    .map((row) => ({ provider_id: row.provider_id, project_id: sweepProject(dialog, row) }));

/// Whether a row matches the search: its title, folder, agent or project.
function sweepMatches(dialog, row, names) {
  const query = (dialog.query ?? "").trim().toLowerCase();
  if (!query) return true;
  const project = names.get(sweepProject(dialog, row)) ?? "";
  return [row.title, row.place, AGENT_NAMES[row.agent] ?? row.agent, project]
    .some((text) => (text ?? "").toLowerCase().includes(query));
}

/// The rows the search shows that can be checked, i.e. have a project.
export const sweepVisible = (dialog) =>
  sweepGroups(dialog).flatMap((item) => item.rows).filter((row) => sweepProject(dialog, row));

/// Rows under the project they were matched to, by name, with the
/// unassigned ones last. A row keeps its place when a project is picked.
/// Only the rows the search matches are listed.
export function sweepGroups(dialog) {
  const names = new Map(state.projects.map((item) => [item.id, item.title]));
  const groups = new Map();
  for (const row of dialog.rows ?? []) {
    if (!sweepMatches(dialog, row, names)) continue;
    const key = row.project_id && names.has(row.project_id) ? row.project_id : "";
    if (!groups.has(key)) groups.set(key, { key, name: key ? names.get(key) : t("Unassigned"), rows: [] });
    groups.get(key).rows.push(row);
  }
  const named = [...groups.values()].filter((item) => item.key).sort((a, b) => a.name.localeCompare(b.name));
  return groups.has("") ? [...named, groups.get("")] : named;
}

function sweepPicker(dialog, row) {
  const projects = [...state.projects].sort((a, b) => a.title.localeCompare(b.title));
  const suggested = row.suggestions.map((id) => projects.find((item) => item.id === id)).filter(Boolean);
  const picked = dialog.picks[row.provider_id] ?? "";
  const option = (item) =>
    `<option value="${escape(item.id)}"${item.id === picked ? " selected" : ""}>${escape(item.title)}</option>`;
  return `<div class="import__pick sweep__pick${picked ? " import__pick--custom" : ""}">
    <select data-sweep-pick="${escape(row.provider_id)}" aria-label="${escape(t("Choose a project…"))}">
      <option value="">${escape(t("Choose a project…"))}</option>
      ${suggested.length ? `<optgroup label="${escape(t("Suggested"))}">${suggested.map(option).join("")}</optgroup>` : ""}
      <optgroup label="${escape(t("All projects"))}">${projects.map(option).join("")}</optgroup>
    </select>
  </div>`;
}

function sweepRow(dialog, row) {
  const project = sweepProject(dialog, row);
  const why =
    row.reason === "folder" ? t("started in this project") : row.reason === "paths" ? plural(row.votes, "file reference") : "";
  const title = row.title || t("Untitled conversation");
  return `
    <div class="import__row sweep__row">
      <input type="checkbox" data-sweep-check="${escape(row.provider_id)}" aria-label="${escape(title)}"
             ${dialog.checked[row.provider_id] && project ? " checked" : ""}${project ? "" : " disabled"} />
      <span class="sweep__agent" title="${escape(AGENT_NAMES[row.agent] ?? row.agent)}">${agentIcon(row.agent, 15)}</span>
      <div class="import__title">
        <div>${escape(title)}</div>
        <div class="import__meta">${escape([row.place || t("unknown folder"), why].filter(Boolean).join(" · "))}</div>
      </div>
      ${row.project_id ? "" : sweepPicker(dialog, row)}
      <span class="sweep__when">${escape(when(row.at))}</span>
    </div>`;
}

function historySweep(dialog) {
  const rows = dialog.rows;
  const count = rows ? sweepChoices(dialog).length : 0;
  let body;
  if (!rows) {
    body = `<div class="sweep__scanning" role="status"><span class="spinner" aria-hidden="true"></span>${escape(t("Scanning conversations…"))}</div>`;
  } else if (!rows.length) {
    body = `<p class="modal__hint">${escape(t("Nothing to import — every conversation on this computer is already in Convoy."))}</p>`;
  } else {
    const groups = sweepGroups(dialog);
    const visible = sweepVisible(dialog);
    const allChecked = visible.length && visible.every((row) => dialog.checked[row.provider_id]);
    // Checked rows the search hides still import, so they are named.
    const shown = new Set(visible.map((row) => row.provider_id));
    const hidden = sweepChoices(dialog).filter((choice) => !shown.has(choice.provider_id)).length;
    const toolbar = `
      <div class="sweep__toolbar">
        <input id="sweep-filter" class="section__filter sweep__filter" type="search"
               placeholder="${escape(t("Search conversations"))}" aria-label="${escape(t("Search conversations"))}"
               value="${escape(dialog.query ?? "")}" spellcheck="false" />
        ${button({ label: t("Select all"), kind: "quiet", disabled: !visible.length || allChecked, data: { "sweep-all": "all" } })}
        ${button({ label: t("Deselect all"), kind: "quiet", disabled: !count, data: { "sweep-all": "none" } })}
      </div>
      ${hidden ? `<p class="sweep__hidden">${escape(t("{count} selected outside this search will be imported too.", { count: hidden }))}</p>` : ""}`;
    const list = groups.length
      ? groups
      .map((item) => {
        const pickable = item.rows.filter((row) => sweepProject(dialog, row));
        const all = pickable.length && pickable.every((row) => dialog.checked[row.provider_id]);
        return `
        <div class="sweep__group">
          <span>${escape(item.name)}</span>
          <span class="section__count">${item.rows.length}</span>
          ${
            pickable.length
              ? button({ label: all ? t("Select none") : t("Select all"), kind: "quiet", data: { "sweep-group": item.key || "-", value: all ? "none" : "all" } })
              : ""
          }
        </div>
        ${item.rows.map((row) => sweepRow(dialog, row)).join("")}`;
      })
      .join("")
      : `<p class="sweep__empty">${escape(t("No conversations match “{query}”", { query: dialog.query.trim() }))}</p>`;
    body = `${toolbar}<div class="import__list sweep__list">${list}</div>`;
  }
  return modal({
    title: t("Import all history"),
    hint: rows?.length
      ? t("{conversations} found, {selected} selected", { conversations: plural(rows.length, "conversation"), selected: count })
      : t("Claude Code and Codex conversations that are not in Convoy yet, matched to your projects."),
    wide: true,
    body,
    foot: `
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${button({
        label: dialog.busy ? t("Importing…") : t("Import {count}", { count }),
        action: "history-sweep-run",
        kind: "primary",
        disabled: dialog.busy || !count,
      })}`,
  });
}

const VIEWS = {
  login,
  "macos-import": macosImport,
  "macos-import-done": macosImportDone,
  "doc-spec": docSpec,
  split: splitPicker,
  quit: quitting,
  session: newSession,
  "edit-session": editSession,
  rename: renameSession,
  review: startReview,
  feedback: sendFeedback,
  output: savedOutput,
  usage,
  "remove-project": removeProject,
  "remove-all-projects": removeAllProjects,
  worktree: createWorktree,
  "worktree-setup": worktreeSetup,
  "remove-worktree": removeWorktree,
  accounts,
  transcripts,
  "history-sweep": historySweep,
  commands: quickCommands,
  spec: specForm,
  task: taskForm,
  queue: queueConfirm,
  branch: newBranch,
  hunk: hunkChoice,
  confirm: confirmAction,
  palette,
  markdown: markdownPreview,
  automation: automationEditor,
  skill: skillEditor,
};

export { session, project };
