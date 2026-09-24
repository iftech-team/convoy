// Every dialog. `state.dialog` holds the kind and its draft; this renders it.

import {
  button,
  choice,
  escape,
  field,
  group,
  modal,
  segmented,
  setting,
  textArea,
  textInput,
  toggle,
} from "./ui.js";
import { markdown } from "./views-files.js";
import { state, project, session } from "./state.js";
import { SHORTCUTS, label } from "./shortcuts.js";

const fallback = (action) =>
  SHORTCUTS.find(([name]) => name === action)?.[1] ?? "";

const AGENTS = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
];

export function dialogView() {
  const dialog = state.dialog;
  if (!dialog) return "";
  const render = VIEWS[dialog.kind];
  return render ? render(dialog) : "";
}

const foot = (confirmLabel, action, kind = "primary") => `
  ${button({ label: "Cancel", data: { dismiss: "1" } })}
  ${button({ label: confirmLabel, action, kind })}`;

// ---------------------------------------------------------------- sessions --

const newSession = (dialog) =>
  modal({
    title: "New session",
    hint: `In ${project()?.title ?? ""}`,
    body: `
      ${field("Name", textInput("draft-title", dialog.title))}
      ${field("Agent", choice("agent", AGENTS, dialog.agent))}
      ${field("Model", textInput("draft-model", dialog.model, "Leave empty for the CLI default"))}
      ${field(
        "First message",
        textArea("draft-prompt", dialog.prompt, "Optional. Sent once, on the first launch only."),
        "Resuming never replays it.",
      )}`,
    foot: foot("Create", "create-session"),
  });

const editSession = (dialog) =>
  modal({
    title: "Edit session",
    hint: dialog.running ? "Running — the provider ID is fixed until it stops." : "",
    body: `
      ${field("Name", textInput("draft-title", dialog.title))}
      ${field(
        "Provider session ID",
        `<input id="draft-provider" value="${escape(dialog.provider_id)}"
                spellcheck="false" ${dialog.running ? "disabled" : ""} />`,
        "What makes an exact resume possible.",
      )}
      ${field("Notes", textArea("draft-notes", dialog.notes, "", 5))}`,
    foot: foot("Save", "save-session"),
  });

const startReview = (dialog) =>
  modal({
    title: "Start review",
    hint: `A ${dialog.reviewer === "claude" ? "Claude Code" : "Codex"} session will review this work in the same folder. It is told not to edit files.`,
    wide: true,
    body: field("Brief", textArea("draft-brief", dialog.brief, "", 14)),
    foot: foot("Create review", "create-review"),
  });

const sendFeedback = () =>
  modal({
    title: "Send feedback to builder",
    hint: "The text is typed into the builder's terminal. Nothing is submitted for you.",
    wide: true,
    body: field("Findings", textArea("draft-feedback", "", "", 12)),
    foot: foot("Insert", "insert-feedback"),
  });

const savedOutput = (dialog) =>
  modal({
    title: "Saved output",
    hint: `${dialog.title} · bounded plain text, not a transcript`,
    wide: true,
    body: `<pre class="preview__text preview__text--boxed">${escape(dialog.text)}</pre>`,
    foot: button({ label: "Close", data: { dismiss: "1" } }),
  });

const usage = (dialog) =>
  modal({
    title: "Usage limits",
    hint: dialog.loading ? "Reading…" : "",
    body: dialog.loading
      ? '<p class="modal__hint">Asking the provider…</p>'
      : dialog.windows?.length
        ? `<div class="group">${dialog.windows
            .map((window) =>
              setting(window.name, "", `<strong>${Math.round(window.percent)}%</strong>`),
            )
            .join("")}</div>`
        : `<p class="modal__hint">${escape(dialog.note ?? "No quota reported.")}</p>`,
    foot: button({ label: "Close", data: { dismiss: "1" } }),
  });

// ---------------------------------------------------------------- projects --

const projectSettings = (dialog) =>
  modal({
    title: "Project settings",
    hint: dialog.path,
    wide: true,
    body: `
      ${field("Name", textInput("draft-title", dialog.title))}
      ${field("Group", textInput("draft-group", dialog.group), "Groups the project under a heading in the sidebar.")}
      ${field("Icon", textInput("draft-icon", dialog.icon), "A single emoji, shown beside the name.")}
      ${field(
        "Shared files",
        textArea("draft-shared", dialog.shared_paths, "One repository-relative path per line", 4),
        "Copied into each new worktree. Existing files are never replaced.",
      )}
      ${field(
        "Setup command",
        textInput("draft-setup", dialog.setup_command),
        "Shown and confirmed before it runs, with your permissions.",
      )}
      ${field(
        "Review instructions",
        textArea("draft-review", dialog.review_template, "", 4),
        "Prefixed to every review brief for this project.",
      )}`,
    foot: foot("Save", "save-project"),
  });

const removeProject = (dialog) =>
  modal({
    title: "Remove project and its saved sessions?",
    body: `<p class="modal__hint">Files, worktrees and provider conversations remain on disk.</p>
           <p class="modal__hint"><code>${escape(dialog.path)}</code></p>`,
    foot: foot("Remove", "confirm-remove-project", "danger-solid"),
  });

// --------------------------------------------------------------- worktrees --

const createWorktree = (dialog) =>
  modal({
    title: "Create worktree",
    hint: "A separate checkout on a new branch. The project's own checkout stays where it is.",
    body: field("New branch", textInput("draft-branch", dialog.branch)),
    foot: foot("Create", "confirm-worktree"),
  });

const worktreeSetup = (dialog) =>
  modal({
    title: "Run this project's worktree setup?",
    body: `
      <p class="modal__hint">Copies: ${escape(dialog.shared.join(", ") || "none")}</p>
      <p class="modal__hint">Command: ${escape(dialog.command || "none")}</p>
      <p class="modal__hint"><code>${escape(dialog.directory)}</code></p>`,
    foot: `
      ${button({ label: "Skip setup", data: { dismiss: "1" } })}
      ${button({ label: "Run setup", action: "confirm-setup", kind: "primary" })}`,
  });

const removeWorktree = (dialog) =>
  modal({
    title: "Remove this worktree?",
    body: `<p class="modal__hint">
      Git removes <code>${escape(dialog.directory)}</code> only if it is clean.
      The branch is kept. All ${dialog.linked} linked sessions will be archived.
    </p>`,
    foot: foot("Remove worktree", "confirm-remove-worktree", "danger-solid"),
  });

// ---------------------------------------------------------------- settings --

const settings = (dialog) => {
  const draft = dialog.settings;
  return modal({
    title: "Settings",
    hint: "Shared with the other builds through the workspace file.",
    body: `
      ${group(
        "Appearance",
        `
        ${setting("Theme", "System follows the desktop.", segmented("theme", [["system", "System"], ["light", "Light"], ["dark", "Dark"]], draft.theme))}
        ${setting("Font size", "Terminal text, 10 to 24.", `<input type="number" id="set-font" min="10" max="24" value="${draft.font_size}" />`)}
        ${setting("Scrollback", "Lines kept per terminal, 1000 to 50000.", `<input type="number" id="set-scrollback" min="1000" max="50000" step="1000" value="${draft.scrollback}" />`)}`,
      )}
      ${group(
        "Agents",
        `
        ${setting("Default agent", "Preselected for a new session.", segmented("default_agent", AGENTS, draft.default_agent))}
        ${setting("Claude usage status line", "Replaces that launch's own status line. Takes effect next launch.", toggle("claude_usage", draft.claude_usage, "Claude usage status line"))}
        ${setting("Stop an idle Claude session after", "Minutes after it reports a finished turn. 0 disables it.", `<input type="number" id="set-hibernate" min="0" max="1440" step="5" value="${draft.hibernate_minutes}" />`)}`,
      )}
      ${group(
        "Session",
        `
        ${setting("Notify when an agent finishes or needs input", "Only while the window is not focused.", toggle("notifications", draft.notifications, "Notifications"))}
        ${setting("Keep the system awake", "Prevents idle suspension, not closing the lid.", segmented("keep_awake", [["off", "Never"], ["always", "Always"], ["sessions", "While running"]], draft.keep_awake))}`,
      )}
      ${group(
        "Shortcuts",
        SHORTCUTS.map(([action, , what]) =>
          setting(
            what,
            "",
            `<button class="button shortcut${dialog.capturing === action ? " shortcut--listening" : ""}"
                     data-capture="${action}">
               ${
                 dialog.capturing === action
                   ? "Press a key…"
                   : escape(label(draft.shortcuts?.[action] || fallback(action)))
               }
             </button>`,
          ),
        ).join(""),
      )}
      ${group(
        "Accounts",
        setting(
          `${state.profiles?.length ?? 0} saved`,
          "Each profile gets its own provider home, so sign-ins stay separate.",
          button({ label: "Manage accounts…", action: "accounts" }),
        ),
      )}`,
    note: `State lives in <code>${escape(state.storage)}</code>.`,
    foot: foot("Save", "save-settings"),
  });
};

const accounts = (dialog) =>
  modal({
    title: "Accounts",
    hint: "Sign in through the provider's own interface inside the terminal.",
    body: `
      ${group(
        "Profiles",
        (state.profiles ?? []).length
          ? state.profiles
              .map((profile) =>
                setting(
                  profile.label,
                  `${profile.agent === "claude" ? "Claude Code" : "Codex"} · ${
                    profile.sessions === 0
                      ? "not in use"
                      : `${profile.sessions} session${profile.sessions > 1 ? "s" : ""}`
                  }`,
                  button({
                    icon: "trash",
                    kind: "quiet",
                    data: { "remove-profile": profile.id },
                    disabled: profile.sessions > 0,
                    title: profile.sessions ? "Bound to saved sessions" : "Remove this record",
                  }),
                ),
              )
              .join("")
          : '<p class="modal__hint">No profiles yet. Without one, a session uses the provider\'s default home.</p>',
      )}
      ${group(
        "New profile",
        `
        ${field("Label", textInput("draft-label", dialog.label))}
        ${field("Provider", choice("agent", AGENTS, dialog.agent))}`,
      )}`,
    note: "Removing a profile removes only Convoy's record of it. Credential files stay where they are.",
    foot: `
      ${button({ label: "Close", data: { dismiss: "1" } })}
      ${button({ label: "Add profile", action: "add-profile", kind: "primary" })}`,
  });

const transcripts = (dialog) =>
  modal({
    title: "Import provider history",
    hint: "Conversations the provider already saved for this folder.",
    wide: true,
    body: `
      ${field("Provider", choice("agent", AGENTS, dialog.agent))}
      ${field(
        "Account",
        choice(
          "profile",
          [["", "Default home"], ...(state.profiles ?? []).map((p) => [p.id, p.label])],
          dialog.profile ?? "",
        ),
      )}
      ${group(
        "Conversations",
        dialog.scanning
          ? '<p class="modal__hint">Looking…</p>'
          : dialog.found === null
            ? '<p class="modal__hint">Press Scan to look for conversations in this folder.</p>'
            : dialog.found.length
              ? dialog.found
                  .map((item) =>
                    setting(
                      item.title,
                      item.provider_id,
                      button({ label: "Import", data: { import: item.provider_id, title: item.title } }),
                    ),
                  )
                  .join("")
              : '<p class="modal__hint">Nothing found. Conversations are matched by the folder the CLI ran in.</p>',
      )}`,
    foot: `
      ${button({ label: "Close", data: { dismiss: "1" } })}
      ${button({ label: "Scan", action: "scan-transcripts", kind: "primary" })}`,
  });

const quickCommands = (dialog) =>
  modal({
    title: "Quick commands",
    hint: "Saved text, sent into a running agent on demand.",
    wide: true,
    body: `
      ${group(
        "Saved",
        state.quickCommands.length
          ? state.quickCommands
              .map((command) =>
                setting(
                  command.title,
                  `${command.text.slice(0, 80)}${command.submit ? " · sends Enter" : ""}`,
                  button({
                    icon: "trash",
                    kind: "quiet",
                    data: { "remove-command": command.id },
                    title: "Remove",
                  }),
                ),
              )
              .join("")
          : '<p class="modal__hint">Nothing saved yet.</p>',
      )}
      ${group(
        "New command",
        `
        ${field("Name", textInput("draft-title", dialog.title))}
        ${field("Text", textArea("draft-text", dialog.text, "", 4))}
        ${setting("Send Enter", "Off by default: the text is typed, not submitted.", toggle("submit", dialog.submit, "Send Enter"))}
        ${setting("Only this project", "", toggle("scoped", dialog.scoped, "Only this project"))}`,
      )}`,
    foot: `
      ${button({ label: "Close", data: { dismiss: "1" } })}
      ${button({ label: "Add command", action: "add-command", kind: "primary" })}`,
  });

// ---------------------------------------------------------------- planning --

const specForm = (dialog) =>
  modal({
    title: dialog.id ? "Specification" : "New specification",
    hint: dialog.id ? `Revision ${dialog.revision} · ${dialog.approved ? "approved" : "draft"}` : "",
    wide: true,
    body: `
      ${field("Title", textInput("spec-title", dialog.title))}
      ${field("Problem", textArea("spec-problem", dialog.problem, "", 3))}
      ${field("Requirements", textArea("spec-requirements", dialog.requirements, "", 4))}
      ${field("Acceptance", textArea("spec-acceptance", dialog.acceptance, "", 3))}
      ${field("Constraints", textArea("spec-constraints", dialog.constraints, "", 3))}
      ${field("Plan", textArea("spec-plan", dialog.plan, "", 4))}`,
    foot: `
      ${button({ label: "Cancel", data: { dismiss: "1" } })}
      ${dialog.id && !dialog.approved ? button({ label: "Approve revision", action: "approve-spec" }) : ""}
      ${button({ label: "Save", action: "save-spec", kind: "primary" })}`,
  });

const taskForm = (dialog) => {
  const specs = state.planning?.specs ?? [];
  return modal({
    title: dialog.id ? "Task" : "New task",
    hint: dialog.id ? `${dialog.status}${dialog.last_error ? ` · ${dialog.last_error}` : ""}` : "",
    wide: true,
    body: `
      ${field("Title", textInput("task-title", dialog.title))}
      ${field("Agent", choice("agent", AGENTS, dialog.agent))}
      ${field(
        "Specification",
        specs.length
          ? choice(
              "spec",
              [["", "None"], ...specs.map((spec) => [spec.id, spec.title])],
              dialog.spec_id ?? "",
            )
          : '<span class="field__note">No specifications in this project.</span>',
        dialog.id ? "Fixed once the task exists." : "",
      )}
      ${field(
        "Publishing",
        choice(
          "mode",
          [["none", "Do not publish"], ["pr", "Pull request"], ["push", "Push"]],
          dialog.mode,
        ),
        "No publishing is the default.",
      )}
      ${setting("Hand to the other agent when it finishes", "", toggle("auto_review", dialog.auto_review, "Automatic review"))}
      ${field("Details", textArea("task-details", dialog.details, "", 4))}
      ${field("Findings", textArea("task-findings", dialog.findings, "", 3))}`,
    foot: `
      ${button({ label: "Cancel", data: { dismiss: "1" } })}
      ${dialog.id ? button({ label: "Back to queue", data: { status: "queued" } }) : ""}
      ${dialog.id ? button({ label: "Accept", data: { status: "done" } }) : ""}
      ${button({ label: "Save", action: "save-task", kind: "primary" })}`,
  });
};

const queueConfirm = (dialog) =>
  modal({
    title: `Run ${dialog.summary.length} queued tasks?`,
    body: `
      <ul class="plain-list">
        ${dialog.summary.map((line) => `<li>${escape(line)}</li>`).join("")}
      </ul>
      <p class="modal__hint">
        Runs the installed agents. Publishing happens only for tasks configured
        for it. A failure pauses the queue.
      </p>`,
    foot: foot("Run queue", "confirm-queue"),
  });

// ------------------------------------------------------------------- misc --

const newBranch = (dialog) =>
  modal({
    title: "New branch",
    body: field("Branch name", textInput("draft-branch", dialog.branch)),
    foot: foot("Create and switch", "confirm-branch"),
  });

const hunkChoice = (dialog) =>
  modal({
    title: "Discard which hunk?",
    hint: dialog.path,
    body: choice(
      "hunk",
      dialog.hunks.map((label, index) => [String(index), `${index + 1}. ${label}`]),
      String(dialog.hunk ?? 0),
    ),
    foot: foot("Discard hunk", "confirm-hunk", "danger-solid"),
  });

const confirmAction = (dialog) =>
  modal({
    title: dialog.title,
    body: `<p class="modal__hint">${escape(dialog.body)}</p>`,
    foot: foot(dialog.confirmLabel, "confirm-generic", dialog.danger ? "danger-solid" : "primary"),
  });

const palette = (dialog) => `
  <div class="scrim" data-dismiss="1">
    <div class="modal modal--palette" role="dialog" aria-modal="true" aria-label="Commands">
      <div class="palette__search">
        <input id="palette-input" type="search" placeholder="Type a command, project or session"
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
            <span class="palette__title">${escape(entry.title)}</span>
            <span class="palette__kind">${escape(entry.kind)}</span>
          </button>`,
                )
                .join("")
            : '<div class="menu__empty">Nothing matches.</div>'
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
      ${button({ label: "Close", data: { dismiss: "1" } })}
      ${button({ label: "Copy", action: "copy-markdown", kind: "primary" })}`,
  });

/// Which other session to show beside this one. A split is two terminals, not
/// two windows: the same list, minus the one already on screen.
function splitPicker() {
  const others = state.sessions.filter(
    (item) => item.id !== state.sessionId && !item.archived,
  );
  return modal({
    title: "Open split terminal",
    hint: "The chosen session appears beside this one and keeps running when the split is closed.",
    body: others.length
      ? `<div class="plain-list plain-list--rows">
           ${others
             .map(
               (item) => `
             <button class="menu__item" data-split="${escape(item.id)}">
               <span>${escape(item.title)}</span>
               <span class="menu__hint">${item.running ? "running" : item.started ? "stopped" : "never started"}</span>
             </button>`,
             )
             .join("")}
         </div>`
      : '<p class="modal__hint">There is no other session in this project.</p>',
    foot: button({ label: "Cancel", data: { dismiss: "1" } }),
  });
}

/// Closing the window ends every agent with it.
const quitting = (dialog) =>
  modal({
    title: dialog.running === 1 ? "An agent is still running" : "Agents are still running",
    body: `<p class="modal__hint">
      Closing Convoy stops ${dialog.running === 1 ? "it" : `all ${dialog.running}`}.
      Anything not written to disk by then is gone.
    </p>`,
    foot: `
      ${button({ label: "Stay open", data: { dismiss: "1" } })}
      ${button({ label: "Stop and quit", action: "quit", kind: "danger-solid" })}`,
  });

const VIEWS = {
  split: splitPicker,
  quit: quitting,
  session: newSession,
  "edit-session": editSession,
  review: startReview,
  feedback: sendFeedback,
  output: savedOutput,
  usage,
  project: projectSettings,
  "remove-project": removeProject,
  worktree: createWorktree,
  "worktree-setup": worktreeSetup,
  "remove-worktree": removeWorktree,
  settings,
  accounts,
  transcripts,
  commands: quickCommands,
  spec: specForm,
  task: taskForm,
  queue: queueConfirm,
  branch: newBranch,
  hunk: hunkChoice,
  confirm: confirmAction,
  palette,
  markdown: markdownPreview,
};

export { session, project };
