// The settings page. Like the macOS app it replaces the window rather than
// floating over it: a searchable list of sections on the left, one page per
// section, and every change saved the moment it is made.

import { agentIcon, icons } from "./icons.js";
import { button, escape, plural, shorten } from "./ui.js";
import { state } from "./state.js";
import { SHORTCUTS, SHORTCUT_GROUPS, label } from "./shortcuts.js";

/// The macOS app's sections, in its order.
export const SECTIONS = [
  ["Setup", "check", ["check", "diagnostics", "doctor", "missing", "install", "path", "login", "gh", "git", "node", "problem"]],
  ["General", "sliders", ["keep awake", "sleep", "sidebar", "sort", "worktree default", "branch prefix", "new sessions"]],
  ["Appearance", "paint", ["theme", "dark", "light", "system"]],
  ["Terminal", "terminal", ["font", "size", "scrollback", "lines"]],
  ["Agents", "sparkles", ["claude", "codex", "default agent", "yolo", "permissions", "skip", "review", "template", "brief", "hibernate", "idle", "sleep"]],
  ["Accounts", "person", ["account", "profile", "login", "sign in", "claude", "codex"]],
  ["Linear & Jira", "link", ["linear", "jira", "atlassian", "issues", "import", "api key", "token", "mcp", "integrations"]],
  ["Quick Commands", "bolt", ["prompt", "command", "snippet", "quick"]],
  ["Git", "git", ["branch", "worktree", "setup", "shared", "location"]],
  ["Notifications", "bell", ["notify", "sound", "waiting", "finished", "done", "focus"]],
  ["AI Limits", "limits", ["usage", "limits", "codex", "claude", "quota", "status line"]],
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

const text = (key, value, placeholder = "") =>
  `<input class="pref-text" data-pref-text="${key}" value="${escape(value ?? "")}"
          placeholder="${escape(placeholder)}" spellcheck="false" />`;

const select = (key, options, selected) =>
  `<select class="pref-select" data-pref-select="${key}">
     ${options.map(([value, name]) => `<option value="${escape(value)}"${value === selected ? " selected" : ""}>${escape(name)}</option>`).join("")}
   </select>`;

const area = (key, value, placeholder, rows = 4) =>
  `<div class="pref-block">
     <textarea class="pref-area" data-pref-text="${key}" rows="${rows}" spellcheck="false"
               placeholder="${escape(placeholder)}">${escape(value ?? "")}</textarea>
   </div>`;

const empty = (text) => `<div class="pref-empty">${escape(text)}</div>`;

// ---------------------------------------------------------------- sections

function setup() {
  const checks = state.diagnostics;
  const rows = !checks
    ? empty("Checking…")
    : checks
        .map((check) =>
          row(
            check.name,
            check.found
              ? [check.version, check.path].filter(Boolean).join(" · ")
              : `${check.purpose} Not found. ${check.install}`,
            check.found
              ? `<span class="pref-ok">${icons.check} Found</span>`
              : `<span class="pref-missing">${icons.warn} Missing</span>`,
          ),
        )
        .join("");
  return `
    ${groupBox(
      "Tools",
      rows,
      "Looked up through your login shell, so the PATH matches what an agent session sees. Sign in to Claude Code and Codex in their own terminals.",
    )}
    <div class="pref-actions">${button({ label: "Check again", icon: "refresh", action: "run-diagnostics" })}</div>`;
}

function general(settings) {
  return `
    ${groupBox(
      "Sidebar",
      row("Sort projects by name", "Otherwise projects keep the order they were added in.", toggle("sort_projects", settings.sort_projects, "Sort projects by name")) +
        row("Compact rows", "Hides the branch and status line under projects and sessions.", toggle("compact_sidebar", settings.compact_sidebar !== false, "Compact rows")),
    )}
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
      "New sessions",
      row(
        "Run new sessions in a git worktree by default",
        "Each new session in a repository gets its own checkout on a new branch.",
        toggle("worktree_by_default", settings.worktree_by_default, "Worktree by default"),
      ) +
        row(
          "Branch prefix",
          "Prepended to generated branch names, e.g. feature → feature/fix-delivery-status.",
          text("branch_prefix", settings.branch_prefix, "none"),
        ),
      "Worktrees live beside the workspace file, never inside your repository.",
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
      "Default agent",
      row(
        "Agent",
        "Preselected for a new session and a new task. Reviews default to the other agent.",
        segmented(
          "default_agent",
          AGENTS.map(([value, name]) => [value, name, agentIcon(value, 13)]),
          settings.default_agent,
        ),
      ),
    )}
    ${groupBox(
      "Permissions",
      row("Claude Code: skip permission prompts (Yolo)", "Launches with --dangerously-skip-permissions.", toggle("yolo_claude", settings.yolo_claude, "Claude Yolo")) +
        row("Codex: bypass approvals and sandbox (Yolo)", "Launches with --dangerously-bypass-approvals-and-sandbox.", toggle("yolo_codex", settings.yolo_codex, "Codex Yolo")) +
        row(
          "Trust project folders automatically",
          "Marks the session folder, new worktrees included, as trusted in Claude's .claude.json and Codex's config.toml before launch, so agents never stop on the trust prompt.",
          toggle("auto_trust", settings.auto_trust !== false, "Trust folders"),
        ),
      "Yolo lets agents edit files and run commands without asking. Applies to sessions launched afterwards.",
    )}
    ${groupBox(
      "Review brief template",
      `<div class="pref-block">
         <textarea class="pref-area" data-pref-text="review_template" rows="6" spellcheck="false"
                   placeholder="Leave empty for the built-in brief">${escape(settings.review_template ?? "")}</textarea>
       </div>`,
      "Used by Start review and by automatic task reviews, unless the project sets its own in Project settings.",
    )}
    ${groupBox(
      "Hibernation",
      row(
        "Sleep finished Claude agents after",
        "Minutes after it reports a finished turn and sits idle. 0 keeps it running.",
        number("hibernate_minutes", settings.hibernate_minutes, 0, 1440, 5),
      ),
      "Frees memory; opening a sleeping session resumes the same conversation.",
    )}
    ${groupBox(
      "Claude Code",
      row(
        "Agent status hooks",
        "Shows working, waiting and done in the sidebar and tabs, and powers notifications and hibernation.",
        toggle("status_hooks", settings.status_hooks !== false, "Status hooks"),
      ),
      "Hooks are passed per session with --settings, so your global ~/.claude/settings.json is never modified.",
    )}
    ${groupBox(
      "Codex",
      row("Status detection", "", '<span class="pref-muted">Process state only</span>'),
      "Codex has no hook API, so its sessions show running or stopped only.",
    )}`;
}

function git(settings) {
  return `
    ${groupBox(
      "Branch & changes",
      row("Show branch and changed-file counts", "In the session bar, and under projects when rows are not compact.", toggle("git_status", settings.git_status !== false, "Git status")) +
        row("Refresh every", "Seconds; 0 refreshes only when you open a session or ask.", number("git_poll_seconds", settings.git_poll_seconds ?? 10, 0, 600, 5)),
      "Read with optional locks disabled, so polling never races an agent's own git commands.",
    )}
    ${groupBox(
      "Worktree setup hooks",
      area("worktree_setup", settings.worktree_setup, "pnpm install", 3),
      "Runs once in every new worktree before the agent starts, after you confirm it. A project's own command runs after this one.",
    )}
    ${groupBox(
      "Worktree shared paths",
      area("worktree_shared", settings.worktree_shared, "One path per line, e.g. .env", 3),
      "Gitignored paths brought into every new worktree from the primary checkout, one per line. Existing files are never replaced.",
    )}
    ${groupBox(
      "Worktrees",
      row(
        "Location",
        state.worktrees ?? "",
        button({ label: "Show", icon: "folder", action: "reveal-worktrees" }),
      ),
      "Convoy only removes worktrees it created itself, and only when they are clean.",
    )}`;
}

/// Default and None everywhere; macOS also names its system sounds.
const SOUNDS = () => [
  ["default", "Default"],
  ["none", "None"],
  ...(/mac/i.test(navigator.platform)
    ? ["Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"].map((name) => [name, name])
    : []),
];

function notifications(settings) {
  const off = !settings.notifications;
  const muted = (control) => (off ? control.replace("<button", "<button disabled") : control);
  return groupBox(
    "Agent notifications",
    row("Enable notifications", "", toggle("notifications", settings.notifications, "Enable notifications")) +
      row("When an agent needs input or permission", "", muted(toggle("notify_waiting", settings.notify_waiting !== false, "Waiting"))) +
      row("When an agent finishes", "", muted(toggle("notify_done", settings.notify_done !== false, "Done"))) +
      row("Also notify while Convoy is in front", "The session you are looking at always stays quiet.", muted(toggle("notify_when_focused", settings.notify_when_focused, "When focused"))) +
      row("Sound", "", off ? select("notification_sound", SOUNDS(), settings.notification_sound ?? "default").replace("<select", "<select disabled") : select("notification_sound", SOUNDS(), settings.notification_sound ?? "default")),
    "Your system may ask for permission the first time.",
  );
}

function limits(settings) {
  const reading = state.limits ?? {};
  const summary = (entry) =>
    entry?.windows?.length
      ? entry.windows.map((window) => `${Math.round(window.percent)}%`).join(" · ")
      : entry?.note ?? "Not read yet";
  return `
    ${groupBox(
      "Claude",
      row(
        "Claude limits integration",
        "Adds a usage status line to new Claude sessions so limits appear in AI Limits.",
        toggle("claude_usage", settings.claude_usage, "Claude limits integration"),
      ) + row("Current", summary(reading.claude), ""),
      "Shows after the next response in a Claude session started from Convoy. Needs a recent Claude Code and an eligible subscription.",
    )}
    ${groupBox(
      "Codex",
      row("Current", summary(reading.codex), button({ label: reading.loading ? "Reading…" : "Refresh", action: "limits-refresh", disabled: !!reading.loading })),
      "Read from your installed Codex login. Convoy never copies account tokens.",
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
  return (
    SHORTCUT_GROUPS.map((group) =>
      groupBox(
        group,
        SHORTCUTS.filter(([, , , owner]) => owner === group)
          .map(([action, , what]) =>
            row(
              what,
              "",
              `<button class="button shortcut${state.capturing === action ? " shortcut--listening" : ""}"
                       data-capture="${action}">
                 ${state.capturing === action ? "Press a key…" : escape(label(settings.shortcuts?.[action] || fallback(action)))}
               </button>`,
            ),
          )
          .join(""),
      ),
    ).join("") +
    `<p class="pref-group__footer">Click a shortcut, then press the new keys. Escape cancels. ${button({ label: "Restore defaults", action: "reset-shortcuts", kind: "quiet" })}</p>`
  );
}

const PAGES = {
  Setup: setup,
  General: general,
  Git: git,
  Notifications: notifications,
  "AI Limits": limits,
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
