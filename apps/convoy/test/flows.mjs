import { chromium, webkit } from "playwright";
import { createServer } from "vite";
import assert from "node:assert/strict";

// Real DOM, mocked IPC: native PTY and persistence are tested by cargo test.
const server = await createServer({ server: { host: "127.0.0.1", port: 1421, strictPort: true } });
await server.listen();
let browser;
try {
  // CONVOY_TEST_ENGINE=webkit runs it in WebKit, which is what the macOS
  // app draws with; Chromium is the default.
  browser =
    process.env.CONVOY_TEST_ENGINE === "webkit"
      ? await webkit.launch({ headless: true })
      : await chromium.launch({ executablePath: process.env.CONVOY_TEST_BROWSER || undefined, headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 840 } });
  // Headless WebKit lets the page write the clipboard but not the test read
  // it back, so those checks run in Chromium only.
  const readClipboard = () => (process.env.CONVOY_TEST_ENGINE === "webkit" ? Promise.resolve(null) : page.evaluate(() => navigator.clipboard.readText()));
  if (process.env.CONVOY_TEST_ENGINE !== "webkit") await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: "http://127.0.0.1:1421" });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    const projects = [], tasks = [];
    const sessions = [{ id: "builder", project_id: "project", title: "Fix login", agent: "claude", provider_id: "id", started: true, running: true, review_of: null }];
    const calls = [];
    window.checkCalls = calls;
    window.mockSessions = sessions;
    // Events the backend would send, delivered to what the page listens for.
    const callbacks = new Map();
    const listeners = {};
    window.emitTauri = (event, payload) => (listeners[event] ?? []).forEach((handler) => callbacks.get(handler)?.({ event, id: 0, payload }));
    // A pinned session in a second project, for the sidebar's Pinned list.
    const elsewhere = [{ id: "notes", project_id: "other", title: "Release notes", agent: "codex", provider_id: "", started: false, running: false, pinned: true, review_of: null }];
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } }, transformCallback: (callback) => { const id = callbacks.size + 1; callbacks.set(id, callback); return id; }, invoke: async (cmd, args) => {
      calls.push({ cmd, args });
      window.savedSettings ??= { theme: "light", default_view: "terminal", default_agent: "claude", font_size: 13, scrollback: 10000, claude_usage: false, notifications: false, keep_awake: "off", hibernate_minutes: 0, shortcuts: {} };
      if (cmd === "settings_read") return { ...window.savedSettings, storage: "/fixture" };
      if (cmd === "settings_save") { window.savedSettings = { ...args.input }; return null; }
      // Sessions with a view of their own (the chat flow's) run and stop for real.
      if (cmd === "workspace_read") return { projects, sessions, running: ["builder", ...sessions.filter((item) => item.view && item.running).map((item) => item.id)], storage: "/fixture/workspace.json" };
      if (cmd === "sessions_for") return sessions.filter((item) => item.project_id === args.projectId);
      if (cmd === "planning_read") return {
        tasks, queued: tasks.length,
        specs: [{ id: "spec1", title: "Checkout retries", problem: "Payments time out", requirements: "Retry twice", acceptance: "", constraints: "", plan: "", revision: 1, approved: true }],
      };
      if (cmd === "tasks_for") return tasks;
      if (cmd === "quick_commands_read") return [{ id: "q1", title: "Run tests", text: "npm test", submit: true, project_id: "project" }];
      if (["quick_command_send", "quick_command_save"].includes(cmd)) return null;
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
        projects.push({ id: "project", title: "Example", path: args.path, sessions: 1, running: 1 }, { id: "other", title: "Other", path: "/fixture/Other", sessions: 1, running: 0 }, { id: "alpha", title: "Alpha", path: "/fixture/clients/alpha", group: "clients", sessions: 0, running: 0 });
        sessions.push(...elsewhere);
        return 3;
      }
      if (cmd === "git_status") return { branch: "main", changed_files: 1 };
      if (cmd === "review_brief") return "Review the login changes. Tests pass.";
      if (cmd === "review_create") { sessions.push({ id: "review", project_id: "project", title: "Review: Fix login", agent: "codex", provider_id: "", started: false, running: false, review_of: "builder" }); return "review"; }
      if (cmd === "review_builder") return "builder";
      if (cmd === "integrations_list") return [{ id: "linear", kind: "linear", auth: "mcp", name: "Linear MCP", has_secret: false }];
      if (cmd === "issues_parse_keys") return [{ key: "ENG-7", title: "Test imported task", details: "", origin: "linear:test" }];
      if (cmd === "issues_import") {
        tasks.push({ id: "task", project_id: "project", title: "ENG-7: Test imported task", details: "", findings: "", agent: args.options.agent, model: args.options.model, status: "queued", mode: "none", auto_review: false, source: { tracker: "linear", key: "ENG-7", origin: "linear:test" } });
        return { created: ["task"], skipped: 0 };
      }
      if (cmd === "task_agent") { tasks[0].model = args.model; tasks[0].agent = args.agent; return null; }
      if (cmd === "task_save") {
        const task = tasks.find((item) => item.id === args.input.id);
        if (task) { Object.assign(task, { title: args.input.title, mode: args.input.mode }); return task.id; }
        const id = `new${tasks.length}`;
        tasks.push({ id, project_id: args.input.project_id, title: args.input.title, details: "", findings: "", agent: args.input.agent, status: "queued", mode: args.input.mode, auto_review: args.input.auto_review, spec_id: args.input.spec_id, session_id: null, depends_on: [], blocked_by: [] });
        return id;
      }
      if (cmd === "task_dependencies") {
        const task = tasks.find((item) => item.id === args.id);
        task.depends_on = args.dependsOn;
        task.blocked_by = args.dependsOn.map((id) => tasks.find((item) => item.id === id).title);
        return null;
      }
      if (cmd === "task_link_session") { tasks.find((item) => item.id === args.id).session_id = args.sessionId; return null; }
      if (cmd === "task_status") { tasks.find((item) => item.id === args.id).status = args.status; return null; }
      if (cmd === "task_delete") { tasks.splice(tasks.findIndex((item) => item.id === args.id), 1); return null; }
      if (cmd === "tasks_all") return tasks;
      if (cmd === "project_move") { const [moved] = projects.splice(projects.findIndex((item) => item.id === args.id), 1); projects.splice(args.index, 0, moved); return null; }
      if (cmd === "project_refresh") return 0;
      if (cmd === "session_archive") { const item = sessions.find((entry) => entry.id === args.id); item.archived = args.archived; return null; }
      if (cmd === "files_snapshot") return {
        branch: "main", files: [], branches: ["main", "origin/main"], directory: "/fixture/Example",
        log: [{ id: "abc1234def5678", short: "abc1234", subject: "Fix the login race", author: "Test", date: "2026-09-25T10:00:00Z" }],
        upstream: { name: "origin/main", ahead: 2, behind: 1 },
        changes: [
          { status: "UU", staged: false, untracked: false, conflict: true, path: "src/app.js", original: null },
          { status: " M", staged: false, untracked: false, conflict: false, path: "README.md", original: null },
        ],
      };
      if (cmd === "files_read") return args.selection.path === "README.md"
        ? { kind: "text", text: Array.from({ length: 3500 }, (_, i) => `+line ${i}`).join("\n"), language: "diff", hash: "r" }
        : { kind: "text", text: "<<<<<<< HEAD", language: "diff", hash: "h" };
      if (cmd === "review_template_default") return "DEFAULT BRIEF";
      if (cmd === "models_list") return args.agent === "claude"
        ? [{ id: "opus", name: "Opus · most capable", efforts: ["low", "medium", "high", "xhigh", "max"] }, { id: "sonnet", name: "Sonnet · balanced", efforts: ["low", "high"] }]
        : [{ id: "gpt-6", name: "GPT-6", efforts: ["low", "medium", "high", "ultra"] }];
      if (cmd === "task_account") { tasks.find((item) => item.id === args.id).profile_id = args.profileId; return null; }
      if (["files_mutate", "files_open"].includes(cmd)) return cmd === "files_mutate" ? "" : null;
      if (cmd === "history_scan") return [{ agent: "claude", provider_id: "0199aaaa-0000-4000-8000-000000000001", title: "Fix flaky login", at: Date.now(), directory: "/fixture/worktrees/fix-login", profile_id: null, profile: null, session_id: null }];
      if (cmd === "transcripts_import") {
        sessions.push({ id: "imported", project_id: args.projectId, title: args.title, agent: args.agent, provider_id: args.providerId, started: true, running: false, pinned: false, review_of: null });
        return "imported";
      }
      if (cmd === "history_sweep_scan") {
        await new Promise((resolve) => setTimeout(resolve, 200));
        return [
          { agent: "claude", provider_id: "0199aaaa-0000-4000-8000-000000000011", title: "Refactor auth", at: Date.now(), place: "~/fixture/Example/api", project_id: "project", reason: "folder", votes: 0, suggestions: [] },
          { agent: "codex", provider_id: "0199aaaa-0000-4000-8000-000000000012", title: "Tidy release notes", at: Date.now() - 86400000, place: "~", project_id: "other", reason: "paths", votes: 4, suggestions: [] },
          { agent: "claude", provider_id: "0199aaaa-0000-4000-8000-000000000013", title: "Look around", at: Date.now() - 172800000, place: "~", project_id: null, reason: "none", votes: 0, suggestions: ["alpha"] },
        ];
      }
      if (cmd === "sessions_delete") return { deleted: args.items.length, running: 0, failed: 0 };
      if (cmd === "history_sweep_import") return { imported: args.choices.length, failed: 0 };
      if (cmd === "plugin:event|listen") { (listeners[args.event] ??= []).push(args.handler); return args.handler; }
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
      if (cmd === "setup_checks") return [
        { id: "gh", title: "GitHub CLI login", detail: "Not installed or not logged in.", status: "warning", fix: "Run gh auth login." },
        { id: "login.claude", title: "Claude login", detail: "Logged in", status: "ok", fix: null },
        { id: "support", title: "Convoy's data folder", detail: "/fixture", status: "ok", fix: null },
      ];
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
      if (cmd === "plugin:window|set_badge_count") { window.badge = args.value; return null; }
      if (cmd === "session_output") return { review: "Found: a token refresh race", notes: "No conversation found with session ID 0199" }[args.id] ?? "";
      if (cmd === "session_snapshot") { (window.snapshots ??= {})[args.id] = args.text; return null; }
      if (cmd === "session_recover") {
        sessions.push({ id: "notes-fresh", project_id: "other", title: "Release notes · recovery", agent: "codex", provider_id: "", started: false, running: false, pinned: false, review_of: null });
        return "notes-fresh";
      }
      if (cmd === "account_login") return `login:${args.profileId}`;
      if (cmd === "session_stop") { const item = sessions.find((entry) => entry.id === args.id); if (item?.view) item.running = false; return null; }
      if (cmd === "chat_history") return window.chatHistory?.[args.id] ?? [];
      if (cmd === "chat_start") { sessions.find((item) => item.id === args.id).running = true; return null; }
      if (["chat_send", "chat_answer", "chat_interrupt"].includes(cmd)) return null;
      if (cmd === "chat_models") return null;
      // One approval asked before this window listened, from a session it never opened.
      if (cmd === "chat_approvals") return [{ id: "chat-c", at: Date.now() - 5 * 60000 - 1000, event: { type: "approval", request: "r30", kind: "command", title: "Run `./deploy staging`", detail: "./deploy staging", always: false } }];
      if (cmd === "chat_set_model") { sessions.find((item) => item.id === args.id).model = args.model ?? ""; return null; }
      if (cmd === "chat_set_effort") { sessions.find((item) => item.id === args.id).effort = args.effort ?? ""; return null; }
      if (cmd === "session_view") { sessions.find((item) => item.id === args.id).view = args.view; return null; }
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
  // Approvals asked before the window listened are read from the backend
  // and wait in the inbox like any other, with how long they have waited.
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_approvals"));
  const inbox = page.locator(".inbox");
  await page.locator('.tabbar [data-action="approvals"]').click();
  const adopted = inbox.locator(".inbox-row", { hasText: "deploy staging" });
  await adopted.waitFor();
  assert.equal(await adopted.locator(".inbox-row__time").textContent(), "5 min");
  await page.evaluate(() => window.emitTauri("chat:event", { id: "chat-c", event: { type: "approval_resolved", request: "r30" } }));
  await adopted.waitFor({ state: "detached" });
  await page.keyboard.press("Escape");
  await inbox.waitFor({ state: "detached" });
  await page.locator('[data-action="open-folder"]').first().click();
  await page.getByRole("heading", { name: "Example", exact: true }).waitFor();
  await page.locator('[data-open="builder"]').first().click();
  await page.locator('[data-action="session-menu"]').last().click();
  await page.locator('[data-action="start-review"]').click();
  // The reviewer defaults to the other agent; Claude and a model can be picked instead.
  const reviewSheet = page.locator(".modal", { hasText: "Start review" });
  assert.equal(await reviewSheet.locator('[data-set="agent"][data-value="codex"]').getAttribute("aria-pressed"), "true");
  await reviewSheet.locator('[data-set="agent"][data-value="claude"]').click();
  await reviewSheet.locator('#draft-model option[value="sonnet"]').waitFor({ state: "attached" });
  await reviewSheet.locator("#draft-model").selectOption("sonnet");
  // Effort follows the model: Sonnet here takes only two levels.
  await reviewSheet.locator('#draft-effort option[value="xhigh"]').waitFor({ state: "detached" });
  await reviewSheet.locator("#draft-effort").selectOption("high");
  await page.locator('[data-action="create-review"]').click();
  const reviewCall = await page.evaluate(() => window.checkCalls.filter((call) => call.cmd === "review_create").at(-1).args);
  assert.equal(reviewCall.agent, "claude");
  assert.equal(reviewCall.model, "sonnet");
  assert.equal(reviewCall.effort, "high");
  await page.locator('[data-action="session-menu"]').last().click();
  await page.locator('[data-action="send-feedback"]').click();
  await page.locator("#draft-feedback").waitFor();
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await page.evaluate(() => document.activeElement.id), "draft-feedback", "terminal must not steal dialog focus");
  // Filled with the reviewer's output under a short ask, as on macOS.
  const prefilled = await page.locator("#draft-feedback").inputValue();
  assert.match(prefilled, /^Please address the review findings below/);
  assert.match(prefilled, /a token refresh race/, "the reviewer's saved output is in it");
  await page.locator("#draft-feedback").fill("Fix the error path before shipping.");
  // A redraw from elsewhere — here the backend reporting a change — keeps
  // what was typed. It used to put the field back, which is what failed on
  // the slower Windows runner.
  await page.evaluate(() => window.emitTauri("session:changed", "builder"));
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "sessions_for").length > 0);
  await page.waitForTimeout(100);
  assert.equal(await page.locator("#draft-feedback").inputValue(), "Fix the error path before shipping.");
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
  // ⌘K reaches every project's sessions and the actions, best match first.
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("notes");
  const first = page.locator(".palette__row").first();
  assert.equal(await first.locator(".palette__title").textContent(), "Release notes");
  assert.match(await first.locator(".palette__subtitle").textContent(), /^Other · Codex/);
  await page.locator("#palette-input").fill("git push");
  await page.locator(".palette__row", { hasText: "Git: Push" }).waitFor();
  await page.locator("#palette-input").fill("switch");
  assert.equal(await page.locator(".palette__row").first().locator(".palette__kind").textContent(), primary === "Meta" ? "⌘E" : "Ctrl+E");
  await page.keyboard.press("Escape");
  // The Dock badge counts the sessions waiting for you.
  await page.waitForFunction(() => window.badge === 1);
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
  // An empty pane suggests sessions from any project; the picker lists them.
  await page.locator('.pane[data-pane="2"] .pane__suggestion', { hasText: "Release notes" }).waitFor();
  await page.locator('[data-pane-pick="2"]').click();
  await page.locator(".modal .menu__item", { hasText: "Release notes" }).getByText("Other ·").waitFor();
  await page.keyboard.press("Escape");
  await page.locator('.pane[data-pane="0"] .pane__label').click();
  await page.locator('.pane--focused[data-pane="0"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/panes.png` });
  // Maximize puts the pane's session back in one pane.
  await page.locator('[data-pane-max="0"]').click();
  assert.equal(await page.locator(".workbench__panes").count(), 0);
  await page.locator('.tab-item--selected[data-tab-open="builder"]').waitFor();
  // ⌘/ is the palette over quick commands, sending to this terminal.
  await page.keyboard.press(`${primary}+Slash`);
  await page.locator("#palette-input[placeholder^='Send a quick command']").waitFor();
  await page.locator(".palette__row", { hasText: "Run tests" }).waitFor();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "quick_command_send" && c.args.commandId === "q1" && c.args.sessionId === "builder"));
  // Quick commands are edited in place, keeping their id.
  await page.locator('.workbench__bar [data-action="quick-menu"]').click();
  await page.locator('[data-action="manage-commands"]').click();
  await page.locator('[data-command-edit="q1"]').click();
  assert.equal(await page.locator("#draft-title").inputValue(), "Run tests");
  await page.locator("#draft-title").fill("Run all tests");
  await page.locator('[data-action="add-command"]', { hasText: "Save command" }).click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "quick_command_save" && c.args.id === "q1" && c.args.title === "Run all tests"));
  await page.keyboard.press("Escape");
  // ⇧⌘P searches sessions and projects only.
  await page.keyboard.press(`${primary}+Shift+p`);
  await page.locator("#palette-input").fill("git push");
  await page.locator(".modal--palette .menu__empty", { hasText: "Nothing matches" }).waitFor();
  await page.locator("#palette-input").fill("other");
  await page.locator(".palette__row", { hasText: "Other" }).first().waitFor();
  await page.keyboard.press("Escape");
  // ⇧⌘→ moves the tab right.
  const order = () => page.evaluate(() => [...document.querySelectorAll(".tab-item")].map((tab) => tab.dataset.tabOpen));
  const before = await order();
  await page.keyboard.press(`${primary}+Shift+ArrowRight`);
  await page.waitForFunction((first) => document.querySelector(".tab-item").dataset.tabOpen !== first, before[0]);
  assert.equal((await order()).indexOf("builder"), before.indexOf("builder") + 1);
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
  // Home spans every project, as on macOS; the Dashboard sorts every session
  // by what its agent is doing.
  await page.locator('[data-action="home"]').click();
  await page.locator(".home__greeting").waitFor();
  await page.locator(".home-block", { hasText: "Needs you" }).locator('[data-open="builder"]').waitFor();
  await page.locator(".chip--warn", { hasText: "1 setup problem" }).waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/home.png` });
  await page.keyboard.press(`${primary}+Alt+d`);
  await page.locator(".home__greeting", { hasText: "Agent Dashboard" }).waitFor();
  await page.locator(".board__column", { hasText: "Needs you" }).locator('[data-open="builder"]').waitFor();
  await page.locator(".board__column", { hasText: "Idle" }).locator('[data-open="notes"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/dashboard.png` });
  await page.locator('.project-row [data-project="project"]').click();
  await page.locator(".header").waitFor();

  await page.locator('[data-tab="tasks"]').click();
  await page.locator('[data-action="import"]').click();
  await page.locator("#imp-manual").fill("ENG-7 Test imported task");
  await page.locator("#imp-model").fill("sonnet");
  await page.locator('[data-action="import-run"]:not([disabled])').click();
  // Grouped by status; the task's own form carries its model.
  await page.locator(".task-group", { hasText: "Queued" }).waitFor();
  await page.locator('.row[data-task="task"]').click();
  // Model and account sit under Advanced; the models are the agent's own.
  await page.locator('.modal [data-toggle="advanced"]').click();
  assert.equal(await page.locator("#task-model").inputValue(), "sonnet");
  assert.deepEqual(await page.locator("#task-model option").allTextContents(), ["Default", "Opus · most capable", "Sonnet · balanced"]);
  await page.locator("#task-model").selectOption("opus");
  await page.locator('[data-action="save-task"]').click();
  await page.locator('.row[data-task="task"]').waitFor();
  // On the board, a card dragged to another column takes that status.
  await page.locator('[data-task-view="board"]').click();
  await page.locator('[data-task-card="task"]').dragTo(page.locator('[data-task-column="review"]'));
  await page.locator('[data-task-column="review"] [data-task-card="task"]').waitFor();
  // Its menu sets any status, and deletes after asking.
  await page.locator('[data-task-card="task"]').click({ button: "right" });
  await page.locator('[data-menu-act="task-status:changes"]').click();
  await page.locator('[data-task-column="changes"] [data-task-card="task"]').waitFor();
  // Every project's tasks, each marked with its project.
  await page.locator('[data-task-scope="all"]').click();
  await page.locator('[data-task-card="task"] .card__project', { hasText: "Example" }).waitFor();
  await page.locator('[data-task-scope="project"]').click();
  await page.locator('[data-task-view="list"]').click();
  // Under the sessions, what the agents saved here and in worktrees that no
  // session holds yet; Resume records it as a session in its own folder and
  // starts it. There is no separate History tab.
  assert.equal(await page.locator('[data-tab="history"]').count(), 0);
  await page.locator('[data-tab="sessions"]').click();
  await page.getByRole("heading", { name: /Not in Convoy yet/ }).waitFor();
  const saved = page.locator(".row", { hasText: "Fix flaky login" });
  await saved.getByText("worktree fix-login").waitFor();
  await saved.locator("[data-history-resume]").click();
  await page.locator('.tab-item--selected[data-tab-open="imported"]').waitFor();
  const imported = await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "transcripts_import").args);
  assert.equal(imported.directory, "/fixture/worktrees/fix-login");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_start" && c.args.id === "imported"));
  await page.locator('.project-row [data-project="project"]').click();
  // Import all history: a spinner while it scans, then the conversations
  // under their projects by name, unassigned last. Matched ones start
  // checked; picking a project for an unassigned one checks it too.
  await page.locator('[data-tab="sessions"]').click();
  await page.locator('[data-action="import-all-history"]').first().click();
  await page.locator(".sweep__scanning .spinner").waitFor();
  await page.locator(".sweep__group", { hasText: "Unassigned" }).waitFor();
  assert.deepEqual(await page.locator(".sweep__group > span:first-child").allTextContents(), ["Example", "Other", "Unassigned"]);
  await page.locator(".sweep__row", { hasText: "~/fixture/Example/api · started in this project" }).waitFor();
  await page.locator(".sweep__row", { hasText: "4 file references" }).waitFor();
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 2" }).waitFor();
  assert.equal(await page.locator('[data-sweep-check="0199aaaa-0000-4000-8000-000000000013"]').isDisabled(), true);
  assert.deepEqual(await page.locator('[data-sweep-pick="0199aaaa-0000-4000-8000-000000000013"] optgroup').first().locator("option").allTextContents(), ["Alpha"]);
  await page.locator('[data-sweep-pick="0199aaaa-0000-4000-8000-000000000013"]').selectOption("alpha");
  await page.locator('[data-sweep-check="0199aaaa-0000-4000-8000-000000000012"]').uncheck();
  await page.locator('[data-sweep-group="other"]', { hasText: "Select all" }).click();
  // The search narrows the list, and Select all / Deselect all act on what
  // it shows. The field keeps focus while typing.
  await page.locator("#sweep-filter").pressSequentially("release");
  assert.deepEqual(await page.locator(".sweep__row .import__title > div:first-child").allTextContents(), ["Tidy release notes"]);
  assert.equal(await page.evaluate(() => document.activeElement?.id), "sweep-filter");
  await page.locator('[data-sweep-all="none"]').click();
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 2" }).waitFor();
  await page.locator("#sweep-filter").fill("nothing like this");
  await page.locator(".sweep__empty").waitFor();
  await page.locator("#sweep-filter").fill("");
  await page.locator('[data-sweep-all="none"]').click();
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 0" }).waitFor();
  await page.locator('[data-sweep-all="all"]').click();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/import-history.png` });
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 3" }).click();
  await page.locator(".toast", { hasText: "Imported 3 conversations" }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "history_sweep_import").args.choices), [
    { provider_id: "0199aaaa-0000-4000-8000-000000000011", project_id: "project" },
    { provider_id: "0199aaaa-0000-4000-8000-000000000012", project_id: "other" },
    { provider_id: "0199aaaa-0000-4000-8000-000000000013", project_id: "alpha" },
  ]);
  assert.equal(await page.locator(".scrim").count(), 0);
  // Deleting: Select shows a checkbox on every row, sessions and saved
  // conversations alike; a running session cannot be picked. Delete asks
  // first, then sends what was picked.
  await page.locator('[data-action="pick-start"]').click();
  await page.locator('[data-action="pick-all"]').click();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/sessions-pick.png` });
  const picked = await page.locator("[data-pick]:checked").count();
  assert.ok(picked >= 2, `picked ${picked}`);
  for (const box of await page.locator("[data-pick]:disabled").all()) assert.equal(await box.isChecked(), false);
  await page.locator('[data-action="pick-none"]').click();
  assert.equal(await page.locator("[data-pick]:checked").count(), 0);
  await page.locator('[data-pick-row="c:claude:0199aaaa-0000-4000-8000-000000000001"] .row__title').click();
  await page.locator(".pick__count", { hasText: "1 selected" }).waitFor();
  await page.locator('[data-action="pick-delete"]').click();
  await page.locator(".modal", { hasText: "Fix flaky login" }).waitFor();
  await page.locator('.modal [data-action="confirm-generic"]').click();
  await page.locator(".toast", { hasText: "Deleted 1 conversation" }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.checkCalls.findLast((c) => c.cmd === "sessions_delete").args.items), [
    { agent: "claude", provider_id: "0199aaaa-0000-4000-8000-000000000001" },
  ]);
  await page.locator('[data-action="pick-done"]').click();
  await page.locator('[data-history-delete="c:claude:0199aaaa-0000-4000-8000-000000000001"]').click();
  await page.locator('.modal [data-action="confirm-generic"]').click();
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "sessions_delete").length === 2);
  await page.locator('[data-tab="tasks"]').click();
  // Files & Changes: ahead and behind its upstream, a conflict marked
  // resolved, everything unstaged, and Commit & Push in one go.
  await page.locator('.tabbar [data-action="files"]').click();
  await page.locator(".files__sync", { hasText: "↑2 ↓1" }).waitFor();
  await page.locator('[data-change="src/app.js"]').click();
  await page.locator('[data-action="resolve"]').click();
  await page.locator('[data-action="unstage-all"]').click();
  await page.locator('[data-action="file-open"]').click();
  await page.locator("#commit-message").fill("Resolve the merge");
  await page.locator('[data-action="commit-push"]').click();
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "files_mutate").map((c) => c.args.mutation.action).join().startsWith("stage,unstageAll,commit,push"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "files_open").args), { id: "project", path: "src/app.js", reveal: false });
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/files.png` });
  // A long diff is drawn in part until asked; long lines can wrap.
  await page.locator('[data-change="README.md"]').click();
  await page.locator('[data-action="files-load-all"]', { hasText: "Load all 3,500 lines" }).click();
  await page.locator(".preview__text .diff-line", { hasText: "+line 3499" }).waitFor();
  await page.locator('[data-action="files-wrap"]').click();
  await page.locator(".preview__text--wrap").waitFor();
  // The log copies a commit's SHA or subject.
  await page.locator('[data-files-view="log"]').click();
  await page.locator('[data-commit="abc1234def5678"]').click();
  await page.locator('[data-action="copy-sha"]').click();
  if (await readClipboard() !== null) assert.equal(await readClipboard(), "abc1234def5678");
  await page.locator('[data-action="copy-subject"]').click();
  if (await readClipboard() !== null) assert.equal(await readClipboard(), "Fix the login race");
  // A new branch can start from any base.
  await page.locator('[data-files-view="branches"]').click();
  await page.locator('[data-action="new-branch"]').click();
  await page.locator("#draft-branch").fill("hotfix");
  await page.locator("#draft-base").fill("origin/main");
  await page.locator('[data-action="confirm-branch"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "files_mutate" && c.args.mutation.action === "branchFrom" && c.args.mutation.base === "origin/main" && c.args.mutation.branch === "hotfix"));
  await page.locator('[data-action="files-close"]').click();
  // Specifications: a search box, and each spec's own work — a task made for
  // it, a session linked as its builder, and the review brief.
  await page.locator('[data-tab="specs"]').click();
  await page.locator("#spec-filter").fill("nothing");
  await page.locator('.row[data-spec="spec1"]').waitFor({ state: "detached" });
  await page.locator("#spec-filter").fill("retries");
  await page.locator('.row[data-spec="spec1"]').click();
  await page.locator('[data-action="spec-new-task"]').click();
  assert.equal(await page.locator("#task-title").inputValue(), "Checkout retries");
  await page.locator('[data-action="save-task"]').click();
  await page.locator('.row[data-spec="spec1"]').click();
  const work = page.locator(".spec-work", { hasText: "Checkout retries" });
  await work.locator("select").selectOption("builder");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "task_link_session" && c.args.sessionId === "builder"));
  await work.locator("[data-spec-brief]").click();
  await page.locator(".toast", { hasText: "Review brief copied" }).waitFor();
  if (await readClipboard() !== null) assert.match(await readClipboard(), /Review the login changes/);
  await page.keyboard.press("Escape");
  // "Blocked by": the task waits for another, and says so.
  await page.locator('[data-tab="tasks"]').click();
  await page.locator('.row[data-task="task"]').click();
  await page.locator('.modal [data-depends-on]').first().check();
  await page.locator('[data-action="save-task"]').click();
  await page.locator('.row[data-task="task"] .task-blocked', { hasText: "waits for Checkout retries" }).waitFor();
  // New tasks publish as a pull request unless the project says otherwise.
  await page.locator('[data-action="new-task"]').click();
  await page.locator('.modal .choice [data-value="pr"][aria-pressed="true"]').waitFor();
  // Specs are a list to pick from, however many there are; Codex brings its
  // own models and accounts under Advanced.
  assert.deepEqual(await page.locator("#task-spec option").allTextContents(), ["None", "Checkout retries"]);
  await page.locator('.modal .choice [data-value="codex"]').click();
  await page.locator('.modal [data-toggle="advanced"]').click();
  await page.locator("#task-model option", { hasText: "GPT-6" }).waitFor({ state: "attached" });
  await page.locator("#task-account").selectOption("work");
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/task-form.png` });
  await page.keyboard.press("Escape");
  await page.locator('.row[data-task="task"] [data-task-more="task"]').click();
  await page.locator('[data-menu-act="task-delete"]').click();
  // A destructive button keeps its colour under the pointer.
  await page.locator('.modal [data-action="confirm-generic"]').hover();
  assert.equal(
    await page.locator('.modal [data-action="confirm-generic"]').evaluate((node) => getComputedStyle(node).backgroundColor),
    "rgb(239, 68, 68)",
  );
  await page.locator('.modal [data-action="confirm-generic"]').click();
  await page.locator('.row[data-task="task"]').waitFor({ state: "detached" });

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
  // The effort beside the model, from the levels Codex lists for it.
  await page.locator('#draft-effort option[value="ultra"]').waitFor({ state: "attached" });
  await page.locator("#draft-effort").selectOption("ultra");
  if (shots) await page.screenshot({ path: `${shots}/new-session.png` });
  await page.locator('[data-action="create-session"]').click();
  await page.locator('.tab-item--selected[data-tab-open="fresh"]').waitFor();
  const made = await page.evaluate(() => window.checkCalls.filter((c) => ["session_create", "worktree_create"].includes(c.cmd) || (c.cmd === "session_start" && c.args.id === "fresh")).map((c) => ({ cmd: c.cmd, ...c.args })));
  assert.deepEqual(made.map((c) => c.cmd), ["session_create", "worktree_create", "session_start"]);
  assert.equal(made[0].profileId, "work");
  assert.equal(made[0].agent, "codex");
  assert.equal(made[0].view, "terminal", "the sheet starts from Settings' default view");
  assert.equal(made[0].effort, "ultra", "the effort chosen in the sheet");
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
  // Agents: where new sessions open.
  await page.locator('[data-settings-section="Agents"]').click();
  await page.locator('[data-pref-set="default_view"][data-value="terminal"][aria-pressed="true"]').waitFor();
  await page.locator('[data-pref-set="default_view"][data-value="chat"]').click();
  await page.waitForFunction(() => window.savedSettings.default_view === "chat");
  await page.locator('[data-pref-set="default_view"][data-value="terminal"]').click();
  await page.waitForFunction(() => window.savedSettings.default_view === "terminal");
  await page.locator('[data-settings-section="Setup"]').click();
  await page.locator(".pref-row", { has: page.locator(".pref-row__title", { hasText: /^gh$/ }) }).locator(".pref-missing").waitFor();
  // Past the tools: logins and Convoy's own pieces, as on macOS.
  await page.locator(".pref-row", { hasText: "Claude login" }).locator(".pref-ok").waitFor();
  await page.locator(".pref-row", { hasText: "GitHub CLI login" }).getByText("Run gh auth login.").waitFor();
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
  // Backspace leaves a shortcut unbound; the row can go back to its default;
  // a key another action has is refused and named.
  await page.locator('[data-capture="palette"]').click();
  await page.keyboard.press("Backspace");
  await page.waitForFunction(() => window.savedSettings.shortcuts.palette === "");
  await page.locator('[data-capture="palette"]', { hasText: "None" }).waitFor();
  await page.locator('[data-shortcut-reset="palette"]').click();
  await page.waitForFunction(() => !("palette" in window.savedSettings.shortcuts));
  await page.locator('[data-capture="palette"]').click();
  await page.keyboard.press(`${primary}+b`);
  await page.locator(".toast", { hasText: "is already “Toggle sidebar”" }).waitFor();
  await page.keyboard.press("Escape");
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
  // The review brief can start from the built-in one.
  await page.locator('[data-settings-section="Agents"]').click();
  await page.locator('[data-action="insert-review-default"]').click();
  await page.waitForFunction(() => window.savedSettings.review_template === "DEFAULT BRIEF");
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

  // Projects move within their group, from the menu or by dragging.
  await page.locator('[data-menu-project="other"]').click({ button: "right" });
  await page.locator('[data-menu-act="move-up"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_move" && c.args.id === "other" && c.args.index === 0));
  await page.locator('[data-project-drag="project"]').dragTo(page.locator('[data-project-drag="other"]'));
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "project_move").length === 2);
  await page.locator('[data-menu-project="project"]').click({ button: "right" });
  await page.locator('[data-menu-act="refresh"]').click();
  await page.locator(".toast", { hasText: "No new projects in this folder" }).waitFor();
  // A group is a row that folds its projects; with the hierarchy off, they
  // sit in the list like any other.
  await page.locator('[data-group-toggle="clients"]').click();
  await page.locator('[data-project="alpha"]').waitFor({ state: "detached" });
  await page.locator('[data-group-toggle="clients"]').click();
  await page.locator('[data-project="alpha"]').waitFor();
  await page.locator('[data-action="workspace-menu"]').click();
  await page.locator('[data-workspace-option="hierarchy"]').click();
  await page.locator('[data-group-toggle="clients"]').waitFor({ state: "detached" });
  await page.locator('[data-project="alpha"]').waitFor();
  await page.locator('[data-action="workspace-menu"]').click();
  await page.locator('[data-workspace-option="hierarchy"]').click();
  await page.locator('[data-group-toggle="clients"]').waitFor();
  // The sidebar's own menu, as on macOS.
  await page.locator('[data-action="workspace-menu"]').click();
  await page.locator('[data-workspace-option="sort"]').click();
  await page.waitForFunction(() => window.savedSettings.sort_projects === true);
  await page.locator('[data-action="workspace-menu"]').click();
  await page.locator('[data-workspace-option="sort"]').click();
  await page.waitForFunction(() => window.savedSettings.sort_projects === false);
  // ⌘-click picks sessions; the picked ones are archived together.
  await page.locator('.side-session[data-open="review"]').click({ modifiers: [primary] });
  await page.locator(".sidebar__selection", { hasText: "1 selected" }).waitFor();
  await page.locator('[data-action="selection-archive"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_archive" && c.args.id === "review" && c.args.archived === true));
  await page.locator(".sidebar__selection").waitFor({ state: "detached" });
  // The status bar says which account new Codex sessions use, and changes it.
  await page.locator('.status [data-action="account-menu"][data-agent="codex"]').click();
  await page.locator('[data-account-pick="codex:"]').click();
  assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("convoy.activeAccount")).codex), "");
  await page.locator('.status [data-action="account-menu"][data-agent="codex"]', { hasText: "System" }).waitFor();
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
  // A conversation the agent cannot find offers a fresh start, which runs.
  await page.locator(".session-banner", { hasText: "couldn't find this conversation" }).waitFor();
  await page.locator('[data-action="start-fresh"]').click();
  await page.locator('.tab-item--selected[data-tab-open="notes-fresh"]').waitFor();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_start" && c.args.id === "notes-fresh"));
  // What a running terminal shows is kept.
  await page.evaluate(() => window.emitTauri("terminal:data", { id: "builder", data: "All 42 tests pass\r\n" }));
  await page.waitForFunction(() => window.snapshots?.builder?.includes("All 42 tests pass"), null, { timeout: 10000 });

  const calls = await page.evaluate(() => window.checkCalls);
  assert(calls.some(c => c.cmd === "terminal_paste" && c.args.id === "builder" && c.args.text === "Fix the error path before shipping." && c.args.submit === false));
  assert(calls.some(c => c.cmd === "issues_import" && c.args.options.model === "sonnet"));
  assert(calls.some(c => c.cmd === "task_agent" && c.args.model === "opus"));
  assert.deepEqual(calls.filter((c) => c.cmd === "task_status").map((c) => c.args.status), ["review", "changes"]);
  // Remove all projects is in the sidebar's ⋯ menu, and waits while an agent runs.
  await page.locator('[data-action="workspace-menu"]').click();
  await page.locator('[data-action="remove-all-projects"]').click();
  const removeAll = page.locator(".modal", { hasText: "Remove all projects?" });
  await removeAll.getByText("Files, git repositories, worktrees").waitFor();
  await removeAll.locator(".modal__hint--warn", { hasText: "Stop the running sessions first" }).waitFor();
  assert.equal(await removeAll.locator('[data-action="confirm-remove-all-projects"]').isDisabled(), true);
  await removeAll.getByRole("button", { name: "Cancel" }).click();
  await removeAll.waitFor({ state: "detached" });
  assert(!(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "projects_remove_all"))));

  // ------------------------------------------------------------- chat --
  // A session shown as a chat: history once, Resume starts chat mode, the
  // agent's events draw messages and cards, an approval is answered, the
  // composer sends, and switching to the terminal restarts it there.
  await page.evaluate(() => {
    window.chatHistory = { chatty: [
      { type: "user", id: "h1", text: "What does the login page do?" },
      { type: "text_done", id: "h2", text: "It signs you in with **email** and a one-time code." },
    ] };
    window.mockSessions.push({ id: "chatty", project_id: "project", title: "Tidy the login page", agent: "claude", provider_id: "p2", started: true, running: false, pinned: false, review_of: null, view: "chat" });
  });
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-open="chatty"]').first().click();
  const chat = page.locator('.chat[data-chat="chatty"]');
  await chat.locator(".msg--user", { hasText: "What does the login page do?" }).waitFor();
  await chat.locator(".msg--assistant strong", { hasText: "email" }).waitFor();
  assert.equal(await page.locator('[data-action="session-view"][data-view="chat"]').getAttribute("aria-pressed"), "true");
  assert.equal(await page.locator("#terminal-host").count(), 0, "a chat session has no terminal");
  await page.locator('.workbench__bar [data-start="chatty"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_start" && c.args.id === "chatty"));
  await page.locator('.workbench__bar [data-start="chatty"]').waitFor({ state: "detached" });
  assert(!(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "session_start" && c.args.id === "chatty"))), "a chat session starts in chat mode");
  const emitChat = (events) => page.evaluate((list) => list.forEach((event) => window.emitTauri("chat:event", { id: "chatty", event })), events);
  // Model and effort chips: the account's list until the agent reports its
  // own, then the agent's; changing either is saved and sent at once.
  const modelChip = chat.locator("[data-chat-model]");
  const effortChip = chat.locator("[data-chat-effort]");
  await modelChip.locator('option[value="sonnet"]').waitFor({ state: "attached" });
  assert.deepEqual(await effortChip.locator("option").evaluateAll((list) => list.map((option) => option.value)), ["", "low", "medium", "high", "xhigh", "max"], "every level the account's models take, before the agent reports");
  assert(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "chat_models" && c.args.id === "chatty")), "a reopened window asks for the last list");
  await emitChat([{ type: "models", model: null, effort: null, models: [
    { id: "opus", name: "Opus 5.5", efforts: ["low", "medium", "high", "xhigh", "max"], default_effort: null, is_default: true },
    { id: "claude-haiku-4-5", name: "Haiku 4.5", efforts: [], default_effort: null, is_default: false },
    { id: "claude-sonnet-4-6", name: "Sonnet 4.6", efforts: ["low", "medium", "high", "max"], default_effort: null, is_default: false },
  ] }]);
  await modelChip.locator('option[value="claude-sonnet-4-6"]').waitFor({ state: "attached" });
  assert.deepEqual(await modelChip.locator("option").evaluateAll((list) => list.map((option) => option.textContent)), ["Default · Opus 5.5", "Opus 5.5", "Haiku 4.5", "Sonnet 4.6"]);
  await modelChip.selectOption("claude-sonnet-4-6");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_set_model"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "chat_set_model").args), { id: "chatty", model: "claude-sonnet-4-6" });
  await effortChip.locator('option[value="xhigh"]').waitFor({ state: "detached" });
  await effortChip.selectOption("max");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_set_effort"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "chat_set_effort").args), { id: "chatty", effort: "max" });
  assert.equal(await effortChip.inputValue(), "max");
  // A model with no effort levels has no effort chip, and the level it
  // cannot take goes back to the default.
  await modelChip.selectOption("claude-haiku-4-5");
  await effortChip.waitFor({ state: "detached" });
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "chat_set_effort").length === 2);
  assert.deepEqual(await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "chat_set_effort").at(-1).args), { id: "chatty", effort: null });
  await modelChip.selectOption("");
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "chat_set_model").at(-1).args.model === null);
  await effortChip.waitFor();
  await emitChat([
    { type: "user", id: "u1", text: "Add a test for the code step" },
    { type: "turn_started" },
    { type: "thinking", id: "k1", delta: "The code step lives in login.js; a unit test is enough." },
    { type: "text", id: "a1", delta: "I'll add a test " },
    { type: "text", id: "a1", delta: "and run the suite." },
    { type: "tool", id: "t1", kind: "command", name: "Bash", title: "Run `npm test`", detail: "npm test -- login" },
    { type: "tool_result", id: "t1", ok: true, output: "PASS login.test.js\n  ✓ signs in with a code (12 ms)\n\nTests: 1 passed" },
    { type: "file_change", id: "f1", path: "src/login.test.js", diff: "--- a/src/login.test.js\n+++ b/src/login.test.js\n@@ -1,3 +1,6 @@\n import { login } from './login.js';\n-test.todo('code');\n+test('signs in with a code', async () => {\n+  expect(await login('a@b.c', '123456')).toBe(true);\n+});" },
    { type: "approval", request: "r1", kind: "command", title: "Run `git commit -m \"Test the code step\"`", detail: "git commit -m \"Test the code step\"", always: true },
  ]);
  await chat.locator(".msg--assistant", { hasText: "I'll add a test and run the suite." }).waitFor();
  await page.locator(".workbench__bar .state--waiting").waitFor();
  await chat.locator(".chat-working--waiting").waitFor();
  const tool = chat.locator(".tool--command");
  assert.equal(await tool.getAttribute("open"), null, "tool cards start folded");
  await tool.locator(".tool__title code", { hasText: "npm test" }).click();
  await tool.locator(".tool__output", { hasText: "Tests: 1 passed" }).waitFor();
  await tool.locator(".tool__detail", { hasText: "npm test -- login" }).waitFor();
  await chat.locator(".tool--file .diff-line--add", { hasText: "signs in with a code" }).waitFor();
  assert.equal(await chat.locator(".tool--file .tool__added").textContent(), "+3");
  const approval = chat.locator(".approval", { hasText: "git commit" });
  assert.equal(await approval.locator("[data-chat-answer]").count(), 3, "Allow, Allow for session, Deny");
  // A redraw from elsewhere keeps an opened card open.
  await page.evaluate(() => window.emitTauri("session:changed", "chatty"));
  await page.waitForTimeout(100);
  assert.notEqual(await tool.getAttribute("open"), null);
  if (shots) {
    await page.setViewportSize({ width: 1280, height: 1360 });
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/chat-${theme}.png` });
    }
    await page.setViewportSize({ width: 1280, height: 840 });
  }
  await approval.locator('[data-chat-answer="always"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_answer"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "chat_answer").args), { id: "chatty", request: "r1", decision: "always" });
  await emitChat([{ type: "approval_resolved", request: "r1" }]);
  await approval.locator(".approval__answer", { hasText: "Allowed for this session" }).waitFor();
  await page.locator(".workbench__bar .state--running").waitFor();
  await emitChat([
    { type: "text", id: "a2", delta: "Committed. The suite passes." },
    { type: "turn_ended", usage: { input: 1200, output: 340, cache_read: 0, cache_write: 0 }, error: null },
  ]);
  await page.locator(".workbench__bar .state--done").waitFor();
  await chat.locator(".chat-working").waitFor({ state: "detached" });
  await chat.locator(".composer__send").waitFor();
  // The composer keeps what is typed through a redraw, and Enter sends.
  const composer = chat.locator("#chat-input-chatty");
  await composer.fill("Now tidy the error messages");
  await page.evaluate(() => window.emitTauri("session:changed", "chatty"));
  await page.waitForTimeout(100);
  assert.equal(await composer.inputValue(), "Now tidy the error messages");
  await composer.press("Enter");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_send"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.find((c) => c.cmd === "chat_send").args), { id: "chatty", text: "Now tidy the error messages", images: [] });
  assert.equal(await composer.inputValue(), "");
  await chat.locator(".msg--user", { hasText: "Now tidy the error messages" }).waitFor();
  // The echo takes the place of the message put up while sending.
  await emitChat([{ type: "user", id: "u2", text: "Now tidy the error messages" }, { type: "turn_started" }]);
  await chat.locator(".composer__stop").click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_interrupt" && c.args.id === "chatty"));
  await emitChat([{ type: "turn_ended", usage: null, error: "Interrupted by the user" }]);
  await chat.locator(".chat-error", { hasText: "Interrupted by the user" }).waitFor();
  assert.equal(await chat.locator(".msg--user", { hasText: "Now tidy the error messages" }).count(), 1);
  if (shots) {
    await page.setViewportSize({ width: 1280, height: 1360 });
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/chat-after-${theme}.png` });
    }
    await page.setViewportSize({ width: 1280, height: 840 });
  }
  // A quick command that submits is sent as a message, not typed into a PTY.
  await chat.locator('.composer__bar [data-action="quick-menu"]').click();
  const quickBox = await page.locator(".menu--anchored").boundingBox();
  assert(quickBox.y + quickBox.height <= 840, "the composer's menu opens upwards, inside the window");
  await page.locator('[data-quick="q1"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_send" && c.args.text === "npm test"));
  assert.equal(await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "quick_command_send" && c.args.sessionId === "chatty").length), 0);
  // Switching a running chat to the terminal asks, stops, saves the view and
  // starts it again in the terminal.
  await page.locator('[data-action="session-view"][data-view="terminal"]').click();
  await page.locator(".modal", { hasText: "The agent stops and resumes in Terminal" }).getByRole("button", { name: "Switch" }).click();
  await page.locator("#terminal-host").waitFor();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_start" && c.args.id === "chatty"));
  const switched = await page.evaluate(() => window.checkCalls.filter((c) => ["session_stop", "session_view", "session_start"].includes(c.cmd) && c.args.id === "chatty").map((c) => [c.cmd, c.args.view]));
  assert.deepEqual(switched, [["session_stop", undefined], ["session_view", "terminal"], ["session_start", undefined]]);
  assert.equal(await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "chat_history").length), 1, "history is read once");

  // ---------------------------------------------------------- approvals inbox --
  // Two chat sessions ask — one never opened in this window — and a terminal
  // session waits: one inbox, newest first, answered in place, in step with
  // the chat's own card.
  await page.evaluate(() => {
    window.mockSessions.push(
      { id: "chat-a", project_id: "project", title: "Ship the release", agent: "claude", provider_id: "pa", started: true, running: true, pinned: false, review_of: null, view: "chat" },
      { id: "chat-b", project_id: "other", title: "Migrate the database", agent: "codex", provider_id: "pb", started: true, running: true, pinned: false, review_of: null, view: "chat" },
    );
  });
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-open="chat-a"]').first().click();
  const chatA = page.locator('.chat[data-chat="chat-a"]');
  await chatA.waitFor();
  const emitTo = (id, events) => page.evaluate(([target, list]) => list.forEach((event) => window.emitTauri("chat:event", { id: target, event })), [id, events]);
  await emitTo("chat-a", [{ type: "turn_started" }, { type: "approval", request: "r10", kind: "command", title: "Run `npm run release`", detail: "npm run release -- --tag next", always: true }]);
  await page.waitForTimeout(20);
  await emitTo("chat-b", [{ type: "turn_started" }, { type: "approval", request: "r20", kind: "edit", title: "Edit db/migrate.sql", detail: "--- a/db/migrate.sql\n+++ b/db/migrate.sql\n@@ -1 +1,2 @@\n ALTER TABLE users ADD COLUMN code text;\n+CREATE INDEX users_code ON users (code);", always: false }]);
  await page.locator(".inbox-button__badge", { hasText: "3" }).waitFor();
  // More than one: "need you" opens the inbox rather than one session.
  await page.locator(".status__needs", { hasText: "3 need you" }).click();
  await inbox.waitFor();
  const rows = inbox.locator(".inbox-row");
  assert.equal(await rows.count(), 3);
  const places = await rows.locator(".inbox-row__session").allTextContents();
  assert.deepEqual(places, ["Migrate the database", "Ship the release", "Fix login"], "newest first");
  const rowA = rows.filter({ hasText: "Ship the release" });
  const rowB = rows.filter({ hasText: "Migrate the database" });
  const rowTerminal = rows.filter({ hasText: "Fix login" });
  assert.equal((await rowB.locator(".inbox-row__project").textContent()).trim(), "Other");
  assert.equal(await rowA.locator("[data-inbox-answer]").count(), 3, "Allow, Allow for session, Deny");
  assert.equal(await rowB.locator("[data-inbox-answer]").count(), 2, "no session-wide answer offered");
  // A terminal session can only be opened: it answers in its own terminal.
  assert.equal(await rowTerminal.locator("[data-inbox-answer]").count(), 0);
  await rowTerminal.locator(".inbox-row__hint", { hasText: "answered in its own terminal" }).waitFor();
  await rowTerminal.locator("[data-inbox-open]").waitFor();
  // The short detail expands to the whole of it.
  assert.equal(await rowB.locator(".inbox-row__full").count(), 0);
  await rowB.locator("[data-inbox-expand]").click();
  await rowB.locator(".inbox-row__full", { hasText: "CREATE INDEX users_code" }).waitFor();
  assert.equal(await rowB.locator("[data-inbox-expand]").getAttribute("aria-expanded"), "true");
  if (shots) {
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(200);
      await page.screenshot({ path: `${shots}/inbox-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  }
  // Allow from the inbox: busy here and on the chat's card until resolved.
  await rowA.locator('[data-inbox-answer="allow"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_answer" && c.args.id === "chat-a"));
  assert.deepEqual(await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "chat_answer").at(-1).args), { id: "chat-a", request: "r10", decision: "allow" });
  assert(await rowA.locator('[data-inbox-answer="allow"]').isDisabled(), "busy until resolved");
  assert(await chatA.locator('.approval [data-chat-answer="allow"]').isDisabled(), "the chat's card is busy too");
  await emitTo("chat-a", [{ type: "approval_resolved", request: "r10" }]);
  await rowA.waitFor({ state: "detached" });
  await chatA.locator(".approval .approval__answer", { hasText: "Allowed" }).waitFor();
  await page.locator(".inbox-button__badge", { hasText: "2" }).waitFor();
  // The keyboard reaches the rows; Esc closes the inbox.
  await inbox.focus();
  await page.keyboard.press("Tab");
  assert(await page.evaluate(() => !!document.activeElement?.closest(".inbox")), "Tab moves into the inbox");
  await page.keyboard.press("Escape");
  await inbox.waitFor({ state: "detached" });
  // The palette has it, and so does a shortcut.
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("Approvals");
  await page.locator(".palette__row", { hasText: "Approvals…" }).first().waitFor();
  await page.keyboard.press("Enter");
  await inbox.waitFor();
  await page.keyboard.press("Escape");
  await inbox.waitFor({ state: "detached" });
  await page.keyboard.press(`${mod}+Alt+a`);
  await inbox.waitFor();
  // Open goes to a session this window had never shown, its card waiting.
  await rowB.locator("[data-inbox-open]").click();
  await inbox.waitFor({ state: "detached" });
  const chatB = page.locator('.chat[data-chat="chat-b"]');
  await chatB.locator(".approval", { hasText: "db/migrate.sql" }).waitFor();
  // Denied from the card: the inbox follows, and with one thing left the
  // status bar jumps straight to it.
  await chatB.locator('[data-chat-answer="deny"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "chat_answer" && c.args.id === "chat-b" && c.args.decision === "deny"));
  await emitTo("chat-b", [{ type: "approval_resolved", request: "r20" }]);
  await page.locator(".inbox-button__badge", { hasText: "1" }).waitFor();
  await page.locator('.status__needs[data-needs-you="builder"]', { hasText: "1 needs you" }).waitFor();

  // Every string the inbox shows has its Russian.
  const ru = (await import("../src/locales/ru.js")).default.strings;
  const { readFile } = await import("node:fs/promises");
  const inboxSource = await readFile(new URL("../src/views-inbox.js", import.meta.url), "utf8");
  const keys = [...inboxSource.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)].map((match) => match[1]);
  keys.push("Approvals…", "What the agents are waiting on you for, in every project");
  assert(keys.length > 15, "the inbox's strings were found");
  assert.deepEqual(keys.filter((key) => !(key in ru)), [], "untranslated inbox strings");
  assert.equal(await page.locator(".toast--error").count(), 0);
  assert.deepEqual(errors, []);
  assert.equal(await page.locator(".toast--error").count(), 0);
  if (process.env.CONVOY_TEST_SCREENSHOT) await page.screenshot({ path: process.env.CONVOY_TEST_SCREENSHOT });
  console.log("PASS: folder → builder → review → feedback; Linear MCP import → per-task model; sidebar menus; settled dialogs; settings page saves; chat session events, approval, composer and view switch; approvals inbox across sessions; no browser errors. IPC mocked.");
} finally {
  await browser?.close();
  await server.close();
}
