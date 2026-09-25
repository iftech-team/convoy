// The settings page. Like the macOS app it replaces the window rather than
// floating over it: a searchable list of sections on the left, one page per
// section, and every change saved the moment it is made.

import { agentIcon, icons } from "./icons.js";
import { button, escape, plural, shorten } from "./ui.js";
import { state } from "./state.js";
import { SHORTCUTS, label } from "./shortcuts.js";

export const SECTIONS = [
  ["General", "sliders", ["keep awake", "sleep", "notify", "notifications", "finished", "waiting"]],
  ["Appearance", "paint", ["theme", "dark", "light", "system"]],
  ["Terminal", "terminal", ["font", "size", "scrollback", "lines"]],
  ["Agents", "sparkles", ["claude", "codex", "default agent", "status line", "usage", "hibernate", "idle", "stop"]],
  ["Accounts", "person", ["account", "profile", "login", "sign in", "claude", "codex"]],
  ["Linear & Jira", "link", ["linear", "jira", "atlassian", "issues", "import", "api key", "token", "mcp", "integrations"]],
  ["Quick Commands", "bolt", ["prompt", "command", "snippet", "quick"]],
  ["Shortcuts", "keyboard", ["keyboard", "keys", "bindings", "palette"]],
];

const AGENTS = [
  ["claude", "Claude Code"],
  ["codex", "Codex"],
];

const visibleSections = () => {
  const query = state.settingsQuery.trim().toLowerCase();
  if (!query) return SECTIONS;
  return SECTIONS.filter(
    ([name, , words]) => name.toLowerCase().includes(query) || words.some((word) => word.includes(query)),
  );
};

// ------------------------------------------------------------ building blocks

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

const segmented = (key, options, selected) => `
  <div class="segmented">
    ${options
      .map(
        ([value, text, mark = ""]) =>
          `<button data-pref-set="${key}" data-value="${escape(value)}"
                   aria-pressed="${value === selected}">${mark}${escape(text)}</button>`,
      )
      .join("")}
  </div>`;

const toggle = (key, on, text) =>
  `<button class="switch" data-pref-toggle="${escape(key)}" aria-pressed="${!!on}"
           aria-label="${escape(text)}"></button>`;

const number = (key, value, min, max, step = 1) =>
  `<input class="pref-number" type="number" data-pref-number="${key}"
          min="${min}" max="${max}" step="${step}" value="${escape(value)}" />`;

const empty = (text) => `<div class="pref-empty">${escape(text)}</div>`;

// ---------------------------------------------------------------- sections

function general(settings) {
  return `
    ${groupBox(
      "Keep computer awake",
      row(
        "Mode",
        "Prevents idle sleep, not closing the lid.",
        segmented(
          "keep_awake",
          [
            ["always", "Always"],
            ["sessions", "While running"],
            ["off", "Off"],
          ],
          settings.keep_awake,
        ),
      ),
      "A running session may be waiting for input; keeping the computer awake lets it finish.",
    )}
    ${groupBox(
      "Notifications",
      row(
        "Notify when an agent finishes or needs input",
        "Only while the window is not focused.",
        toggle("notifications", settings.notifications, "Notifications"),
      ),
    )}`;
}

function appearance(settings) {
  return groupBox(
    "Theme",
    row(
      "Appearance",
      "System follows the desktop.",
      segmented(
        "theme",
        [
          ["system", "System"],
          ["light", "Light"],
          ["dark", "Dark"],
        ],
        settings.theme,
      ),
    ),
  );
}

function terminalSection(settings) {
  return groupBox(
    "Terminal",
    row("Font size", "10 to 24 points.", number("font_size", settings.font_size, 10, 24)) +
      row("Scrollback", "Lines kept per terminal, 1,000 to 50,000.", number("scrollback", settings.scrollback, 1000, 50000, 1000)),
    "Applies to open terminals immediately.",
  );
}

function agents(settings) {
  return `
    ${groupBox(
      "New sessions",
      row(
        "Default agent",
        "Preselected for a new session and a new task.",
        segmented(
          "default_agent",
          AGENTS.map(([value, text]) => [value, text, agentIcon(value, 13)]),
          settings.default_agent,
        ),
      ),
    )}
    ${groupBox(
      "Claude Code",
      row(
        "Usage status line",
        "Shows subscription limits in Claude's status line. Replaces that launch's own status line; takes effect next launch.",
        toggle("claude_usage", settings.claude_usage, "Usage status line"),
      ) +
        row(
          "Stop an idle session after",
          "Minutes after it reports a finished turn. 0 keeps it running.",
          number("hibernate_minutes", settings.hibernate_minutes, 0, 1440, 5),
        ),
      "A stopped session keeps its conversation and resumes exactly where it was.",
    )}`;
}

function accounts() {
  const profiles = state.profiles ?? [];
  const rows = profiles.length
    ? profiles
        .map((profile) =>
          row(
            profile.label,
            `${profile.agent === "claude" ? "Claude Code" : "Codex"} · ${
              profile.sessions ? plural(profile.sessions, "session") : "not in use"
            }`,
            `${agentIcon(profile.agent, 14)}
             ${button({
               icon: "trash",
               kind: "quiet",
               data: { "remove-profile": profile.id },
               disabled: profile.sessions > 0,
               title: profile.sessions ? "Bound to saved sessions" : "Remove this account",
             })}`,
          ),
        )
        .join("")
    : empty("No accounts yet. Without one, a session uses the provider's own login.");
  return `
    ${groupBox(
      "Accounts",
      rows,
      "Each account gets its own provider home, so sign-ins stay separate. Removing one keeps its credential files.",
    )}
    ${groupBox(
      "Add account",
      `<div class="pref-inline">
         ${segmented(
           "profile_agent",
           AGENTS.map(([value, text]) => [value, text, agentIcon(value, 13)]),
           state.profileAgent,
         )}
         <input id="pref-profile-label" placeholder="Label, e.g. work" spellcheck="false" />
         ${button({ label: "Add", action: "page-add-profile", kind: "primary" })}
       </div>`,
      "Sign in afterwards through the provider's own login inside a session's terminal.",
    )}`;
}

function integrations() {
  const connections = state.integrations ?? [];
  const auth = (item) =>
    item.auth === "mcp" ? "Agent MCP server" : item.auth === "password" ? "Login & password" : item.kind === "jira" ? "API token" : "API key";
  const rows = connections.length
    ? connections
        .map((item) =>
          row(
            item.name,
            [item.kind === "jira" ? "Jira" : "Linear", auth(item), item.site, item.username].filter(Boolean).join(" · "),
            `${button({ label: "Edit", data: { "conn-edit": item.id } })}
             ${button({ icon: "trash", kind: "quiet", data: { "conn-remove": item.id }, title: "Remove" })}`,
          ),
        )
        .join("")
    : empty("No connections yet.");
  return `
    ${groupBox(
      "Connections",
      rows,
      "Import issues as tasks from a project's Tasks tab. MCP connections store nothing: the agent fetches each issue with its own MCP server.",
    )}
    <div class="pref-actions">
      ${button({ label: "Add Linear", icon: "plus", data: { "conn-add": "linear" } })}
      ${button({ label: "Add Jira", icon: "plus", data: { "conn-add": "jira" } })}
    </div>`;
}

function quickCommands() {
  const commands = state.quickCommands ?? [];
  const rows = commands.length
    ? commands
        .map((command) =>
          row(
            command.title,
            `${shorten(command.text, 90)}${command.submit ? " · sends Enter" : ""}`,
            button({ icon: "trash", kind: "quiet", data: { "remove-command": command.id }, title: "Remove" }),
          ),
        )
        .join("")
    : empty("None yet.");
  return `
    ${groupBox(
      "Commands",
      rows,
      "Saved text you can send into a running agent from the ⚡ menu. Global ones appear in every project.",
    )}
    <div class="pref-actions">${button({ label: "Add command…", icon: "plus", action: "manage-commands" })}</div>`;
}

function shortcuts(settings) {
  const fallback = (action) => SHORTCUTS.find(([name]) => name === action)?.[1] ?? "";
  return groupBox(
    "Keyboard",
    SHORTCUTS.map(([action, , what]) =>
      row(
        what,
        "",
        `<button class="button shortcut${state.capturing === action ? " shortcut--listening" : ""}"
                 data-capture="${action}">
           ${state.capturing === action ? "Press a key…" : escape(label(settings.shortcuts?.[action] || fallback(action)))}
         </button>`,
      ),
    ).join(""),
    "Click a shortcut, then press the new keys. Escape cancels.",
  );
}

const PAGES = {
  General: general,
  Appearance: appearance,
  Terminal: terminalSection,
  Agents: agents,
  Accounts: accounts,
  "Linear & Jira": integrations,
  "Quick Commands": quickCommands,
  Shortcuts: shortcuts,
};

export function settingsPage() {
  const sections = visibleSections();
  const current = PAGES[state.settingsSection] ? state.settingsSection : "General";
  const version = state.version ? `Convoy ${escape(state.version)}` : "Convoy";
  return `
    <div class="prefs">
      <aside class="prefs__nav">
        <button class="prefs__back" data-action="close-settings">${icons.back} Back to app</button>
        <div class="sidebar__search prefs__search">
          ${icons.search}
          <input id="settings-search" type="search" placeholder="Search settings"
                 value="${escape(state.settingsQuery)}" spellcheck="false" />
        </div>
        <nav class="prefs__list">
          ${
            sections.length
              ? sections
                  .map(
                    ([name, icon]) => `
            <button class="prefs__item" data-settings-section="${escape(name)}"
                    aria-current="${name === current}">
              ${icons[icon] ?? ""}<span>${escape(name)}</span>
            </button>`,
                  )
                  .join("")
              : '<div class="sidebar__none">No matching settings</div>'
          }
        </nav>
        <div class="prefs__foot" title="${escape(state.storage)}">${version}<br />${escape(shorten(state.storage, 34))}</div>
      </aside>
      <div class="prefs__page" data-scroll="settings">
        <div class="prefs__content">
          <h1 class="prefs__title">${escape(current)}</h1>
          ${PAGES[current](state.settings)}
        </div>
      </div>
    </div>`;
}
