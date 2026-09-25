import { chromium } from "playwright";
import { createServer } from "vite";
import assert from "node:assert/strict";

// Real DOM, mocked IPC: native PTY and persistence are tested by cargo test.
const server = await createServer({ server: { host: "127.0.0.1", port: 1421, strictPort: true } });
await server.listen();
let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CONVOY_TEST_BROWSER || undefined, headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 840 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    const projects = [], tasks = [];
    const sessions = [{ id: "builder", project_id: "project", title: "Fix login", agent: "claude", provider_id: "id", started: true, running: true, review_of: null }];
    const calls = [];
    window.checkCalls = calls;
    // A pinned session in a second project, for the sidebar's Pinned list.
    const elsewhere = [{ id: "notes", project_id: "other", title: "Release notes", agent: "codex", provider_id: "", started: false, running: false, pinned: true, review_of: null }];
    window.__TAURI_INTERNALS__ = { transformCallback: () => 1, invoke: async (cmd, args) => {
      calls.push({ cmd, args });
      window.savedSettings ??= { theme: "light", default_agent: "claude", font_size: 13, scrollback: 10000, claude_usage: false, notifications: false, keep_awake: "off", hibernate_minutes: 0, shortcuts: {} };
      if (cmd === "settings_read") return { ...window.savedSettings, storage: "/fixture" };
      if (cmd === "settings_save") { window.savedSettings = { ...args.input }; return null; }
      if (cmd === "workspace_read") return { projects, sessions, running: ["builder"], storage: "/fixture/workspace.json" };
      if (cmd === "sessions_for") return sessions.filter((item) => item.project_id === args.projectId);
      if (cmd === "planning_read") return { tasks, specs: [], queued: tasks.length };
      if (cmd === "tasks_for") return tasks;
      if (["quick_commands_read"].includes(cmd)) return [];
      if (cmd === "activity_read") return window.activity ??= [
        { id: "e2", at: new Date().toISOString(), kind: "waiting", session_id: "builder", title: "Fix login", detail: "Permission to run tests", project_id: "project", project: "Example" },
        { id: "e1", at: new Date(Date.now() - 60000).toISOString(), kind: "started", session_id: "builder", title: "Fix login", detail: "Agent process launched", project_id: "project", project: "Example" },
      ];
      if (cmd === "activity_clear") { window.activity = []; return null; }
      window.docs ??= { "README.md": "# Example\n\nHello **docs**." };
      if (cmd === "docs_list") return Object.keys(window.docs).sort((a, b) => (a.startsWith(".specdesk") ? -1 : 1));
      if (cmd === "doc_read") return window.docs[args.path];
      if (cmd === "doc_write") { window.docs[args.path] = args.text; return null; }
      if (cmd === "spec_create") { const path = ".specdesk/specs/checkout-retries.md"; window.docs[path] = "# Checkout retries\n\n## Problem\n"; return path; }
      if (cmd === "doc_prompts") return { project_doc: "Write the project document", spec: "Draft the specification" };
      if (cmd === "monitor_tick") return { states: [{ id: "builder", state: "waiting", notable: false }], hibernate: [], changed: false };
      if (cmd === "plugin:dialog|open") return "/fixture/Example";
      if (cmd === "project_open") {
        projects.push({ id: "project", title: "Example", path: args.path, sessions: 1, running: 1 }, { id: "other", title: "Other", path: "/fixture/Other", sessions: 1, running: 0 });
        sessions.push(...elsewhere);
        return 2;
      }
      if (cmd === "git_status") return { branch: "main", changed_files: 1 };
      if (cmd === "review_brief") return "Review the login changes. Tests pass.";
      if (cmd === "review_create") { sessions.push({ id: "review", project_id: "project", title: "Review: Fix login", agent: "codex", provider_id: "", started: false, running: false, review_of: "builder" }); return "review"; }
      if (cmd === "review_builder") return "builder";
      if (cmd === "integrations_list") return [{ id: "linear", kind: "linear", auth: "mcp", name: "Linear MCP", has_secret: false }];
      if (cmd === "issues_parse_keys") return [{ key: "ENG-7", title: "Test imported task", details: "", origin: "linear:test" }];
      if (cmd === "issues_import") {
        tasks.push({ id: "task", title: "ENG-7: Test imported task", details: "", findings: "", agent: args.options.agent, model: args.options.model, status: "queued", mode: "none", auto_review: false, source: { tracker: "linear", key: "ENG-7", origin: "linear:test" } });
        return { created: ["task"], skipped: 0 };
      }
      if (cmd === "task_agent") { tasks[0].model = args.model; tasks[0].agent = args.agent; return null; }
      if (cmd === "plugin:event|listen") return 1;
      if (cmd === "limits_read") {
        const now = Date.now() / 1000;
        return {
          claude: { windows: [{ name: "five_hour", percent: 42, resets_at: now + 3600, minutes: 300 }], note: null, updated_at: now },
          codex: args.codex ? { windows: [{ name: "codex", percent: 93, resets_at: now + 86400 * 3, minutes: 10080 }], note: null, updated_at: now } : null,
        };
      }
      if (cmd === "diagnostics_run") return [
        { name: "claude", found: true, path: "/usr/local/bin/claude", version: "2.1.0", purpose: "Runs Claude Code sessions.", install: "npm i -g" },
        { name: "gh", found: false, path: null, version: null, purpose: "Opens pull requests.", install: "Install the GitHub CLI." },
      ];
      if (cmd === "worktrees_path") return "/fixture/worktrees";
      const report = { projects: 3, projects_existing: 0, sessions: 5, tasks: 2, specs: 0, quick_commands: 0, activity: 4, settings: !!args?.settings, shortcuts: 0, connections: 0, warnings: [] };
      if (cmd === "macos_import_preview") return report;
      if (cmd === "macos_import_run") return { ...report, warnings: ["2 sessions run in a worktree the macOS app made."] };
      if (cmd === "project_detail") {
        const p = projects.find((item) => item.id === args.id);
        return { id: p.id, title: p.title, path: p.path, group: "", icon: p.icon ?? "", setup_command: "", shared_paths: "", review_template: "",
                 color: p.color ?? "", default_agent: p.default_agent ?? "", base_ref: "", branch_prefix: "", task_mode: "", auto_run_tasks: false };
      }
      if (cmd === "project_edit") { Object.assign(projects.find((item) => item.id === args.id), args.input); return null; }
      if (cmd === "project_git") return { branch: "main", changed_files: 2 };
      if (cmd === "git_refs") return args.projectId === "project" ? ["main", "feature/x", "origin/main", "tag:v1.0"] : [];
      if (cmd === "profiles_read") return [{ id: "work", label: "Work", agent: "codex", sessions: 0 }];
      if (cmd === "session_create") {
        sessions.push({ id: "fresh", project_id: args.projectId, title: args.title, agent: args.agent, provider_id: "", started: false, running: false, pinned: false, review_of: null });
        return "fresh";
      }
      if (cmd === "worktree_create") return { branch: args.branch, shared_paths: [], setup_command: null, directory: "/fixture/worktrees/x" };
      if (cmd === "account_login") return `login:${args.profileId}`;
      if (cmd === "session_stop") return null;
      if (cmd === "session_start") { sessions.find((item) => item.id === args.id).running = true; return null; }
      if (["keep_awake", "terminal_resize", "terminal_write", "terminal_paste"].includes(cmd)) return null;
      throw new Error(`Unmocked IPC: ${cmd}`);
    } };
  });
  await page.goto("http://127.0.0.1:1421");
  // An empty workspace on a Mac with the macOS app's data is offered it once.
  const offer = page.locator(".modal", { hasText: "Import from the macOS app" });
  await offer.getByText("3 projects").waitFor();
  assert.equal(await offer.locator('[data-toggle="settings"]').getAttribute("aria-pressed"), "true");
  await offer.getByRole("button", { name: "Not now" }).click();
  await offer.waitFor({ state: "detached" });
  assert.equal(await page.evaluate(() => localStorage.getItem("convoy.macImportOffered")), "1");
  await page.locator('[data-action="open-folder"]').first().click();
  await page.getByRole("heading", { name: "Example", exact: true }).waitFor();
  await page.locator('[data-open="builder"]').first().click();
  await page.locator('[data-action="session-menu"]').last().click();
  await page.locator('[data-action="start-review"]').click();
  await page.locator('[data-action="create-review"]').click();
  await page.locator('[data-action="session-menu"]').last().click();
  await page.locator('[data-action="send-feedback"]').click();
  await page.locator("#draft-feedback").waitFor();
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await page.evaluate(() => document.activeElement.id), "draft-feedback", "terminal must not steal dialog focus");
  await page.locator("#draft-feedback").fill("Fix the error path before shipping.");
  await page.locator('[data-action="insert-feedback"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "terminal_paste"));
  // A session has the whole height, with a tab like the macOS app's.
  assert.equal(await page.locator(".header").count(), 0, "the project header gives way to the session");
  await page.locator('.tab-item[data-tab-open="builder"]').waitFor();
  await page.locator('.tab-item[data-tab-open="review"]').waitFor();
  const primary = process.platform === "darwin" ? "Meta" : "Control";
  await page.keyboard.press(`${primary}+1`);
  await page.locator('.tab-item--selected[data-tab-open="builder"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/tabs.png` });
  // ⌘+ / ⌘0 zoom this terminal only, from the Settings size (13).
  await page.keyboard.press(`${primary}+Equal`);
  await page.locator(".toast", { hasText: "Terminal text 14 pt" }).waitFor();
  await page.keyboard.press(`${primary}+Digit0`);
  await page.locator(".toast", { hasText: "Terminal text 13 pt" }).waitFor();
  // ⌘E lists open terminals most recent first, the one on screen last.
  await page.keyboard.press(`${primary}+e`);
  const switcher = page.locator(".modal--palette");
  await switcher.locator("#palette-input[placeholder^='Switch to an open terminal']").waitFor();
  assert.deepEqual(await switcher.locator(".palette__title").allTextContents(), ["Review: Fix login", "Fix login"]);
  await page.keyboard.press("Enter");
  await page.locator('.tab-item--selected[data-tab-open="review"]').waitFor();
  await page.keyboard.press(`${primary}+e`);
  await page.keyboard.press("Enter");
  await page.locator('.tab-item--selected[data-tab-open="builder"]').waitFor();
  // ⌘F finds in the terminal on screen; Escape closes the bar.
  await page.keyboard.press(`${primary}+f`);
  await page.waitForFunction(() => document.activeElement?.id === "find-term");
  await page.keyboard.type("nothing like this");
  await page.locator(".find-bar__count", { hasText: "No matches" }).waitFor();
  await page.locator('[data-action="find-case"]').click();
  await page.locator('[data-action="find-case"][aria-pressed="true"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/find.png` });
  await page.locator("#find-term").press("Escape");
  await page.locator(".find-bar").waitFor({ state: "detached" });
  // Files dropped onto a running terminal are typed in as quoted paths.
  const host = await page.locator("#terminal-host").boundingBox();
  assert(await page.evaluate(([x, y]) => window.convoyDrop(["/tmp/a b.txt", "/work/notes.md"], x, y), [host.x + 40, host.y + 40]));
  const typed = process.platform === "win32" ? '"/tmp/a b.txt" /work/notes.md ' : "'/tmp/a b.txt' /work/notes.md ";
  await page.waitForFunction((text) => window.checkCalls.some((c) => c.cmd === "terminal_write" && c.args.id === "builder" && c.args.data.includes(text)), typed);
  await page.locator('.tab-item[data-tab-open="review"]').click({ button: "right" });
  await page.locator('[data-tab-act="close"]').click();
  await page.locator('.tab-item[data-tab-open="review"]').waitFor({ state: "detached" });
  // ⇧⌘T brings the closed tab back.
  await page.keyboard.press(`${primary}+Shift+t`);
  await page.locator('.tab-item--selected[data-tab-open="review"]').waitFor();
  await page.locator('.tab-item[data-tab-open="review"]').click({ button: "right" });
  await page.locator('[data-tab-act="close"]').click();
  await page.locator('.tab-item[data-tab-open="review"]').waitFor({ state: "detached" });
  // As on macOS: no Stop in the session bar; it is in the session menu.
  assert.equal(await page.locator('.workbench__bar [data-stop]').count(), 0);
  await page.locator('.workbench__bar [data-action="session-menu"]').first().click();
  const opener = await page.locator('.workbench__bar [data-action="session-menu"]').first().boundingBox();
  const menu = await page.locator(".menu--session").boundingBox();
  assert(menu.y - (opener.y + opener.height) < 12, "the menu opens right under its button");
  await page.locator('[data-action="stop-session"]').click();
  await page.locator(".modal", { hasText: "Stop this agent?" }).waitFor();
  await page.keyboard.press("Escape");
  // Four panes, as on macOS: empty ones offer a session, clicking focuses.
  await page.locator('[data-layout="4"]').click();
  assert.equal(await page.locator(".workbench__panes--4 .pane").count(), 4);
  await page.locator('[data-pane-pick="1"]').click();
  await page.locator('.modal [data-split="review"]').click();
  await page.locator('.pane[data-pane="1"] .pane__title', { hasText: "Review: Fix login" }).waitFor();
  await page.locator("#terminal-host-1 .xterm").waitFor();
  await page.locator('.pane[data-pane="0"] .pane__label').click();
  await page.locator('.pane--focused[data-pane="0"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/panes.png` });
  await page.locator('[data-layout="1"]').click();
  assert.equal(await page.locator(".workbench__panes").count(), 0);
  await page.locator(".status__needs", { hasText: "1 needs you" }).waitFor();
  await page.locator('[data-action="awake-menu"]').click();
  await page.locator('.menu--awake [data-action="awake-settings"]').waitFor();
  await page.locator('[data-awake="sessions"]').click();
  await page.waitForFunction(() => window.savedSettings.keep_awake === "sessions");

  // The sidebar folds away from the tab bar, and ⌘B brings it back.
  await page.locator('[data-action="toggle-sidebar"]').click();
  await page.locator(".sidebar").waitFor({ state: "detached" });
  await page.keyboard.press(`${primary}+b`);
  await page.locator(".sidebar").waitFor();
  await page.locator('[data-action="home"]').click();
  await page.locator(".header").waitFor();

  await page.locator('[data-tab="tasks"]').click();
  await page.locator('[data-action="import"]').click();
  await page.locator("#imp-manual").fill("ENG-7 Test imported task");
  await page.locator("#imp-model").fill("sonnet");
  await page.locator('[data-action="import-run"]:not([disabled])').click();
  await page.locator('.row[data-task="task"]').click();
  await page.locator('[data-action="edit-task-model"]').click();
  assert.equal(await page.locator("#task-model").inputValue(), "sonnet");
  await page.locator("#task-model").fill("opus");
  await page.locator('[data-action="task-save"]').click();
  await page.locator('.row[data-task="task"]').waitFor();

  // Sidebar: the project shows its icon, its sessions show the agent's mark,
  // and right-click offers the project's and the session's actions.
  const shots = process.env.CONVOY_TEST_SHOTS;
  await page.locator('.project-row .project-icon svg').first().waitFor();
  assert(await page.locator('.side-session .agent-icon').count() >= 1, "sessions show the agent's mark");
  await page.locator('[data-menu-project="project"]').click({ button: "right" });
  await page.locator('[data-menu-act="project-settings"]').waitFor();
  if (shots) await page.waitForTimeout(250), await page.screenshot({ path: `${shots}/project-menu.png` });
  await page.keyboard.press("Escape");
  await page.locator('.side-session[data-menu-session="builder"]').click({ button: "right" });
  await page.locator('[data-menu-act="stop"]').waitFor();
  await page.keyboard.press("Escape");

  // A click inside an open dialog re-renders it, and must not replay its
  // entrance: the whole modal blinked on every toggle.
  await page.locator('.tabbar [data-action="new-session"]').click();
  await page.locator('.choice [data-value="codex"]').click();
  await page.locator(".scrim--settled .modal").waitFor();
  assert.equal(await page.evaluate(() => document.querySelector(".modal").getAnimations().length), 0, "no animation after a toggle");
  assert(await page.locator('.choice [data-value="codex"] .agent-icon').count(), "agent choices carry their marks");
  // The sheet names the account for the agent, offers a worktree from any
  // ref, and starting it starts the agent.
  await page.locator("#draft-account option", { hasText: "Work" }).waitFor({ state: "attached" });
  await page.locator('.modal [data-toggle="worktree"]').click();
  await page.locator("#draft-base").fill("origin/main");
  await page.locator("#draft-title").fill("Fix flaky checkout");
  assert.equal(await page.locator("#draft-branch").inputValue(), "convoy/fix-flaky-checkout", "the branch follows the name");
  await page.locator("#draft-account").selectOption("work");
  if (shots) await page.screenshot({ path: `${shots}/new-session.png` });
  await page.locator('[data-action="create-session"]').click();
  await page.locator('.tab-item--selected[data-tab-open="fresh"]').waitFor();
  const made = await page.evaluate(() => window.checkCalls.filter((c) => ["session_create", "worktree_create", "session_start"].includes(c.cmd)).map((c) => ({ cmd: c.cmd, ...c.args })));
  assert.deepEqual(made.map((c) => c.cmd), ["session_create", "worktree_create", "session_start"]);
  assert.equal(made[0].profileId, "work");
  assert.equal(made[0].agent, "codex");
  assert.deepEqual([made[1].branch, made[1].base], ["convoy/fix-flaky-checkout", "origin/main"]);

  // Project settings: a page, as on macOS, saving each choice.
  await page.locator('[data-menu-project="project"]').click({ button: "right" });
  await page.locator('[data-menu-act="project-settings"]').click();
  await page.locator(".prefs__title", { hasText: "Example" }).waitFor();
  await page.locator('[data-proj-icon="🚀"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.icon === "🚀"));
  await page.locator('[data-proj-color="#4CAF83"]').click();
  await page.locator('.swatch[data-proj-color="#4CAF83"][aria-pressed="true"]').waitFor();
  await page.locator('[data-icon-tab="symbol"]').click();
  await page.locator('[data-proj-icon="sf:cpu"]').click();
  await page.locator('.project-row .project-icon__dot').first().waitFor();
  await page.locator('[data-proj-select="default_agent"]').selectOption("codex");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.default_agent === "codex"));
  if (shots) await page.screenshot({ path: `${shots}/project-settings.png`, fullPage: true });
  await page.locator('[data-action="close-project-settings"]').click();
  await page.locator(".header").waitFor();

  // Activity: a bell with the unread count; opening the feed reads it.
  await page.locator(".bell__badge", { hasText: "2" }).waitFor();
  await page.locator('.tabbar [data-action="activity"]').click();
  await page.locator(".activity", { hasText: "needs you" }).waitFor();
  if (shots) await page.waitForTimeout(250), await page.screenshot({ path: `${shots}/activity.png` });
  await page.keyboard.press("Escape");
  assert.equal(await page.locator(".bell__badge").count(), 0, "the badge clears once the feed is read");

  // Docs: the repository's Markdown, rendered, and a spec from the template.
  await page.locator('[data-tab="docs"]').click();
  await page.locator('[data-doc="README.md"]').click();
  await page.locator(".docs__render h1", { hasText: "Example" }).waitFor();
  await page.locator('.docs__list [data-action="doc-new-spec"]').click();
  await page.locator("#draft-title").fill("Checkout retries");
  await page.locator('[data-action="doc-create-spec"]').click();
  await page.locator("#doc-editor").waitFor();
  await page.locator("#doc-editor").pressSequentially("More.");
  await page.locator('[data-action="doc-save"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "doc_write" && c.args.text.endsWith("More.")));
  if (shots) await page.screenshot({ path: `${shots}/docs.png` });

  // AI Limits: the status bar shows the peak reading, and the panel asks
  // Codex on opening.
  await page.locator(".status__limit", { hasText: "Claude 42%" }).waitFor();
  await page.locator('.status [data-action="limits"]').click();
  await page.locator(".limits .limit", { hasText: "7-day window" }).waitFor();
  assert(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "limits_read" && c.args.codex)));
  await page.locator(".status__limit--high", { hasText: "Codex 93%" }).waitFor();
  if (shots) await page.waitForTimeout(250), await page.screenshot({ path: `${shots}/limits.png` });
  await page.keyboard.press("Escape");

  // A shortcut with Option held: the key is read from the physical key, so
  // the character Option types does not stop it.
  // `mod` is ⌘ on macOS and Control elsewhere; Control is its own modifier.
  const mod = process.platform === "darwin" ? "Meta" : "Control";
  await page.keyboard.press(`${mod}+Alt+2`);
  await page.locator('[data-tab="reviews"][aria-selected="true"]').waitFor();
  // ⌃Tab, the macOS binding for the next session, now that Ctrl-only
  // bindings exist.
  await page.keyboard.press("Control+Tab");
  await page.locator(".workbench").waitFor();

  // Settings is a page, and a change is saved as soon as it is made.
  await page.locator(".sidebar__gear").click();
  await page.locator(".prefs__title", { hasText: "General" }).waitFor();
  await page.locator('[data-settings-section="Appearance"]').click();
  await page.locator('[data-pref-set="theme"][data-value="dark"]').click();
  await page.waitForFunction(() => window.savedSettings.theme === "dark");
  await page.locator('[data-settings-section="Setup"]').click();
  await page.locator(".pref-row", { hasText: "gh" }).locator(".pref-missing").waitFor();
  if (shots) await page.screenshot({ path: `${shots}/setup.png` });
  await page.locator('[data-action="macos-import"]').click();
  await page.locator('.modal [data-toggle="settings"][aria-pressed="false"]').waitFor();
  if (shots) await page.screenshot({ path: `${shots}/macos-import.png` });
  await page.locator('[data-action="macos-import-run"]').click();
  await page.locator(".modal", { hasText: "Imported, with notes" }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "macos_import_run").args), { settings: false });
  await page.keyboard.press("Escape");
  await page.locator('[data-settings-section="Shortcuts"]').click();
  await page.locator(".pref-group__title", { hasText: "Project" }).waitFor();
  assert(await page.locator(".shortcut").count() >= 30, "the macOS set of shortcuts is listed");
  // Accounts, per agent as on macOS: which one new sessions use, and a login
  // terminal that ends when the dialog closes.
  await page.locator('[data-settings-section="Accounts"]').click();
  // Starting a session with Work made it the active account already.
  await page.locator(".pref-row", { hasText: "Work" }).locator(".pref-ok").waitFor();
  await page.locator('[data-use-account="codex:"]').click();
  await page.locator('[data-use-account="codex:work"]').click();
  await page.locator(".pref-row", { hasText: "Work" }).locator(".pref-ok").waitFor();
  assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("convoy.activeAccount")).codex), "work");
  await page.locator('[data-login-account="work"]').click();
  await page.locator(".modal", { hasText: "Log in: Work" }).locator("#login-host .xterm").waitFor();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "account_login" && c.args.profileId === "work"));
  if (shots) await page.screenshot({ path: `${shots}/login.png` });
  // Escape belongs to the login in the terminal; Close ends it.
  await page.locator(".modal .button", { hasText: "Close" }).click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_stop" && c.args.id === "login:work"));
  await page.locator('[data-settings-section="Notifications"]').click();
  await page.locator('[data-pref-toggle="notifications"]').click();
  await page.waitForFunction(() => window.savedSettings.notifications === true);
  await page.locator("#settings-search").fill("jira");
  assert.equal(await page.locator(".prefs__item").count(), 1, "search narrows the sections");
  await page.locator('[data-settings-section="Linear & Jira"]').click();
  await page.locator(".pref-row", { hasText: "Linear MCP" }).waitFor();
  if (shots) await page.screenshot({ path: `${shots}/settings.png` });
  await page.keyboard.press("Escape");
  await page.locator(".sidebar").waitFor();

  // Pinned sessions from every project sit above the projects, and every
  // project lists its sessions; opening one selects its project.
  await page.locator(".sidebar__group", { hasText: "Pinned" }).waitFor();
  const pinnedRow = page.locator('.project-row__sessions--pinned [data-open="notes"]');
  await pinnedRow.getByText("Other").waitFor();
  assert.equal(await page.locator('.project-row__sessions:not(.project-row__sessions--pinned) [data-open="notes"]').count(), 1);
  await page.locator('[data-expand="other"]').click();
  assert.equal(await page.locator('.project-row__sessions:not(.project-row__sessions--pinned) [data-open="notes"]').count(), 0);
  assert.deepEqual(await page.evaluate(() => JSON.parse(localStorage.getItem("convoy.collapsedProjects"))), ["other"]);
  await pinnedRow.click();
  await page.locator('.project-row--current [data-project="other"]').waitFor();
  await page.locator('.tab-item--selected[data-tab-open="notes"]').waitFor();

  const calls = await page.evaluate(() => window.checkCalls);
  assert(calls.some(c => c.cmd === "terminal_paste" && c.args.id === "builder" && c.args.text === "Fix the error path before shipping." && c.args.submit === false));
  assert(calls.some(c => c.cmd === "issues_import" && c.args.options.model === "sonnet"));
  assert(calls.some(c => c.cmd === "task_agent" && c.args.model === "opus"));
  assert.deepEqual(errors, []);
  assert.equal(await page.locator(".toast--error").count(), 0);
  if (process.env.CONVOY_TEST_SCREENSHOT) await page.screenshot({ path: process.env.CONVOY_TEST_SCREENSHOT });
  console.log("PASS: folder → builder → review → feedback; Linear MCP import → per-task model; sidebar menus; settled dialogs; settings page saves; no browser errors. IPC mocked.");
} finally {
  await browser?.close();
  await server.close();
}
