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
    const sessions = [{ id: "builder", title: "Fix login", agent: "claude", provider_id: "id", started: true, running: true, review_of: null }];
    const calls = [];
    window.checkCalls = calls;
    window.__TAURI_INTERNALS__ = { transformCallback: () => 1, invoke: async (cmd, args) => {
      calls.push({ cmd, args });
      window.savedSettings ??= { theme: "light", default_agent: "claude", font_size: 13, scrollback: 10000, claude_usage: false, notifications: false, keep_awake: "off", hibernate_minutes: 0, shortcuts: {} };
      if (cmd === "settings_read") return { ...window.savedSettings, storage: "/fixture" };
      if (cmd === "settings_save") { window.savedSettings = { ...args.input }; return null; }
      if (cmd === "workspace_read") return { projects, running: ["builder"], storage: "/fixture/workspace.json" };
      if (cmd === "sessions_for") return sessions;
      if (cmd === "planning_read") return { tasks, specs: [], queued: tasks.length };
      if (cmd === "tasks_for") return tasks;
      if (["quick_commands_read", "profiles_read", "activity_read"].includes(cmd)) return [];
      if (cmd === "monitor_tick") return { states: [], hibernate: [], changed: false };
      if (cmd === "plugin:dialog|open") return "/fixture/Example";
      if (cmd === "project_open") { projects.push({ id: "project", title: "Example", path: args.path, sessions: 1, running: 1 }); return 1; }
      if (cmd === "git_status") return { branch: "main", changed_files: 1 };
      if (cmd === "review_brief") return "Review the login changes. Tests pass.";
      if (cmd === "review_create") { sessions.push({ id: "review", title: "Review: Fix login", agent: "codex", provider_id: "", started: false, running: false, review_of: "builder" }); return "review"; }
      if (cmd === "review_builder") return "builder";
      if (cmd === "integrations_list") return [{ id: "linear", kind: "linear", auth: "mcp", name: "Linear MCP", has_secret: false }];
      if (cmd === "issues_parse_keys") return [{ key: "ENG-7", title: "Test imported task", details: "", origin: "linear:test" }];
      if (cmd === "issues_import") {
        tasks.push({ id: "task", title: "ENG-7: Test imported task", details: "", findings: "", agent: args.options.agent, model: args.options.model, status: "queued", mode: "none", auto_review: false, source: { tracker: "linear", key: "ENG-7", origin: "linear:test" } });
        return { created: ["task"], skipped: 0 };
      }
      if (cmd === "task_agent") { tasks[0].model = args.model; tasks[0].agent = args.agent; return null; }
      if (cmd === "plugin:event|listen") return 1;
      if (["keep_awake", "terminal_resize", "terminal_write", "terminal_paste"].includes(cmd)) return null;
      throw new Error(`Unmocked IPC: ${cmd}`);
    } };
  });
  await page.goto("http://127.0.0.1:1421");
  await page.locator('[data-action="open-folder"]').first().click();
  await page.getByRole("heading", { name: "Example", exact: true }).waitFor();
  await page.locator('[data-open="builder"]').first().click();
  await page.locator('[data-action="session-menu"]').click();
  await page.locator('[data-action="start-review"]').click();
  await page.locator('[data-action="create-review"]').click();
  await page.locator('[data-action="session-menu"]').click();
  await page.locator('[data-action="send-feedback"]').click();
  await page.locator("#draft-feedback").waitFor();
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await page.evaluate(() => document.activeElement.id), "draft-feedback", "terminal must not steal dialog focus");
  await page.locator("#draft-feedback").fill("Fix the error path before shipping.");
  await page.locator('[data-action="insert-feedback"]').click();
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
  await page.locator('.header [data-action="new-session"]').click();
  await page.locator('.choice [data-value="codex"]').click();
  await page.locator(".scrim--settled .modal").waitFor();
  assert.equal(await page.evaluate(() => document.querySelector(".modal").getAnimations().length), 0, "no animation after a toggle");
  assert(await page.locator('.choice [data-value="codex"] .agent-icon').count(), "agent choices carry their marks");
  if (shots) await page.screenshot({ path: `${shots}/new-session.png` });
  await page.keyboard.press("Escape");

  // Settings is a page, and a change is saved as soon as it is made.
  await page.locator(".sidebar__gear").click();
  await page.locator(".prefs__title", { hasText: "General" }).waitFor();
  await page.locator('[data-settings-section="Appearance"]').click();
  await page.locator('[data-pref-set="theme"][data-value="dark"]').click();
  await page.waitForFunction(() => window.savedSettings.theme === "dark");
  await page.locator("#settings-search").fill("jira");
  assert.equal(await page.locator(".prefs__item").count(), 1, "search narrows the sections");
  await page.locator('[data-settings-section="Linear & Jira"]').click();
  await page.locator(".pref-row", { hasText: "Linear MCP" }).waitFor();
  if (shots) await page.screenshot({ path: `${shots}/settings.png` });
  await page.keyboard.press("Escape");
  await page.locator(".sidebar").waitFor();

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
