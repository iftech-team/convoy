// Skills: named Markdown instructions attached to sessions, tasks and
// automations. Which apply, and how they reach the agent, are the backend's
// rules (convoy-core); this draws the library, its editor, the pickers and
// a session's chip and menu.

import { button, choice, escape, field, modal, plural, setting, textInput, toggle } from "./ui.js";
import { icons } from "./icons.js";
import { t } from "./i18n.js";
import { state } from "./state.js";
import { markdown } from "./views-files.js";

/// The parts an agent plays in a task, in the order they come.
export const ROLES = [
  ["plan", "Planner", "Writes the plan before anything is built, when a task is planned first."],
  ["build", "Builder", "Builds the task. Added to its brief."],
  ["review", "Reviewer", "Reviews each round of work. Reports a verdict; edits nothing."],
  ["qa", "QA", "Tests the change end to end after the checks and the review."],
  ["docs", "Documentation", "Writes the task's document once QA passes."],
];

export const AGENT_SCOPES = [
  ["both", "Both agents"],
  ["claude", "Claude Code"],
  ["codex", "Codex"],
];

/// Three short, generic starting points, offered while the library is empty.
export const TEMPLATES = [
  {
    key: "commits",
    name: "Small focused commits",
    description: "One logical change per commit, with a clear message.",
    body: "Make small, focused commits: one logical change each.\n\n- Keep refactors apart from behaviour changes.\n- Write the message as a short imperative summary line, then why, if it is not obvious.\n- Never commit unrelated files or generated noise.",
  },
  {
    key: "tests",
    name: "Write tests first",
    description: "A failing test before the fix or feature.",
    body: "Before changing behaviour, write a test that fails for the right reason.\n\n1. Add or extend a test that shows the bug or the missing feature.\n2. Run it and confirm it fails.\n3. Make the smallest change that makes it pass, then run the whole suite.",
  },
  {
    key: "qa",
    name: "QA tester",
    description: "Test the change like a user and report reproducible findings.",
    body: "Test the change end to end rather than reading the code.\n\n1. Run the project's tests and note any failure with its output.\n2. If there is a user interface or an API, use it the way a user would: the happy path, empty and wrong input, slow or failing network.\n3. Check what sits next to the change for regressions.\n4. Do not edit, commit or push anything.\n5. Report numbered findings, each with steps to reproduce, what happened and what you expected, then a verdict: pass or fail.",
  },
  {
    key: "explain",
    name: "Explain before editing",
    description: "Say what will change and why before touching files.",
    body: "Before editing any file, explain in a few sentences what you are going to change and why. If the change is large or risky, wait for a go-ahead. Afterwards, summarise what changed.",
  },
];

const all = () => state.skills ?? [];
export const findSkill = (id) => all().find((item) => item.id === id);

const projectName = (id) => state.projects.find((item) => item.id === id)?.title ?? t("Unknown project");

/// "Everywhere" or the project's name.
export const scopeText = (skill) => (skill.project_id ? projectName(skill.project_id) : t("Everywhere"));

export const agentsText = (agents) => t((AGENT_SCOPES.find(([value]) => value === agents) ?? AGENT_SCOPES[0])[1]);

/// Skills a session of this agent in this project may have, in name order.
export function skillsFor(agent, projectId) {
  return all()
    .filter((item) => !item.project_id || item.project_id === projectId)
    .filter((item) => !agent || item.agents === "both" || item.agents === agent)
    .sort((a, b) => a.name.localeCompare(b.name));
}

/// The automatic ones, for a new session, task or automation.
export const autoSkills = (agent, projectId) =>
  skillsFor(agent, projectId)
    .filter((item) => item.auto && (item.roles ?? ["build"]).includes("build"))
    .map((item) => item.id);

// ------------------------------------------------------------- settings --

function skillRow(skill) {
  const tags = [
    `<span class="badge badge--muted skill-tag">${escape(scopeText(skill))}</span>`,
    `<span class="badge badge--muted skill-tag">${escape(agentsText(skill.agents))}</span>`,
    skill.auto ? `<span class="badge skill-tag skill-tag--auto">${escape(t("Automatic"))}</span>` : "",
    ...(skill.roles ?? [])
      .filter((role) => role !== "build")
      .map((role) => `<span class="badge badge--muted skill-tag">${escape(t(ROLES.find(([key]) => key === role)?.[1] ?? role))}</span>`),
  ].join("");
  const detail = skill.description || String(skill.body ?? "").split("\n").find((line) => line.trim()) || "";
  return `
    <div class="pref-row skill-row" data-skill-row="${escape(skill.id)}">
      <div class="pref-row__text">
        <div class="pref-row__title skill-row__title">${escape(skill.name)}<span class="skill-row__tags">${tags}</span></div>
        <div class="pref-row__detail skill-row__detail">${escape(detail)}</div>
      </div>
      <div class="pref-row__control">
        ${button({ label: t("Edit"), data: { skill: "edit", "skill-id": skill.id } })}
        ${button({ icon: "trash", kind: "quiet", title: t("Delete"), data: { skill: "delete", "skill-id": skill.id } })}
      </div>
    </div>`;
}

function templates() {
  const taken = new Set(all().map((item) => item.name.toLowerCase()));
  const offered = TEMPLATES.filter((item) => !taken.has(t(item.name).toLowerCase()));
  if (!offered.length) return "";
  return `
    <div class="skill-templates">
      <div class="skill-templates__label">${escape(t("Start from a template"))}</div>
      <div class="skill-templates__list">${offered
        .map(
          (item) => `
        <button class="skill-template" data-skill="template" data-skill-template="${item.key}">
          <span class="skill-template__name">${icons.plus}${escape(t(item.name))}</span>
          <span class="skill-template__text">${escape(t(item.description))}</span>
        </button>`,
        )
        .join("")}</div>
    </div>`;
}

export function skillsSection() {
  if (!state.skillsLoaded) return `<div class="pref-empty">${escape(t("Reading…"))}</div>`;
  const list = [...all()].sort((a, b) => a.name.localeCompare(b.name));
  const create = button({ label: t("New skill"), icon: "plus", data: { skill: "new" }, kind: "primary" });
  if (!list.length) {
    return `
      <div class="skill-empty">
        <span class="skill-empty__mark">${icons.book}</span>
        <h2 class="skill-empty__title">${escape(t("No skills yet"))}</h2>
        <p class="skill-empty__text">${escape(t("A skill is a short set of instructions you write once — how to commit, when to test — and attach to sessions, tasks and automations. Convoy gives it to the agent when the session starts."))}</p>
        <div class="centre-action">${create}</div>
      </div>
      ${templates()}`;
  }
  return `
    <section class="pref-group">
      <div class="skill-head">
        <h3 class="pref-group__title">${escape(t("Library"))} <span class="section__count">${list.length}</span></h3>
        <span class="section__spacer"></span>
        ${create}
      </div>
      <div class="pref-group__box">${list.map(skillRow).join("")}</div>
      <p class="pref-group__footer">${escape(t("Attached skills are given to the agent as instructions when a session starts. Automatic ones are attached to new sessions, tasks and automations in their scope."))}</p>
    </section>
    ${templates()}`;
}

/// Project settings: the project's own skills, and the way to the library.
export function projectSkills(projectId) {
  const own = all().filter((item) => item.project_id === projectId);
  const global = all().filter((item) => !item.project_id).length;
  const rows = own.length
    ? own.map(skillRow).join("")
    : `<div class="pref-empty">${escape(t("No skills of this project's own yet."))}</div>`;
  return `
    <section class="pref-group">
      <h3 class="pref-group__title">${escape(t("Skills"))}</h3>
      <div class="pref-group__box">${rows}
        <div class="pref-inline">
          ${button({ label: t("New skill for this project"), icon: "plus", data: { skill: "new", "skill-project": projectId } })}
          ${button({ label: t("Manage skills…"), kind: "quiet", data: { skill: "manage" } })}
        </div>
      </div>
      <p class="pref-group__footer">${escape(t("Also here, from the ones for every project: {skills}.", { skills: plural(global, "skill") }))}</p>
    </section>`;
}

// ---------------------------------------------------------------- editor --

export function skillEditor(dialog) {
  const projects = [["", t("Everywhere")], ...state.projects.map((item) => [item.id, item.title])];
  const body = dialog.preview
    ? `<div class="skill-preview preview__markdown">${dialog.body.trim() ? markdown(dialog.body) : `<p class="skill-preview__none">${escape(t("Nothing to preview yet."))}</p>`}</div>`
    : `<textarea id="skill-body" class="skill-body" spellcheck="false" rows="12"
                 placeholder="${escape(t("What the agent should do, in Markdown. It reads this at the start of every session it is attached to."))}">${escape(dialog.body)}</textarea>`;
  return modal({
    title: dialog.id ? t("Edit skill") : t("New skill"),
    wide: true,
    body: `
      <div class="field-row">
        ${field(t("Name"), textInput("skill-name", dialog.name, t("Small focused commits")))}
        ${field(t("Description (optional)"), textInput("skill-description", dialog.description, t("One line, shown in the lists")))}
      </div>
      <div class="field">
        <div class="skill-body__head">
          <span class="field__label">${escape(t("Instructions"))}</span>
          <div class="segmented segmented--small" role="group" aria-label="${escape(t("Show"))}">
            <button data-skill="write" aria-pressed="${!dialog.preview}">${escape(t("Write"))}</button>
            <button data-skill="preview" aria-pressed="${!!dialog.preview}">${escape(t("Preview"))}</button>
          </div>
        </div>
        ${body}
        <span class="field__note">${escape(t("{count} of 16,000 characters", { count: dialog.body.length.toLocaleString() }))}</span>
      </div>
      <div class="field-row">
        <label class="field">
          <span class="field__label">${escape(t("Applies in"))}</span>
          <select id="skill-project" class="field__select">${projects
            .map(([value, label]) => `<option value="${escape(value)}"${value === (dialog.project_id ?? "") ? " selected" : ""}>${escape(label)}</option>`)
            .join("")}</select>
        </label>
        ${field(t("For"), choice("skill-agents", AGENT_SCOPES.map(([value, label]) => [value, t(label)]), dialog.agents))}
      </div>
      <div class="sheet-box">
        ${setting(
          t("Attach to new sessions automatically"),
          t("Preselected for new sessions, tasks and automations in its scope. You can still take it off each one."),
          toggle("skill-auto", dialog.auto, t("Attach to new sessions automatically")),
        )}
        ${setting(
          t("Roles in a task"),
          t("Which agents of a task get it: the builder, and the planner, reviewer, QA or documentation writer of its project."),
          `<div class="segmented">${ROLES.map(
            ([role, name]) =>
              `<button data-toggle="skill-role:${role}" aria-pressed="${(dialog.roles ?? ["build"]).includes(role)}">${escape(t(name))}</button>`,
          ).join("")}</div>`,
        )}
      </div>`,
    note: escape(t("A change applies to each session the next time it starts.")),
    foot: `
      ${button({ label: t("Cancel"), data: { dismiss: "1" } })}
      ${button({ label: dialog.busy ? t("Saving…") : dialog.id ? t("Save") : t("Create skill"), data: { skill: "save" }, kind: "primary", disabled: !!dialog.busy })}`,
  });
}

// ---------------------------------------------------------------- picker --

/// Chips to pick a new session's, task's or automation's skills. `picked`
/// is the ids chosen; only skills for the agent and project are offered.
export function skillPicker(picked, agent, projectId) {
  const offered = skillsFor(agent, projectId);
  const chosen = new Set(picked ?? []);
  const chips = offered.length
    ? offered
        .map(
          (item) => `
        <button type="button" class="skill-chip" data-skill-pick="${escape(item.id)}" aria-pressed="${chosen.has(item.id)}"
                title="${escape(item.description || item.name)}">
          <span class="skill-chip__check">${icons.check}</span>${escape(item.name)}
        </button>`,
        )
        .join("")
    : `<span class="skill-picker__none">${escape(t("No skills for this agent yet."))}</span>`;
  return `
    <div class="field skill-picker">
      <span class="field__label">${escape(t("Skills"))}</span>
      <div class="skill-picker__chips">${chips}
        <button type="button" class="skill-picker__manage" data-skill="manage">${escape(t("Manage…"))}</button>
      </div>
    </div>`;
}

// -------------------------------------------------------- session chip --

/// "2 skills", for a session's composer or terminal header.
export function skillsChip(id, where = "bar") {
  const item = state.sessions.find((entry) => entry.id === id) ?? state.allSessions.find((entry) => entry.id === id);
  if (!item) return "";
  const count = (item.skills ?? []).filter((skill) => findSkill(skill)).length;
  const label = plural(count, "skill");
  return `<button class="skills-chip skills-chip--${where}${count ? "" : " skills-chip--none"}" data-action="skills-menu" data-skills-session="${escape(id)}"
                  aria-haspopup="menu" title="${escape(t("Skills for this session"))}">${icons.book}<span>${escape(label)}</span></button>`;
}

/// The menu it opens: each skill that suits the session, ticked when on.
export function skillsMenu(id, anchor) {
  const item = state.sessions.find((entry) => entry.id === id) ?? state.allSessions.find((entry) => entry.id === id);
  if (!item) return "";
  const on = new Set(item.skills ?? []);
  const offered = skillsFor(item.agent, item.project_id);
  const rows = offered.length
    ? offered
        .map(
          (skill) => `
        <button class="menu__item skills-menu__item" role="menuitemcheckbox" aria-checked="${on.has(skill.id)}"
                data-skill-toggle="${escape(skill.id)}" data-skill-session="${escape(id)}">
          <span class="skills-menu__check">${on.has(skill.id) ? icons.check : ""}</span>
          <span class="skills-menu__title">${escape(skill.name)}</span>
          ${skill.auto ? `<span class="menu__hint">${escape(t("automatic"))}</span>` : ""}
        </button>`,
        )
        .join("")
    : `<div class="skills-menu__none">${escape(t("No skills for this agent yet."))}</div>`;
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--anchored skills-menu" role="menu" style="${anchor}">
        <div class="helpers-menu__label">${escape(t("Skills"))}</div>
        ${rows}
        <div class="skills-menu__note">${escape(item.running ? t("Changes apply when the session next starts.") : t("Applies when the session next starts."))}</div>
        <div class="menu__divider"></div>
        <button class="menu__item" data-skill="manage">${escape(t("Manage skills…"))}</button>
      </div>
    </div>`;
}
