// Project settings, in four parts: General (name, colour, icon, agent,
// folder), How tasks run (a preset, then each step), Tracker, and Advanced
// (git, roles, skills, quick commands, removal). Each change is saved as it
// is made.

import { SYMBOLS, agentIcon, icon, projectIcon } from "./icons.js";
import { icons } from "./icons.js";
import { button, escape, shorten } from "./ui.js";
import { state } from "./state.js";
import { t } from "./i18n.js";
import { TASK_MODES, taskModeOf } from "./task-defaults.js";
import { CUSTOM, PRESETS, PRESET_ORDER, canTest, isRepository, presetOf } from "./presets.js";
import { ROLES, projectSkills } from "./views-skills.js";

/// The macOS app's palette and emoji choices, in its order.
export const COLORS = ["#5E6AD2", "#4CAF83", "#E5A54B", "#D65C5C", "#3FA7D6", "#B067C9", "#8A8F98", "#E07C4C"];
const EMOJIS = ["🚀", "🧪", "📦", "🛒", "💳", "📱", "🌐", "🤖", "🧭", "🛠️", "🏦", "🚚", "📊", "🔐", "🧾", "🎯", "🧠", "⚙️", "🧱", "🛰️", "🗂️", "🧩", "📡", "🏗️"];

const groupBox = (title, rows, footer = "") => `
  <section class="pref-group">
    <h3 class="pref-group__title">${escape(title)}</h3>
    <div class="pref-group__box">${rows}</div>
    ${footer ? `<p class="pref-group__footer">${escape(footer)}</p>` : ""}
  </section>`;

const row = (title, detail, control) => `
  <div class="pref-row">
    <div class="pref-row__text">
      <div class="pref-row__title">${escape(title)}</div>
      ${detail ? `<div class="pref-row__detail">${escape(detail)}</div>` : ""}
    </div>
    <div class="pref-row__control">${control}</div>
  </div>`;

const text = (key, value, placeholder, wide = false) =>
  `<input class="pref-text${wide ? " pref-text--wide" : ""}" data-proj-text="${key}"
          value="${escape(value ?? "")}" placeholder="${escape(placeholder)}" spellcheck="false" />`;

const area = (key, value, placeholder, rows = 3) => `
  <div class="pref-block">
    <textarea class="pref-area" data-proj-text="${key}" rows="${rows}" spellcheck="false"
              placeholder="${escape(placeholder)}">${escape(value ?? "")}</textarea>
  </div>`;

const select = (key, options, selected) =>
  `<select class="pref-select" data-proj-select="${key}">
     ${options.map(([value, name]) => `<option value="${escape(value)}"${value === selected ? " selected" : ""}>${escape(name)}</option>`).join("")}
   </select>`;

const TRACKERS = { linear: "Linear", jira: "Jira", peshbar: "Peshbar", notion: "Notion" };


/// Each role's instructions in this project; an empty field follows the
/// global ones, shown as its placeholder.
function roles(project) {
  const own = project.role_instructions ?? {};
  const fallback = project.role_fallbacks ?? {};
  return groupBox(
    t("Roles"),
    ROLES.map(
      ([role, name, about]) => `
      <div class="pref-block pref-stack">
        <div class="pref-row__title">${escape(t(name))}</div>
        <div class="pref-row__detail">${escape(t(about))}</div>
        <textarea class="pref-area" data-proj-role="${role}" rows="4" spellcheck="false"
                  placeholder="${escape(fallback[role] || t("No instructions"))}">${escape(own[role] ?? "")}</textarea>
      </div>`,
    ).join(""),
    t("Empty fields follow Settings → Agents → Roles. Skills reach a role when they are marked for it in the skill's editor."),
  );
}

const toggle = (key, on, label) =>
  `<button class="switch" data-proj-toggle="${key}" aria-pressed="${!!on}" aria-label="${escape(label)}"></button>`;

/// One step of the road a task takes: its switch, what it does, and its
/// options while it is on. `forNew` says the switch sets new tasks only.
function stepCard({ key, title, about, control, on, options = "", forNew = false, open = false }) {
  return `
    <section class="step-card step-card--${on ? "on" : "off"}" data-step-card="${key}">
      <div class="step-card__head">
        <div class="pref-row__text">
          <div class="step-card__title">${escape(title)}</div>
          <div class="pref-row__detail">${escape(about)}${forNew ? ` <em>${escape(t("Applies to new tasks."))}</em>` : ""}</div>
        </div>
        <div class="pref-row__control">${control}</div>
      </div>
      ${(on || open) && options ? `<div class="step-card__options">${options}</div>` : ""}
    </section>`;
}

/// Presets, then the steps: how this project's tasks run, in the order they
/// run.
function pipeline(project) {
  const repository = isRepository(project);
  const chosen = presetOf(project);
  const presets = `
    <div class="presets" role="radiogroup" aria-label="${escape(t("How tasks run"))}">
      ${PRESET_ORDER.map((name) => {
        const [label, about] = PRESETS[name];
        return `<button class="preset" role="radio" data-preset="${name}" aria-checked="${chosen === name}">
            <span class="preset__name">${escape(t(label))}</span>
            <span class="preset__about">${escape(t(about))}</span>
          </button>`;
      }).join("")}
      ${chosen === "custom" ? `<div class="preset preset--custom" aria-checked="true"><span class="preset__name">${escape(t(CUSTOM[0]))}</span><span class="preset__about">${escape(t(CUSTOM[1]))}</span></div>` : ""}
    </div>`;

  const timeouts = [5, 10, 15, 30, 60, 120];
  if (project.qa_timeout && !timeouts.includes(project.qa_timeout)) timeouts.push(project.qa_timeout);
  const testOptions = `
     <div class="pref-block pref-stack">
       <div class="pref-row__title">${escape(t("Test commands (one per line)"))}</div>
       <textarea class="pref-area pref-area--mono" data-proj-text="test_commands" rows="3" spellcheck="false"
                 placeholder="npm test&#10;npm run lint">${escape(project.test_commands ?? "")}</textarea>
       <div class="pref-inline pref-inline--flush">
         ${button({ label: t("Detect from the project"), icon: "sparkles", action: "qa-detect", kind: "quiet" })}
         <span class="pref-row__detail">${escape(t("Run in the task's own checkout, in order; the first failure ends the run."))}</span>
       </div>
     </div>
     ${row(
       t("Then test with an agent"),
       t("Tests the change end to end against the task and reports a verdict. It does not edit code."),
       select("qa_agent", [["", t("No agent")], ["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini CLI"]], project.qa_agent ?? ""),
     )}
     <div class="pref-block pref-stack">
       <div class="pref-row__title">${escape(t("Shared QA environment"))}</div>
       <input class="pref-text pref-text--wide" data-proj-text="qa_environment" spellcheck="false"
              value="${escape(project.qa_environment ?? "")}" placeholder="${escape(t("e.g. local-db"))}" />
       <div class="pref-row__detail">${escape(t("Projects with the same name here test one task at a time: the others wait their turn. Leave empty when QA runs can overlap."))}</div>
     </div>
     ${row(
       t("Time limit per check"),
       "",
       select("qa_timeout", timeouts.sort((a, b) => a - b).map((minutes) => [String(minutes), t("{minutes} min", { minutes })]), String(project.qa_timeout ?? 15)),
     )}`;

  const mode = taskModeOf(project);
  return `
    ${presets}
    <div class="step-cards">
      ${stepCard({
        key: "plan",
        title: t("Plan"),
        about: t("A planner writes the plan before anything is built; you approve it or ask for changes."),
        control: toggle("plan_first_default", project.plan_first_default, t("Plan new tasks first")),
        on: !!project.plan_first_default,
        forNew: true,
        options: row(
          t("Approve plans automatically"),
          t("The builder starts as soon as the plan is written. A task can say otherwise."),
          toggle("plan_auto_approve", project.plan_auto_approve, t("Approve plans automatically")),
        ),
      })}
      ${stepCard({
        key: "build",
        title: t("Build"),
        about: t("The task's agent writes the change. Always on."),
        control: "",
        on: true,
        options: `${row(
          t("A worktree for each task"),
          repository
            ? t("Each task builds on a branch of its own, set up with the shared paths and setup commands in Advanced. Turning this on lets those commands run without asking.")
            : t("This project is a folder, not a git repository, so its tasks work in the folder itself. Turn this on in the repositories inside it."),
          toggle("task_worktrees", project.task_worktrees, t("A worktree for each task")),
        )}
        ${row(
          t("Tasks at once"),
          project.task_worktrees && repository ? t("How many queued tasks run side by side.") : t("Needs a worktree for each task; one checkout runs one task."),
          select("max_parallel", [1, 2, 3, 4, 6, 8].map((count) => [String(count), String(count)]), String(project.max_parallel ?? 1)),
        )}
        ${row(t("Auto-run task queue"), t("Start the next queued task when one finishes."), toggle("auto_run_tasks", project.auto_run_tasks, t("Auto-run task queue")))}
        ${row(
          t("Rounds of automatic fixes"),
          t("A failed review or QA goes back to the builder with the findings this many times; after that the task needs you."),
          select("max_fix_rounds", [0, 1, 2, 3, 4, 5].map((rounds) => [String(rounds), rounds === 0 ? t("None") : String(rounds)]), String(project.max_fix_rounds ?? 2)),
        )}`,
      })}
      ${stepCard({
        key: "review",
        title: t("Review"),
        about: t("The other agent reviews each round before testing; what it finds goes back to the builder."),
        control: toggle("review_auto", project.review_auto, t("Review tasks automatically")),
        on: !!project.review_auto,
        forNew: true,
      })}
      ${stepCard({
        key: "test",
        title: t("Test"),
        about: canTest(project)
          ? t("The project's checks run, then a QA agent if one is chosen. A failure goes back to the builder.")
          : t("Add test commands or choose a QA agent to test tasks."),
        control: toggle("qa_auto", project.qa_auto, t("Test tasks when they finish")),
        on: !!project.qa_auto,
        open: true,
        options: testOptions,
      })}
      ${stepCard({
        key: "publish",
        title: t("Publish"),
        about: t("How a finished task leaves this machine."),
        control: select("task_mode", TASK_MODES.map(([id, name]) => [id, t(name)]), mode),
        on: mode !== "none",
        forNew: true,
        options: `${row(
          t("Convoy publishes"),
          repository
            ? t("Builders commit and stop; once review and QA pass, Convoy pushes the branch and opens the request, with a description from the Pull request author role.")
            : t("This project is a folder, not a git repository: its builders publish in the repositories they changed, as the task's publishing says. Turn this on in the repositories inside it."),
          toggle("convoy_publishes", project.convoy_publishes, t("Convoy publishes")),
        )}
        ${row(
          t("Requests open with"),
          t("Read from the remote's host when automatic: glab for GitLab, gh otherwise."),
          select("pr_cli", [["", t("Automatic")], ["gh", "gh (GitHub)"], ["glab", "glab (GitLab)"]], project.pr_cli ?? ""),
        )}`,
      })}
      ${stepCard({
        key: "document",
        title: t("Document"),
        about: t("A documentation writer, with the Documentation role's instructions, writes it once QA passes."),
        control: toggle("docs_auto", project.docs_auto, t("Write the document when QA passes")),
        on: !!project.docs_auto,
        options: `<div class="pref-block pref-stack">
           <div class="pref-row__title">${escape(t("Documents folder"))}</div>
           <input class="pref-text pref-text--wide" data-proj-text="docs_dir" spellcheck="false"
                  value="${escape(project.docs_dir ?? "")}" placeholder="${escape(project.docs_default_dir ?? "")}" />
           <div class="pref-row__detail">${escape(t("A full path outside the repository, such as ~/Desktop/EskhataDocs. Empty keeps documents in Convoy's own data."))}</div>
         </div>`,
      })}
    </div>`;
}

/// One level's picker: what the tracker listed, the saved choice even when
/// the list could not be read, and "Any" for not narrowing.
function levelPicker(id, level, chosen, any) {
  const items = level?.items ?? [];
  const options = [["", any], ...items.map((item) => [item.id, item.key ? `${item.name} · ${item.key}` : item.name])];
  if (chosen && !items.some((item) => item.id === chosen.id)) options.push([chosen.id, chosen.name]);
  const busy = level?.loading;
  return `<select class="pref-select" id="${id}"${busy ? " disabled" : ""}>
      ${busy ? `<option>${escape(t("Loading…"))}</option>` : options.map(([value, name]) => `<option value="${escape(value)}"${value === (chosen?.id ?? "") ? " selected" : ""}>${escape(name)}</option>`).join("")}
    </select>`;
}

/// The Convoy statuses a tracker can follow, in the order a task moves.
const SYNCED = [
  ["building", "Running", "running"],
  ["review", "Needs review", "review"],
  ["pr", "Pull request", "review"],
  ["testing", "In testing", "testing"],
  ["changes", "Needs changes", "open"],
  ["done", "Done", "done"],
  ["failed", "Failed", "failed"],
  ["queued", "Queued", "idle"],
];

/// What the tracker cannot do that a person would expect it to.
const SYNC_NOTES = {
  jira: "Jira moves an issue only along its workflow: a status it cannot reach from where the issue is shows as a failed sync on the task.",
  notion: "The integration needs Update content to set the status, and Insert comments when the database has no PR link property.",
  linear: "If Linear's GitHub integration also moves issues, let one of them do it, or they will race.",
  peshbar: "Statuses are matched by name in each task's own Peshbar project.",
};

/// Pushing task status back: a switch, then which tracker status each
/// Convoy status becomes, read from the tracker and suggested by name.
function statusSync(link, connection) {
  const tracker = TRACKERS[connection.kind] ?? connection.kind;
  const sync = link.sync ?? { enabled: false, map: {}, link_pr: true };
  const toggle = (key, on, label) =>
    `<button class="switch" data-sync-toggle="${key}" aria-pressed="${!!on}" aria-label="${escape(label)}"></button>`;
  const head = row(
    t("Update {tracker} when a task changes", { tracker }),
    t("Sets the issue's status as the task moves in Convoy."),
    toggle("enabled", sync.enabled, t("Update {tracker} when a task changes", { tracker })),
  );
  if (!sync.enabled) return head;
  if (connection.kind === "notion" && !link.workspace) {
    return head + `<div class="pref-block"><div class="pref-row__detail">${escape(t("Pick the database above; its status property holds the choices."))}</div></div>`;
  }
  const choices = state.syncChoices;
  let table;
  if (!choices || choices.loading) {
    table = `<div class="pref-row__detail">${escape(t("Reading {tracker}'s statuses…", { tracker }))}</div>`;
  } else if (choices.error) {
    table = `<div class="pref-row__detail pref-row__detail--error">${escape(choices.error)}</div>`;
  } else {
    const names = choices.statuses.map((status) => status.name);
    table = `
      <div class="sync-map" role="table" aria-label="${escape(t("Status mapping"))}">
        <div class="sync-map__head" role="row">
          <span role="columnheader">${escape(t("When a task is"))}</span>
          <span role="columnheader">${escape(t("Set {tracker} to", { tracker }))}</span>
        </div>
        ${SYNCED.map(([key, label, tone]) => {
          const chosen = sync.map?.[key]?.name ?? "";
          const options = [["", t("Don't change")], ...names.map((name) => [name, name])];
          if (chosen && !names.includes(chosen)) options.push([chosen, t("{name} (not found)", { name: chosen })]);
          return `
            <div class="sync-map__row" role="row">
              <span class="sync-map__status" role="cell"><span class="state__dot state__dot--${tone}"></span>${escape(t(label))}</span>
              <span role="cell">
                <select class="pref-select${chosen ? "" : " pref-select--quiet"}" data-sync-status="${key}" aria-label="${escape(t(label))}">
                  ${options.map(([value, name]) => `<option value="${escape(value)}"${value === chosen ? " selected" : ""}>${escape(name)}</option>`).join("")}
                </select>
              </span>
            </div>`;
        }).join("")}
        <div class="sync-map__foot">
          <button class="link-button" data-action="sync-suggest">${escape(t("Use suggested"))}</button>
        </div>
      </div>`;
  }
  return `${head}
    <div class="pref-block">${table}</div>
    ${row(
      t("Add the pull request to the issue"),
      { linear: t("As an attachment, once the pull request opens."), jira: t("As a remote link, once the pull request opens."), peshbar: t("As a comment, once the pull request opens."), notion: t("In a PR URL property, or as a comment.") }[connection.kind] ?? "",
      toggle("link_pr", sync.link_pr, t("Add the pull request to the issue")),
    )}
    ${SYNC_NOTES[connection.kind] ? `<div class="pref-block"><div class="pref-row__detail">${escape(t(SYNC_NOTES[connection.kind]))}</div></div>` : ""}`;
}

/// Where this project imports tasks from: a connection, and in it a
/// workspace and a project. The import searches only there.
function integration(project) {
  const connections = project.connections ?? [];
  const link = project.link;
  const current = connections.find((item) => item.id === link?.connection_id);
  const levels = state.linkLevels ?? {};
  if (!connections.length) {
    return groupBox(
      t("Integration"),
      row(t("No integrations yet"), t("Add Linear, Jira, Peshbar or Notion, then link this project to a workspace or database in it."), button({ label: t("Integrations…"), action: "integrations" })),
    );
  }
  const [first, second] = current?.levels ?? [];
  const failure = levels.workspaces?.error ?? levels.projects?.error;
  const rows = [
    row(
      t("Import from"),
      "",
      `<select class="pref-select" id="link-connection">
         ${[["", t("Not linked")], ...connections.map((item) => [item.id, `${item.name} · ${TRACKERS[item.kind] ?? item.kind}`])]
           .map(([value, name]) => `<option value="${escape(value)}"${value === (link?.connection_id ?? "") ? " selected" : ""}>${escape(name)}</option>`)
           .join("")}
       </select>`,
    ),
  ];
  if (current?.auth === "mcp") {
    rows.push(row(t("Read by the agent"), t("An MCP connection is read through the agent's own server; there is nothing to narrow here."), ""));
  } else if (current) {
    rows.push(row(t(first), "", levelPicker("link-workspace", levels.workspaces, link.workspace, t("Any"))));
    if (second && link.workspace) rows.push(row(t(second), "", levelPicker("link-project", levels.projects, link.project, t("Any"))));
    if (failure) rows.push(`<div class="pref-block"><div class="pref-row__detail pref-row__detail--error">${escape(failure)}</div></div>`);
    rows.push(statusSync(link, current));
    rows.push(intakeRows(link, current));
  }
  return groupBox(
    t("Integration"),
    rows.join(""),
    t("Import tasks searches only here, and opens on this connection. Each project keeps its own link."),
  );
}

/// Importing new issues by itself: a saved search, every few minutes, and
/// whether the queue runs what it found.
function intakeRows(link, connection) {
  const intake = link.intake ?? {};
  const on = !!intake.enabled;
  const rows = [
    row(
      t("Import new issues by themselves"),
      t("Searches this link on a schedule; what is not imported yet becomes tasks with this project's defaults."),
      `<button class="switch" data-intake-toggle="enabled" aria-pressed="${on}" aria-label="${escape(t("Import new issues by themselves"))}"></button>`,
    ),
  ];
  if (on) {
    rows.push(`
      <div class="pref-block pref-stack">
        <div class="pref-row__title">${escape(connection.kind === "jira" ? t("Search or JQL") : t("Search"))}</div>
        <input class="pref-text pref-text--wide" data-intake-text="query" spellcheck="false"
               value="${escape(intake.query ?? "")}" placeholder="${escape(connection.kind === "jira" ? 'status = "To Do" AND assignee = currentUser()' : t("Empty finds every open issue"))}" />
        <div class="pref-inline pref-inline--flush">
          ${connection.kind === "jira" ? `<label class="check"><input type="checkbox" data-intake-check="raw_jql"${intake.raw_jql ? " checked" : ""} /> ${escape(t("JQL as written"))}</label>` : ""}
          <label class="check"><input type="checkbox" data-intake-check="mine_only"${intake.mine_only ? " checked" : ""} /> ${escape(t("Only mine"))}</label>
        </div>
      </div>`);
    rows.push(
      row(
        t("Every"),
        "",
        `<select class="pref-select" data-intake-select="minutes">${[5, 10, 15, 30, 60]
          .map((minutes) => `<option value="${minutes}"${minutes === (intake.minutes ?? 15) ? " selected" : ""}>${escape(t("{minutes} min", { minutes }))}</option>`)
          .join("")}</select>`,
      ),
      row(
        t("Run what is imported"),
        t("Queues the new tasks and starts the queue."),
        `<button class="switch" data-intake-toggle="run" aria-pressed="${!!intake.run}" aria-label="${escape(t("Run what is imported"))}"></button>`,
      ),
    );
    if (intake.last_result) {
      rows.push(`<div class="pref-block"><div class="pref-row__detail">${escape(t("Last search: {result}", { result: intake.last_result }))}</div></div>`);
    }
  }
  return rows.join("");
}

function identity(project) {
  const tab = state.iconTab ?? "emoji";
  const tile = (value, inner, title) => `
    <button class="icon-tile" data-proj-icon="${escape(value)}" aria-pressed="${project.icon === value}"
            title="${escape(title)}">${inner}</button>`;
  const picker =
    tab === "symbol"
      ? `<div class="icon-grid">${Object.keys(SYMBOLS)
          .map((name) => tile(`sf:${name}`, icon(SYMBOLS[name], 16), name))
          .join("")}</div>`
      : tab === "image"
        ? `<div class="icon-images">
             <div class="pref-inline pref-inline--flush">
               ${button({ label: t("Use GitHub avatar"), icon: "person", data: { "icon-source": "github" } })}
               ${button({ label: t("Upload PNG…"), icon: "download", data: { "icon-source": "upload" } })}
             </div>
             <div class="pref-inline pref-inline--flush">
               <input id="proj-favicon" class="pref-text pref-text--wide" placeholder="example.com" spellcheck="false" />
               ${button({ label: t("Favicon"), icon: "link", data: { "icon-source": "favicon" } })}
             </div>
             ${state.iconBusy ? `<p class="pref-group__footer">${escape(t("Downloading…"))}</p>` : ""}
           </div>`
        : `<div class="icon-grid">${EMOJIS.map((emoji) => tile(emoji, `<span class="icon-tile__emoji">${emoji}</span>`, emoji)).join("")}</div>`;

  return groupBox(
    t("Identity"),
    `${row(t("Display name"), "", text("title", project.title, t("Name"), true))}
     ${row(t("Group"), t("Shown as a heading above related projects in the sidebar."), text("group", project.group, t("none")))}
     <div class="pref-block pref-stack">
       <div class="pref-row__title">${escape(t("Color"))}</div>
       <div class="swatches">
         ${COLORS.map(
           (hex) => `<button class="swatch" data-proj-color="${hex}" style="background:${hex}"
                             aria-pressed="${project.color?.toUpperCase() === hex}" title="${hex}"></button>`,
         ).join("")}
         <input class="pref-text pref-text--hex" data-proj-text="color" value="${escape(project.color ?? "")}"
                placeholder="#5E6AD2" spellcheck="false" />
         ${button({ label: t("Auto"), data: { "proj-color": "" }, kind: "quiet", title: t("Derive a colour from the name") })}
       </div>
       <div class="pref-row__title">${escape(t("Icon"))}</div>
       <div class="segmented">
         ${[
           ["emoji", "Emoji"],
           ["symbol", "Icon"],
           ["image", "Image"],
         ]
           .map(([value, name]) => `<button data-icon-tab="${value}" aria-pressed="${tab === value}">${escape(t(name))}</button>`)
           .join("")}
       </div>
       ${picker}
       <div class="pref-inline pref-inline--flush">
         <span class="icon-preview">${projectIcon(project, 22)}</span>
         ${button({ label: t("Reset to folder icon"), data: { "proj-icon": "" }, disabled: !project.icon })}
       </div>
     </div>`,
    t("Shown in the sidebar, tabs and menus so parallel projects stay distinguishable."),
  );
}

/// The page's four parts: who the project is, how its tasks run, which
/// tracker it follows, and the rest.
const PAGE_TABS = [
  ["general", "General"],
  ["pipeline", "How tasks run"],
  ["tracker", "Tracker"],
  ["advanced", "Advanced"],
];

export function projectSettingsPage() {
  const project = state.projectDraft;
  if (!project) return "";
  const global = { codex: "Codex", gemini: "Gemini CLI" }[state.settings.default_agent] ?? "Claude Code";
  const commands = (state.quickCommands ?? []).filter((command) => command.project_id === project.id);
  const tab = PAGE_TABS.some(([key]) => key === state.projectSettingsTab) ? state.projectSettingsTab : "general";
  return `
    <div class="prefs__page prefs__page--project" data-scroll="project-settings">
      <div class="prefs__content">
        <button class="prefs__back prefs__back--inline" data-action="close-project-settings">${icons.back} ${escape(t("Back"))}</button>
        <div class="project-title">
          <span class="icon-preview">${projectIcon(project, 26)}</span>
          <div>
            <h1 class="prefs__title">${escape(project.title)}</h1>
            <div class="pref-row__detail">${escape(t("Project settings"))}</div>
          </div>
        </div>
        <div class="segmented project-tabs" role="tablist">
          ${PAGE_TABS.map(([key, label]) => `<button role="tab" data-project-tab="${key}" aria-pressed="${tab === key}">${escape(t(label))}</button>`).join("")}
        </div>
        ${tab === "pipeline" ? pipeline(project) : ""}
        ${tab === "tracker" ? integration(project) : ""}
        ${
          tab === "general"
            ? `${identity(project)}
        ${groupBox(
          t("Agent"),
          row(
            t("Default agent"),
            "",
            select(
              "default_agent",
              [
                ["", t("Use global ({agent})", { agent: global })],
                ["claude", "Claude Code"],
                ["codex", "Codex"],
                ["gemini", "Gemini CLI"],
              ],
              project.default_agent ?? "",
            ),
          ),
          t("Preselected for new sessions and tasks in this project. Global default: {agent}.", { agent: global }),
        )}
        ${groupBox(
          t("Folder"),
          row(
            project.path,
            "",
            `${button({ label: t("Show"), icon: "folder", action: "reveal-project" })}
             ${button({ label: t("Reconnect…"), icon: "link", action: "reconnect-project" })}`,
          ),
        )}`
            : ""
        }
        ${
          tab === "advanced"
            ? `${groupBox(
          t("Git & worktrees"),
          `${row(t("Base ref for worktrees"), t("Empty starts from the current HEAD."), text("base_ref", project.base_ref, "main"))}
           ${row(t("Branch prefix"), t("Global: {prefix}", { prefix: state.settings.branch_prefix || "convoy" }), text("branch_prefix", project.branch_prefix, "feature"))}
           <div class="pref-block pref-stack">
             <div class="pref-row__title">${escape(t("Shared paths (one per line)"))}</div>
             <textarea class="pref-area" data-proj-text="shared_paths" rows="3" spellcheck="false"
                       placeholder=".env">${escape(project.shared_paths ?? "")}</textarea>
             <div class="pref-row__detail">${escape(t("e.g. {examples}", { examples: ".env, node_modules, vendor" }))}</div>
             <div class="pref-row__title">${escape(t("Worktree setup commands"))}</div>
             <textarea class="pref-area" data-proj-text="setup_command" rows="3" spellcheck="false"
                       placeholder="pnpm install">${escape(project.setup_command ?? "")}</textarea>
             <div class="pref-row__detail">${escape(t("e.g. {examples}", { examples: "pnpm install · composer install · direnv allow" }))}</div>
           </div>`,
          t("Shared paths are gitignored files copied into each new worktree; existing files are never replaced. Setup commands run once, after you confirm, following the global ones in Settings → Worktrees."),
        )}
        ${roles(project)}
        ${projectSkills(project.id)}
        ${groupBox(
          t("Quick commands for this project"),
          (commands.length
            ? commands
                .map((command) =>
                  row(
                    command.title,
                    shorten(command.text.replace(/\n/g, " "), 100),
                    button({ icon: "trash", kind: "quiet", data: { "remove-command": command.id }, title: t("Remove") }),
                  ),
                )
                .join("")
            : `<div class="pref-empty">${escape(t("None yet."))}</div>`) +
            `<div class="pref-inline">${button({ label: t("Add"), icon: "plus", action: "manage-commands" })}</div>`,
          t("Sent to a terminal from the ⚡ menu. Global commands are managed in Settings → Commands & shortcuts."),
        )}
        ${groupBox(
          t("Remove"),
          row(
            t("Remove project from Convoy"),
            "",
            button({ label: t("Remove…"), kind: "danger", action: "remove-project", disabled: project.running > 0 }),
          ),
          t("Stop this project's running sessions first. Files, worktrees and the agents' own history stay on disk."),
        )}`
            : ""
        }
      </div>
    </div>`;
}

export { agentIcon };
