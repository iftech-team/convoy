// Project settings, laid out as the macOS app's page: identity (name,
// colour, icon), agent, Git & worktrees, review & tasks, quick commands and
// removal. Each change is saved as it is made.

import { SYMBOLS, agentIcon, icon, projectIcon } from "./icons.js";
import { icons } from "./icons.js";
import { button, escape, shorten } from "./ui.js";
import { state } from "./state.js";
import { t } from "./i18n.js";
import { projectSkills } from "./views-skills.js";

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
  }
  return groupBox(
    t("Integration"),
    rows.join(""),
    t("Import tasks searches only here, and opens on this connection. Each project keeps its own link."),
  );
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

export function projectSettingsPage() {
  const project = state.projectDraft;
  if (!project) return "";
  const global = state.settings.default_agent === "codex" ? "Codex" : "Claude Code";
  const commands = (state.quickCommands ?? []).filter((command) => command.project_id === project.id);
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
        ${identity(project)}
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
              ],
              project.default_agent ?? "",
            ),
          ),
          t("Preselected for new sessions and tasks in this project. Global default: {agent}.", { agent: global }),
        )}
        ${groupBox(
          t("Git & worktrees"),
          `${row(t("Base ref for worktrees"), t("Empty starts from the current HEAD."), text("base_ref", project.base_ref, "main"))}
           ${row(t("Branch prefix"), t("Global: {prefix}", { prefix: state.settings.branch_prefix || t("none") }), text("branch_prefix", project.branch_prefix, "feature"))}
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
          t("Shared paths are gitignored files copied into each new worktree; existing files are never replaced. Setup commands run once, after you confirm, following the global ones in Settings → Git."),
        )}
        ${groupBox(
          t("Review & tasks"),
          `${row(
            t("Tasks finish with"),
            "",
            select(
              "task_mode",
              [
                ["none", t("Commit only")],
                ["pr", t("Pull request")],
                ["push", t("Push")],
              ],
              project.task_mode || "none",
            ),
          )}
           ${row(
             t("Auto-run task queue"),
             t("Start the next queued task when one finishes."),
             `<button class="switch" data-proj-toggle="auto_run_tasks" aria-pressed="${!!project.auto_run_tasks}"
                      aria-label="${escape(t("Auto-run task queue"))}"></button>`,
           )}
           <div class="pref-block pref-stack">
             <div class="pref-row__title">${escape(t("Review brief template"))}</div>
             <textarea class="pref-area" data-proj-text="review_template" rows="6" spellcheck="false"
                       placeholder="${escape(t("Empty uses the global template from Settings → Agents"))}">${escape(project.review_template ?? "")}</textarea>
             <div class="pref-inline pref-inline--flush">
               ${button({ label: t("Insert default"), action: "insert-review-default", kind: "quiet" })}
               ${button({ label: t("Use global"), data: { "proj-clear": "review_template" }, disabled: !project.review_template })}
             </div>
           </div>`,
        )}
        ${integration(project)}
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
          t("Sent to a terminal from the ⚡ menu. Global commands are managed in Settings → Quick Commands."),
        )}
        ${groupBox(
          t("Folder"),
          row(
            project.path,
            "",
            `${button({ label: t("Show"), icon: "folder", action: "reveal-project" })}
             ${button({ label: t("Reconnect…"), icon: "link", action: "reconnect-project" })}`,
          ),
        )}
        ${groupBox(
          t("Remove"),
          row(
            t("Remove project from Convoy"),
            "",
            button({ label: t("Remove…"), kind: "danger", action: "remove-project", disabled: project.running > 0 }),
          ),
          t("Stop this project's running sessions first. Files, worktrees and the agents' own history stay on disk."),
        )}
      </div>
    </div>`;
}

export { agentIcon };
