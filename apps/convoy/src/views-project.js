// Project settings, laid out as the macOS app's page: identity (name,
// colour, icon), agent, Git & worktrees, review & tasks, quick commands and
// removal. Each change is saved as it is made.

import { SYMBOLS, agentIcon, icon, projectIcon } from "./icons.js";
import { icons } from "./icons.js";
import { button, escape, shorten } from "./ui.js";
import { state } from "./state.js";

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
               ${button({ label: "Use GitHub avatar", icon: "person", data: { "icon-source": "github" } })}
               ${button({ label: "Upload PNG…", icon: "download", data: { "icon-source": "upload" } })}
             </div>
             <div class="pref-inline pref-inline--flush">
               <input id="proj-favicon" class="pref-text pref-text--wide" placeholder="example.com" spellcheck="false" />
               ${button({ label: "Favicon", icon: "link", data: { "icon-source": "favicon" } })}
             </div>
             ${state.iconBusy ? '<p class="pref-group__footer">Downloading…</p>' : ""}
           </div>`
        : `<div class="icon-grid">${EMOJIS.map((emoji) => tile(emoji, `<span class="icon-tile__emoji">${emoji}</span>`, emoji)).join("")}</div>`;

  return groupBox(
    "Identity",
    `${row("Display name", "", text("title", project.title, "Name", true))}
     ${row("Group", "Shown as a heading above related projects in the sidebar.", text("group", project.group, "none"))}
     <div class="pref-block pref-stack">
       <div class="pref-row__title">Color</div>
       <div class="swatches">
         ${COLORS.map(
           (hex) => `<button class="swatch" data-proj-color="${hex}" style="background:${hex}"
                             aria-pressed="${project.color?.toUpperCase() === hex}" title="${hex}"></button>`,
         ).join("")}
         <input class="pref-text pref-text--hex" data-proj-text="color" value="${escape(project.color ?? "")}"
                placeholder="#5E6AD2" spellcheck="false" />
         ${button({ label: "Auto", data: { "proj-color": "" }, kind: "quiet", title: "Derive a colour from the name" })}
       </div>
       <div class="pref-row__title">Icon</div>
       <div class="segmented">
         ${[
           ["emoji", "Emoji"],
           ["symbol", "Icon"],
           ["image", "Image"],
         ]
           .map(([value, name]) => `<button data-icon-tab="${value}" aria-pressed="${tab === value}">${name}</button>`)
           .join("")}
       </div>
       ${picker}
       <div class="pref-inline pref-inline--flush">
         <span class="icon-preview">${projectIcon(project, 22)}</span>
         ${button({ label: "Reset to folder icon", data: { "proj-icon": "" }, disabled: !project.icon })}
       </div>
     </div>`,
    "Shown in the sidebar, tabs and menus so parallel projects stay distinguishable.",
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
        <button class="prefs__back prefs__back--inline" data-action="close-project-settings">${icons.back} Back</button>
        <div class="project-title">
          <span class="icon-preview">${projectIcon(project, 26)}</span>
          <div>
            <h1 class="prefs__title">${escape(project.title)}</h1>
            <div class="pref-row__detail">Project settings</div>
          </div>
        </div>
        ${identity(project)}
        ${groupBox(
          "Agent",
          row(
            "Default agent",
            "",
            select(
              "default_agent",
              [
                ["", `Use global (${global})`],
                ["claude", "Claude Code"],
                ["codex", "Codex"],
              ],
              project.default_agent ?? "",
            ),
          ),
          `Preselected for new sessions and tasks in this project. Global default: ${global}.`,
        )}
        ${groupBox(
          "Git & worktrees",
          `${row("Base ref for worktrees", "Empty starts from the current HEAD.", text("base_ref", project.base_ref, "main"))}
           ${row("Branch prefix", `Global: ${state.settings.branch_prefix || "none"}`, text("branch_prefix", project.branch_prefix, "feature"))}
           <div class="pref-block pref-stack">
             <div class="pref-row__title">Shared paths (one per line)</div>
             <textarea class="pref-area" data-proj-text="shared_paths" rows="3" spellcheck="false"
                       placeholder=".env">${escape(project.shared_paths ?? "")}</textarea>
             <div class="pref-row__detail">e.g. .env, node_modules, vendor</div>
             <div class="pref-row__title">Worktree setup commands</div>
             <textarea class="pref-area" data-proj-text="setup_command" rows="3" spellcheck="false"
                       placeholder="pnpm install">${escape(project.setup_command ?? "")}</textarea>
             <div class="pref-row__detail">e.g. pnpm install · composer install · direnv allow</div>
           </div>`,
          "Shared paths are gitignored files copied into each new worktree; existing files are never replaced. Setup commands run once, after you confirm, following the global ones in Settings → Git.",
        )}
        ${groupBox(
          "Review & tasks",
          `${row(
            "Tasks finish with",
            "",
            select(
              "task_mode",
              [
                ["none", "Commit only"],
                ["pr", "Pull request"],
                ["push", "Push"],
              ],
              project.task_mode || "none",
            ),
          )}
           ${row(
             "Auto-run task queue",
             "Start the next queued task when one finishes.",
             `<button class="switch" data-proj-toggle="auto_run_tasks" aria-pressed="${!!project.auto_run_tasks}"
                      aria-label="Auto-run task queue"></button>`,
           )}
           <div class="pref-block pref-stack">
             <div class="pref-row__title">Review brief template</div>
             <textarea class="pref-area" data-proj-text="review_template" rows="6" spellcheck="false"
                       placeholder="Empty uses the global template from Settings → Agents">${escape(project.review_template ?? "")}</textarea>
             <div class="pref-inline pref-inline--flush">
               ${button({ label: "Insert default", action: "insert-review-default", kind: "quiet" })}
               ${button({ label: "Use global", data: { "proj-clear": "review_template" }, disabled: !project.review_template })}
             </div>
           </div>`,
        )}
        ${groupBox(
          "Quick commands for this project",
          (commands.length
            ? commands
                .map((command) =>
                  row(
                    command.title,
                    shorten(command.text.replace(/\n/g, " "), 100),
                    button({ icon: "trash", kind: "quiet", data: { "remove-command": command.id }, title: "Remove" }),
                  ),
                )
                .join("")
            : '<div class="pref-empty">None yet.</div>') +
            `<div class="pref-inline">${button({ label: "Add", icon: "plus", action: "manage-commands" })}</div>`,
          "Sent to a terminal from the ⚡ menu. Global commands are managed in Settings → Quick Commands.",
        )}
        ${groupBox(
          "Folder",
          row(
            project.path,
            "",
            `${button({ label: "Show", icon: "folder", action: "reveal-project" })}
             ${button({ label: "Reconnect…", icon: "link", action: "reconnect-project" })}`,
          ),
        )}
        ${groupBox(
          "Remove",
          row(
            "Remove project from Convoy",
            "",
            button({ label: "Remove…", kind: "danger", action: "remove-project", disabled: project.running > 0 }),
          ),
          "Stop this project's running sessions first. Files, worktrees and the agents' own history stay on disk.",
        )}
      </div>
    </div>`;
}

export { agentIcon };
