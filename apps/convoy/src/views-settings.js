// The settings page. Like the macOS app it replaces the window rather than
// floating over it: a searchable list of sections on the left, one page per
// section, and every change saved the moment it is made.

import { agentIcon, icons } from "./icons.js";
import { button, escape, plural, shorten } from "./ui.js";
import { activeAccount, state } from "./state.js";
import { SHORTCUTS, SHORTCUT_GROUPS, label } from "./shortcuts.js";
import { LANGUAGES, languageChoice, t } from "./i18n.js";

/// The macOS app's sections, in its order.
export const SECTIONS = [
  ["Setup", "check", ["check", "diagnostics", "doctor", "missing", "install", "path", "login", "gh", "git", "node", "problem"]],
  ["General", "sliders", ["keep awake", "sleep", "sidebar", "sort", "worktree default", "branch prefix", "new sessions"]],
  ["Appearance", "paint", ["theme", "dark", "light", "system", "language", "russian", "english", "язык", "русский"]],
  ["Terminal", "terminal", ["font", "size", "scrollback", "lines"]],
  ["Agents", "sparkles", ["claude", "codex", "default agent", "chat", "terminal", "view", "yolo", "permissions", "skip", "review", "template", "brief", "hibernate", "idle", "sleep"]],
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
    ([name, , words]) =>
      name.toLowerCase().includes(query) ||
      t(name).toLowerCase().includes(query) ||
      words.some((word) => word.includes(query)),
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
    ? empty(t("Checking…"))
    : checks
        .map((check) =>
          row(
            check.name,
            check.found
              ? [check.version, check.path].filter(Boolean).join(" · ")
              : t("{purpose} Not found. {install}", { purpose: check.purpose, install: check.install }),
            check.found
              ? `<span class="pref-ok">${icons.check} ${escape(t("Found"))}</span>`
              : `<span class="pref-missing">${icons.warn} ${escape(t("Missing"))}</span>`,
          ),
        )
        .join("");
  return `
    ${groupBox(
      t("Tools"),
      rows,
      t("Looked up through your login shell, so the PATH matches what an agent session sees. Sign in to Claude Code and Codex in their own terminals."),
    )}
    ${
      state.setupChecks?.length
        ? groupBox(
            t("Checks"),
            state.setupChecks
              .map((check) =>
                row(
                  check.title,
                  check.status === "ok" ? check.detail : [check.detail, check.fix].filter(Boolean).join(" "),
                  check.status === "ok"
                    ? `<span class="pref-ok">${icons.check} ${escape(t("OK"))}</span>`
                    : `<span class="pref-missing${check.status === "failed" ? " pref-missing--failed" : ""}">${icons.warn} ${escape(check.status === "failed" ? t("Problem") : t("Look at"))}</span>`,
                ),
              )
              .join(""),
            t("Logins are checked for the accounts new sessions start with."),
          )
        : ""
    }
    <div class="pref-actions">${button({ label: t("Check again"), icon: "refresh", action: "run-diagnostics" })}</div>
    ${state.macImport ? groupBox(
      t("macOS app"),
      row(
        t("Import from the macOS app"),
        t("{projects} and {sessions} to bring across.", {
          projects: plural(state.macImport.projects + state.macImport.projects_existing, "project"),
          sessions: plural(state.macImport.sessions, "session"),
        }),
        button({ label: t("Import…"), action: "macos-import" }),
      ),
      t("Projects, sessions, tasks, quick commands and saved terminal output. Run it again later to pick up what changed there."),
    ) : ""}`;
}

function general(settings) {
  return `
    ${groupBox(
      t("Sidebar"),
      row(t("Sort projects by name"), t("Otherwise projects keep the order they were added in."), toggle("sort_projects", settings.sort_projects, t("Sort projects by name"))) +
        row(t("Compact rows"), t("Hides the branch and status line under projects and sessions."), toggle("compact_sidebar", settings.compact_sidebar !== false, t("Compact rows"))),
    )}
    ${groupBox(
      t("Keep computer awake"),
      row(
        t("Mode"),
        t("Prevents idle sleep, not closing the lid."),
        segmented(
          "keep_awake",
          [
            ["always", t("Always")],
            ["sessions", t("While running")],
            ["off", t("Off")],
          ],
          settings.keep_awake,
        ),
      ),
      t("A running session may be waiting for input; keeping the computer awake lets it finish."),
    )}
    ${groupBox(
      t("New sessions"),
      row(
        t("Run new sessions in a git worktree by default"),
        t("Each new session in a repository gets its own checkout on a new branch."),
        toggle("worktree_by_default", settings.worktree_by_default, t("Worktree by default")),
      ) +
        row(
          t("Branch prefix"),
          t("Prepended to generated branch names, e.g. feature → feature/fix-delivery-status."),
          text("branch_prefix", settings.branch_prefix, t("none")),
        ),
      t("Worktrees live beside the workspace file, never inside your repository."),
    )}`;
}

function appearance(settings) {
  return groupBox(
    t("Theme"),
    row(
      t("Appearance"),
      t("System follows the desktop."),
      segmented(
        "theme",
        [
          ["system", t("System")],
          ["light", t("Light")],
          ["dark", t("Dark")],
        ],
        settings.theme,
      ),
    ) +
      row(
        t("Language"),
        t("The window's language. System follows the desktop."),
        segmented("language", [["system", t("System")], ...LANGUAGES], languageChoice()),
      ),
  );
}

function terminalSection(settings) {
  return groupBox(
    t("Terminal"),
    row(t("Font size"), t("10 to 24 points."), number("font_size", settings.font_size, 10, 24)) +
      row(t("Scrollback"), t("Lines kept per terminal, 1,000 to 50,000."), number("scrollback", settings.scrollback, 1000, 50000, 1000)),
    t("Applies to open terminals immediately."),
  );
}

function agents(settings) {
  return `
    ${groupBox(
      t("Default agent"),
      row(
        t("Agent"),
        t("Preselected for a new session and a new task. Reviews default to the other agent."),
        segmented(
          "default_agent",
          AGENTS.map(([value, name]) => [value, name, agentIcon(value, 13)]),
          settings.default_agent,
        ),
      ) +
        row(
          t("New sessions open in"),
          t("Chat shows replies, tool calls, file changes and approvals as cards. Terminal runs the agent's own screen. Each session can switch later."),
          segmented(
            "default_view",
            [
              ["chat", t("Chat")],
              ["terminal", t("Terminal")],
            ],
            settings.default_view === "chat" ? "chat" : "terminal",
          ),
        ),
    )}
    ${groupBox(
      t("Permissions"),
      row(t("Claude Code: skip permission prompts (Yolo)"), t("Launches with --dangerously-skip-permissions."), toggle("yolo_claude", settings.yolo_claude, "Claude Yolo")) +
        row(t("Codex: bypass approvals and sandbox (Yolo)"), t("Launches with --dangerously-bypass-approvals-and-sandbox."), toggle("yolo_codex", settings.yolo_codex, "Codex Yolo")) +
        row(
          t("Trust project folders automatically"),
          t("Marks the session folder, new worktrees included, as trusted in Claude's .claude.json and Codex's config.toml before launch, so agents never stop on the trust prompt."),
          toggle("auto_trust", settings.auto_trust !== false, t("Trust folders")),
        ),
      t("Yolo lets agents edit files and run commands without asking. Applies to sessions launched afterwards."),
    )}
    ${groupBox(
      t("Review brief template"),
      `<div class="pref-block">
         <textarea class="pref-area" data-pref-text="review_template" rows="6" spellcheck="false"
                   placeholder="${escape(t("Leave empty for the built-in brief"))}">${escape(settings.review_template ?? "")}</textarea>
         ${button({ label: t("Insert default"), action: "insert-review-default", kind: "quiet" })}
       </div>`,
      t("Used by Start review and by automatic task reviews, unless the project sets its own in Project settings."),
    )}
    ${groupBox(
      t("Hibernation"),
      row(
        t("Sleep finished Claude agents after"),
        t("Minutes after it reports a finished turn and sits idle. 0 keeps it running."),
        number("hibernate_minutes", settings.hibernate_minutes, 0, 1440, 5),
      ),
      t("Frees memory; opening a sleeping session resumes the same conversation."),
    )}
    ${groupBox(
      "Claude Code",
      row(
        t("Agent status hooks"),
        t("Shows working, waiting and done in the sidebar and tabs, and powers notifications and hibernation."),
        toggle("status_hooks", settings.status_hooks !== false, t("Status hooks")),
      ),
      t("Hooks are passed per session with --settings, so your global ~/.claude/settings.json is never modified."),
    )}
    ${groupBox(
      "Codex",
      row(t("Status detection"), "", `<span class="pref-muted">${escape(t("Process state only"))}</span>`),
      t("Codex has no hook API, so its sessions show running or stopped only."),
    )}`;
}

function git(settings) {
  return `
    ${groupBox(
      t("Branch & changes"),
      row(t("Show branch and changed-file counts"), t("In the session bar, and under projects when rows are not compact."), toggle("git_status", settings.git_status !== false, t("Git status"))) +
        row(t("Refresh every"), t("Seconds; 0 refreshes only when you open a session or ask."), number("git_poll_seconds", settings.git_poll_seconds ?? 10, 0, 600, 5)),
      t("Read with optional locks disabled, so polling never races an agent's own git commands."),
    )}
    ${groupBox(
      t("Worktree setup hooks"),
      area("worktree_setup", settings.worktree_setup, "pnpm install", 3),
      t("Runs once in every new worktree before the agent starts, after you confirm it. A project's own command runs after this one."),
    )}
    ${groupBox(
      t("Worktree shared paths"),
      area("worktree_shared", settings.worktree_shared, t("One path per line, e.g. .env"), 3),
      t("Gitignored paths brought into every new worktree from the primary checkout, one per line. Existing files are never replaced."),
    )}
    ${groupBox(
      t("Worktrees"),
      row(
        t("Location"),
        state.worktrees ?? "",
        button({ label: t("Show"), icon: "folder", action: "reveal-worktrees" }),
      ),
      t("Convoy only removes worktrees it created itself, and only when they are clean."),
    )}`;
}

/// Default and None everywhere; macOS also names its system sounds.
const SOUNDS = () => [
  ["default", t("Default")],
  ["none", t("None")],
  ...(/mac/i.test(navigator.platform)
    ? ["Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink"].map((name) => [name, name])
    : []),
];

function notifications(settings) {
  const off = !settings.notifications;
  const muted = (control) => (off ? control.replace("<button", "<button disabled") : control);
  return groupBox(
    t("Agent notifications"),
    row(t("Enable notifications"), "", toggle("notifications", settings.notifications, t("Enable notifications"))) +
      row(t("When an agent needs input or permission"), "", muted(toggle("notify_waiting", settings.notify_waiting !== false, t("Waiting")))) +
      row(t("When an agent finishes"), "", muted(toggle("notify_done", settings.notify_done !== false, t("Done")))) +
      row(t("Also notify while Convoy is in front"), t("The session you are looking at always stays quiet."), muted(toggle("notify_when_focused", settings.notify_when_focused, t("When focused")))) +
      row(t("Sound"), "", off ? select("notification_sound", SOUNDS(), settings.notification_sound ?? "default").replace("<select", "<select disabled") : select("notification_sound", SOUNDS(), settings.notification_sound ?? "default")),
    t("Your system may ask for permission the first time."),
  );
}

function limits(settings) {
  const reading = state.limits ?? {};
  const summary = (entry) =>
    entry?.windows?.length
      ? entry.windows.map((window) => `${Math.round(window.percent)}%`).join(" · ")
      : entry?.note ?? t("Not read yet");
  return `
    ${groupBox(
      "Claude",
      row(
        t("Claude limits integration"),
        t("Adds a usage status line to new Claude sessions so limits appear in AI Limits."),
        toggle("claude_usage", settings.claude_usage, t("Claude limits integration")),
      ) + row(t("Current"), summary(reading.claude), ""),
      t("Shows after the next response in a Claude session started from Convoy. Needs a recent Claude Code and an eligible subscription."),
    )}
    ${groupBox(
      "Codex",
      row(t("Current"), summary(reading.codex), button({ label: reading.loading ? t("Reading…") : t("Refresh"), action: "limits-refresh", disabled: !!reading.loading })),
      t("Read from your installed Codex login. Convoy never copies account tokens."),
    )}`;
}

function accounts() {
  const profiles = state.profiles ?? [];
  const using = (agent, id) =>
    activeAccount(agent) === id
      ? `<span class="pref-ok">${icons.check} ${escape(t("Active"))}</span>`
      : button({ label: t("Use"), data: { "use-account": `${agent}:${id}` } });
  const perAgent = AGENTS.map(([agent, name]) => {
    const own = profiles.filter((profile) => profile.agent === agent);
    const rows =
      row(t("System login"), t("Your normal login in a terminal. Convoy never changes it."), using(agent, "")) +
      own
        .map((profile) =>
          row(
            profile.label,
            profile.sessions ? plural(profile.sessions, "session") : t("Not in use yet"),
            `${using(agent, profile.id)}
             ${button({ label: t("Log in…"), data: { "login-account": profile.id } })}
             ${button({
               icon: "trash",
               kind: "quiet",
               data: { "remove-profile": profile.id },
               disabled: profile.sessions > 0,
               title: profile.sessions ? t("Bound to saved sessions") : t("Remove this account"),
             })}`,
          ),
        )
        .join("");
    return groupBox(
      name,
      rows,
      agent === "claude"
        ? t("Each account is its own CLAUDE_CONFIG_DIR, so sign-ins stay separate. The active one is what new sessions start with; the New Session sheet can pick another.")
        : t("Each account is its own CODEX_HOME. The active one is what new sessions start with."),
    );
  }).join("");
  return `
    ${perAgent}
    ${groupBox(
      t("Add account"),
      `<div class="pref-inline">
         ${segmented(
           "profile_agent",
           AGENTS.map(([value, text]) => [value, text, agentIcon(value, 13)]),
           state.profileAgent,
         )}
         <input id="pref-profile-label" placeholder="${escape(t("Label, e.g. work"))}" spellcheck="false" />
         ${button({ label: t("Add & log in"), action: "page-add-profile", kind: "primary" })}
       </div>`,
      t("Adding an account opens its login in a terminal."),
    )}`;
}

function integrations() {
  const connections = state.integrations ?? [];
  const auth = (item) =>
    item.auth === "mcp" ? t("Agent MCP server") : item.auth === "password" ? t("Login & password") : item.kind === "jira" ? t("API token") : t("API key");
  const rows = connections.length
    ? connections
        .map((item) =>
          row(
            item.name,
            [item.kind === "jira" ? "Jira" : "Linear", auth(item), item.site, item.username].filter(Boolean).join(" · "),
            `${button({ label: t("Edit"), data: { "conn-edit": item.id } })}
             ${button({ icon: "trash", kind: "quiet", data: { "conn-remove": item.id }, title: t("Remove") })}`,
          ),
        )
        .join("")
    : empty(t("No connections yet."));
  return `
    ${groupBox(
      t("Connections"),
      rows,
      t("Import issues as tasks from a project's Tasks tab. MCP connections store nothing: the agent fetches each issue with its own MCP server."),
    )}
    <div class="pref-actions">
      ${button({ label: t("Add {name}", { name: "Linear" }), icon: "plus", data: { "conn-add": "linear" } })}
      ${button({ label: t("Add {name}", { name: "Jira" }), icon: "plus", data: { "conn-add": "jira" } })}
    </div>`;
}

function quickCommands() {
  const commands = state.quickCommands ?? [];
  const rows = commands.length
    ? commands
        .map((command) =>
          row(
            command.title,
            `${shorten(command.text, 90)}${command.submit ? ` · ${t("sends Enter")}` : ""}`,
            button({ icon: "trash", kind: "quiet", data: { "remove-command": command.id }, title: t("Remove") }),
          ),
        )
        .join("")
    : empty(t("None yet."));
  return `
    ${groupBox(
      t("Commands"),
      rows,
      t("Saved text you can send into a running agent from the ⚡ menu. Global ones appear in every project."),
    )}
    <div class="pref-actions">${button({ label: t("Add command…"), icon: "plus", action: "manage-commands" })}</div>`;
}

function shortcuts(settings) {
  const fallback = (action) => SHORTCUTS.find(([name]) => name === action)?.[1] ?? "";
  return (
    SHORTCUT_GROUPS.map((group) =>
      groupBox(
        t(group),
        SHORTCUTS.filter(([, , , owner]) => owner === group)
          .map(([action, , what]) => {
            const saved = settings.shortcuts?.[action];
            const custom = saved !== undefined && saved !== fallback(action);
            const shown = saved === "" ? t("None") : label(saved || fallback(action));
            return row(
              t(what),
              "",
              `<button class="button shortcut${state.capturing === action ? " shortcut--listening" : ""}${saved === "" ? " shortcut--unbound" : ""}"
                       data-capture="${action}">
                 ${state.capturing === action ? escape(t("Press a key… (⌫ for none)")) : escape(shown)}
               </button>
               ${custom ? button({ icon: "refresh", kind: "quiet", title: t("Back to the default"), data: { "shortcut-reset": action } }) : ""}`,
            );
          })
          .join(""),
      ),
    ).join("") +
    `<p class="pref-group__footer">${escape(t("Click a shortcut, then press the new keys. Backspace leaves it unbound; Escape cancels. A key already in use is refused and named."))} ${button({ label: t("Restore defaults"), action: "reset-shortcuts", kind: "quiet" })}</p>`
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
        <button class="prefs__back" data-action="close-settings">${icons.back} ${escape(t("Back to app"))}</button>
        <div class="sidebar__search prefs__search">
          ${icons.search}
          <input id="settings-search" type="search" placeholder="${escape(t("Search settings"))}"
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
              ${icons[icon] ?? ""}<span>${escape(t(name))}</span>
            </button>`,
                  )
                  .join("")
              : `<div class="sidebar__none">${escape(t("No matching settings"))}</div>`
          }
        </nav>
        <div class="prefs__foot" title="${escape(state.storage)}">${version}<br />${escape(shorten(state.storage, 34))}</div>
      </aside>
      <div class="prefs__page" data-scroll="settings">
        <div class="prefs__content">
          <h1 class="prefs__title">${escape(t(current))}</h1>
          ${PAGES[current](state.settings)}
        </div>
      </div>
    </div>`;
}
