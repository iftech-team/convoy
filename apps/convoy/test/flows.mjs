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
    window.mockTasks = tasks;
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
      if (cmd === "integrations_list") return [{ id: "linear", kind: "linear", auth: "mcp", name: "Linear MCP", has_secret: false, levels: ["Team", "Project"] }, { id: "peshbar", kind: "peshbar", auth: "apiKey", name: "Peshbar", has_secret: true, levels: ["Workspace", "Project"] }, { id: "notion", kind: "notion", auth: "apiKey", name: "Notion", has_secret: true, levels: ["Database", null] }];
      if (cmd === "project_link_get") return window.links?.[args.projectId] ?? null;
      if (cmd === "tracker_statuses") return {
        statuses: [{ name: "To do", group: "todo" }, { name: "In progress", group: "started" }, { name: "In review", group: "started" }, { name: "QA", group: "started" }, { name: "Done", group: "done" }],
        suggested: { building: { name: "In progress" }, review: { name: "In review" }, pr: { name: "In review" }, testing: { name: "QA" }, changes: { name: "In progress" }, done: { name: "Done" } },
      };
      if (cmd === "task_sync_retry") { const task = tasks.find((item) => item.id === args.id); task.sync = { pending: task.sync.failed, last: task.sync.last }; return null; }
      if (cmd === "project_link_save") { (window.links ??= {})[args.projectId] = args.link; return null; }
      if (cmd === "integration_scopes") return args.parent ? [{ id: "3", name: "Amir cargo", key: "AMR" }] : [{ id: "9", name: "Tojiron" }, { id: "4", name: "Peshbar" }, { id: "5", name: "Logistics" }];
      if (cmd === "issues_search") return [{ key: "AMR-1", title: "Weigh parcels at intake", details: "", origin: "management-api.peshbar.com", status: "To do" }];
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
      if (cmd === "task_related") {
        for (const task of tasks) {
          if (task.id === args.id) task.related = args.related;
          else if (args.related.includes(task.id)) task.related = [...new Set([...(task.related ?? []), args.id])];
          else task.related = (task.related ?? []).filter((other) => other !== args.id);
        }
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
      if (cmd === "sessions_remove") return [args.ids.length, 0];
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
                 color: p.color ?? "", default_agent: p.default_agent ?? "", base_ref: "", branch_prefix: "", task_mode: "", auto_run_tasks: false,
                 test_commands: p.test_commands ?? "", qa_agent: p.qa_agent ?? "", qa_auto: !!p.qa_auto, qa_timeout: p.qa_timeout ?? 15 };
      }
      if (cmd === "project_edit") {
        const p = projects.find((item) => item.id === args.id);
        Object.assign(p, args.input);
        p.qa = !!(p.test_commands || p.qa_agent);
        return null;
      }
      if (cmd === "qa_suggest_commands") return ["npm test", "npm run lint"];
      if (cmd === "qa_start") return window.qaStart(args.taskId);
      if (cmd === "pipeline_after_exit") return window.pipelineExit?.(args.id) ?? [];
      if (cmd === "pipeline_tick") return window.pipelineTick?.() ?? [];
      if (cmd === "queue_next") return window.queueNext?.() ?? [];
      if (cmd === "task_prepare") return window.prepared?.(args.id) ?? null;
      if (cmd === "queue_summary") return tasks.filter((item) => item.status === "queued").map((item) => item.title);
      if (cmd === "plan_approve") { const task = tasks.find((item) => item.id === args.taskId); task.plan.status = "approved"; task.status = "queued"; return null; }
      if (cmd === "plan_request_changes") { const task = tasks.find((item) => item.id === args.taskId); task.plan.status = "drafting"; task.status = "planning"; return [{ kind: "send", task: task.id, session: "builder", text: `Revise the plan: ${args.comment}` }]; }
      if (cmd === "task_plan") { Object.assign(tasks.find((item) => item.id === args.id), { plan_first: args.planFirst, plan_auto_approve: args.autoApprove }); return null; }
      if (cmd === "project_git") return { branch: "main", changed_files: 2 };
      if (cmd === "git_refs") return args.projectId === "project" ? ["main", "feature/x", "origin/main", "tag:v1.0"] : [];
      if (cmd === "profiles_read") return [{ id: "work", label: "Work", agent: "codex", sessions: 0 }];
      if (cmd === "session_create") {
        // A manager gets an id of its own, and keeps its view so it runs as one.
        const id = args.manager ? "lead" : sessions.some((item) => item.id === "fresh") ? `fresh${sessions.length}` : "fresh";
        sessions.push({ id, project_id: args.projectId, title: args.title, agent: args.agent, provider_id: "", started: false, running: false, pinned: false, review_of: null, manager: !!args.manager, skills: args.skills ?? [], ...(args.manager ? { view: args.view } : {}) });
        return id;
      }
      if (cmd === "worktree_create") return { branch: args.branch, shared_paths: [], setup_command: null, directory: "/fixture/worktrees/x" };
      if (cmd === "plugin:window|set_badge_count") { window.badge = args.value; return null; }
      if (cmd === "session_output") return { review: "Found: a token refresh race", notes: "No conversation found with session ID 0199", busy: "● Pushed to main as 0fc2782.\n\nWhat changed\n- Sign-up with an emailed code\n- Password reset\n\n❯ " }[args.id] ?? "";
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
      if (cmd === "session_edit") return null;
      if (cmd === "session_view") { sessions.find((item) => item.id === args.id).view = args.view; return null; }
      if (cmd === "session_start") { sessions.find((item) => item.id === args.id).running = true; return null; }
      if (cmd === "keep_awake") return args.wanted;
      if (["terminal_resize", "terminal_write", "terminal_paste"].includes(cmd)) return null;
      // Automations: kept here as the backend keeps them; the times are an
      // hour, a day and two days on, whatever the schedule.
      window.automations ??= [];
      const soon = () => Math.floor(Date.now() / 1000) + 3600;
      if (cmd === "automations_list") return window.automations.filter((item) => !args.projectId || item.project_id === args.projectId).map((item) => ({ ...item, next_run: item.enabled ? soon() : null }));
      if (cmd === "automation_preview") {
        if (args.schedule.kind === "weekdays" && !args.schedule.days.length) throw "Pick at least one day.";
        return [soon(), soon() + 86400, soon() + 2 * 86400];
      }
      if (cmd === "automation_save") {
        const existing = window.automations.find((item) => item.id === args.input.id);
        if (existing) { Object.assign(existing, args.input, { profile_id: args.input.profile_id }); return existing.id; }
        const id = `auto${window.automations.length + 1}`;
        window.automations.push({ ...args.input, id, created: Date.now() / 1000, history: [], last_run: null, running: false });
        return id;
      }
      if (cmd === "automation_enable") { window.automations.find((item) => item.id === args.id).enabled = args.enabled; return null; }
      if (cmd === "automation_delete") { window.automations = window.automations.filter((item) => item.id !== args.id); return null; }
      if (cmd === "automation_run_now") {
        const item = window.automations.find((entry) => entry.id === args.id);
        item.last_run = { at: Math.floor(Date.now() / 1000), session_id: "builder", status: "started", manual: true };
        item.running = true;
        return "builder";
      }
      // Skills: kept here as the backend keeps them.
      window.skills ??= [];
      if (cmd === "skills_list") return window.skills.map((item) => ({ ...item }));
      if (cmd === "skill_save") {
        const input = args.input;
        if (!input.name.trim()) throw "Give the skill a name.";
        const existing = window.skills.find((item) => item.id === input.id);
        const record = { name: input.name.trim(), description: input.description.trim(), body: input.body.trim(), project_id: input.project_id || null, agents: input.agents, auto: !!input.auto };
        if (existing) { Object.assign(existing, record); return existing.id; }
        const id = `skill${window.skills.length + 1}`;
        window.skills.push({ id, ...record, created: 1, updated: 1 });
        return id;
      }
      if (cmd === "skills_discover") return [
        { name: "eskhata-qa", description: "Test an Eskhata task on the local stack.", path: "/home/me/.claude/skills/eskhata-qa/SKILL.md", project_id: null, source: "Claude Code", imported: false },
        { name: "api-style", description: "How the API is written.", path: "/fixture/Example/.claude/skills/api-style/SKILL.md", project_id: "project", source: "Agent Skills", imported: true },
      ];
      if (cmd === "skills_import") return args.paths.length;
      if (cmd === "skill_delete") {
        window.skills = window.skills.filter((item) => item.id !== args.id);
        for (const item of sessions) if (item.skills) item.skills = item.skills.filter((id) => id !== args.id);
        return null;
      }
      if (cmd === "session_skills") { sessions.find((item) => item.id === args.id).skills = args.skills; return null; }
      // Usage: a report shaped as the backend's, built from the arguments so
      // a range or filter change shows; prices kept here as the file would.
      if (cmd === "usage_stats") {
        const pad = (n) => String(n).padStart(2, "0");
        const iso = (date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
        const today = new Date();
        const back = (n) => new Date(today.getFullYear(), today.getMonth(), today.getDate() - n);
        const wave = (n, seed) => Math.max(0, Math.round((Math.sin(n * 1.7 + seed) + 1.1) * ((n + today.getDay()) % 7 === 0 || (n + today.getDay()) % 7 === 1 ? 0.2 : 1) * 9e6));
        const empty = !!window.usageEmpty;
        const wantClaude = args.agent !== "codex", wantCodex = args.agent !== "claude";
        const share = args.projectId === "other" ? 0.2 : args.projectId ? 0.6 : 1;
        const days = Array.from({ length: args.days }, (_, i) => {
          const n = args.days - 1 - i;
          const claude = empty || !wantClaude ? 0 : Math.round(wave(n, 0) * share);
          const codex = empty || !wantCodex ? 0 : Math.round(wave(n, 2) * 0.35 * share);
          return { day: iso(back(n)), claude, codex, cost: (claude * 0.6) / 1e6 };
        });
        const heatmap = Array.from({ length: 84 }, (_, i) => ({ day: iso(back(83 - i)), total: empty ? 0 : (wantClaude ? wave(83 - i, 0) : 0) + (wantCodex ? Math.round(wave(83 - i, 2) * 0.35) : 0) }));
        const sum = (key) => days.reduce((total, day) => total + day[key], 0);
        const claude = sum("claude"), codex = sum("codex");
        const split = (total, cost, agent, model) => ({ agent, model, total, cost,
          tokens: { input: Math.round(total * 0.01), output: Math.round(total * 0.03), cache_read: Math.round(total * 0.9), cache_write: Math.round(total * 0.06), cache_write_1h: Math.round(total * 0.06), reasoning: Math.round(total * 0.01), requests: Math.round(total / 9e4) } });
        const models = [
          wantClaude && split(Math.round(claude * 0.8), (claude * 0.8 * 0.7) / 1e6, "claude", "claude-opus-5-5"),
          wantCodex && split(codex, null, "codex", "gpt-6-astra"),
          wantClaude && split(Math.round(claude * 0.2), (claude * 0.2 * 0.2) / 1e6, "claude", "claude-sonnet-5-5"),
        ].filter((row) => row && row.total > 0).sort((a, b) => b.total - a.total);
        const total = claude + codex;
        const cost = models.reduce((value, row) => value + (row.cost ?? 0), 0);
        const projectRow = (project_id, part, partial) => ({ project_id, total: Math.round(total * part), claude: Math.round(claude * part), codex: Math.round(codex * part), cost: cost * part, partial, tokens: models[0]?.tokens });
        const projects = total ? (args.projectId ? [projectRow(args.projectId, 1, wantCodex)] : [projectRow("project", 0.6, true), projectRow("other", 0.25, false), projectRow(null, 0.15, true)]) : [];
        const tokens = models.reduce((acc, row) => { for (const key in row.tokens) acc[key] += row.tokens[key]; return acc; }, { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0, reasoning: 0, requests: 0 });
        return { days, heatmap, models, projects, tokens, total, cost, unpriced: models.filter((row) => row.cost === null).map((row) => row.model),
                 active_days: days.filter((day) => day.claude + day.codex > 0).length, top_model: models[0]?.model ?? null, files: 550,
                 scanning: !!window.usageScanning, stale: empty, prices_error: null };
      }
      window.prices ??= [
        { model: "claude-opus-5-5", input: 4, output: 20, cache_read: 0.2, cache_write: 5, cache_write_1h: 8 },
        { model: "claude-sonnet-5-5", input: 2, output: 10, cache_read: 0.2, cache_write: 2.5, cache_write_1h: 4 },
      ];
      if (cmd === "usage_prices") return { prices: window.prices, defaults: [] };
      if (cmd === "usage_prices_save") { window.prices = args.prices.map((row) => ({ ...row, model: row.model.trim().toLowerCase() })); return window.prices; }
      if (cmd === "usage_prices_reset") { window.prices = [{ model: "claude-opus-5-5", input: 4, output: 20, cache_read: 0.2, cache_write: 5, cache_write_1h: 8 }]; return window.prices; }
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
  // The project's page, opened first, keeps a tab of its own.
  await page.locator('.tab-item--project[data-tab-open^="project:"]').first().waitFor();
  const primary = process.platform === "darwin" ? "Meta" : "Control";
  const builderAt = await page.evaluate(() =>
    [...document.querySelectorAll(".tab-item")].findIndex((tab) => tab.dataset.tabOpen === "builder"),
  );
  await page.keyboard.press(`${primary}+${builderAt + 1}`);
  await page.locator('.tab-item--selected[data-tab-open="builder"]').waitFor();
  // Leaving the project's page for a session, its tab brings the page back.
  await page.locator('.tab-item--project[data-tab-open^="project:"]').first().click();
  await page.locator('.tab-item--project.tab-item--selected').waitFor();
  await page.locator(".header").waitFor();
  await page.locator('.tab-item[data-tab-open="builder"]').click();
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
  // A tab strip scrolled sideways stays put when its last tab is clicked.
  // Narrower than the tabs, wider than the last one, whatever the fonts.
  const narrow = await page.evaluate(() => [...document.querySelectorAll(".tab-item")].at(-1).offsetWidth + 40);
  await page.addStyleTag({ content: `.tabbar__tabs { max-width: ${narrow}px !important; flex: none !important; }` });
  const strip = page.locator(".tabbar__tabs");
  const shownTab = await page.locator(".tab-item--selected").first().getAttribute("data-tab-open");
  const end = await strip.evaluate((node) => (node.scrollLeft = node.scrollWidth));
  const scrolledTo = await strip.evaluate((node) => node.scrollLeft);
  assert(scrolledTo > 0, `the strip did not scroll (${end})`);
  await page.locator(".tab-item").last().click({ position: { x: 20, y: 10 } });
  await page.locator(".tab-item--selected").last().waitFor();
  // It does not jump back to the start, and the tab stays in view. (A
  // selected tab sheds its ⌘-number, so a few pixels may shift.)
  const [kept, inView] = await strip.evaluate((node) => {
    const tab = node.querySelector(".tab-item--selected").getBoundingClientRect();
    const box = node.getBoundingClientRect();
    return [node.scrollLeft, tab.left >= box.left - 1 && tab.right <= box.right + 1];
  });
  assert(kept > scrolledTo / 2 && inView, `the strip moved from ${scrolledTo} to ${kept}, tab in view: ${inView}`);
  await page.evaluate(() => document.querySelectorAll("style").forEach((tag) => tag.textContent.includes(".tabbar__tabs { max-width") && tag.remove()));
  await page.locator(`.tab-item[data-tab-open="${shownTab}"]`).click({ position: { x: 20, y: 10 } });
  await page.locator(`.tab-item--selected[data-tab-open="${shownTab}"]`).waitFor();
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
  await page.waitForFunction(
    (at) => [...document.querySelectorAll(".tab-item")].findIndex((tab) => tab.dataset.tabOpen === "builder") !== at,
    before.indexOf("builder"),
  );
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
  // Opening another project's session gives the session a tab, not the
  // project's page it passes through.
  await page.locator(".board__column", { hasText: "Idle" }).locator('[data-open="notes"]').click();
  await page.locator('.tab-item--selected[data-tab-open="notes"]').waitFor();
  assert.equal(await page.locator('.tab-item[data-tab-open="project:other"]').count(), 0, "no tab for a page not gone to");
  await page.locator('[data-tab-close="notes"]').click();
  await page.locator('.tab-item[data-tab-open="notes"]').waitFor({ state: "detached" });
  await page.locator('.project-row [data-project="project"]').click();
  await page.locator(".header").waitFor();

  await page.locator('[data-tab="tasks"]').click();
  await page.locator('[data-action="import"]').click();
  await page.locator("#imp-manual").fill("ENG-7 Test imported task");
  await page.locator("#imp-model").fill("sonnet");
  await page.locator('[data-action="import-run"]:not([disabled])').click();
  // The board is the default view; the task's own form carries its model.
  await page.locator('[data-task-view="board"][aria-pressed="true"]').waitFor();
  await page.locator('[data-task-column="queued"] [data-task-card="task"]').click();
  // The form keeps its work to Save and Run now: no Accept or Back to queue.
  assert.equal(await page.locator(".modal [data-status]").count(), 0);
  // Model and account sit under Advanced; the models are the agent's own.
  await page.locator('.modal [data-toggle="advanced"]').click();
  assert.equal(await page.locator("#task-model").inputValue(), "sonnet");
  assert.deepEqual(await page.locator("#task-model option").allTextContents(), ["Default", "Opus · most capable", "Sonnet · balanced"]);
  await page.locator("#task-model").selectOption("opus");
  await page.locator('[data-action="save-task"]').click();
  await page.locator('[data-task-card="task"]').waitFor();
  // A card dragged to another column takes that status, and the drop does
  // not open the task.
  await page.locator('[data-task-card="task"]').dragTo(page.locator('[data-task-column="review"]'));
  await page.locator('[data-task-column="review"] [data-task-card="task"]').waitFor();
  assert.equal(await page.locator(".modal").count(), 0);
  // Its menu sets any status, and deletes after asking.
  await page.locator('[data-task-card="task"]').click({ button: "right" });
  await page.locator('[data-menu-act="task-status:changes"]').click();
  await page.locator('[data-task-column="changes"] [data-task-card="task"]').waitFor();
  // Every project's tasks, each marked with its project.
  await page.locator('[data-task-scope="all"]').click();
  await page.locator('[data-task-card="task"] .card__project', { hasText: "Example" }).waitFor();
  await page.locator('[data-task-scope="project"]').click();
  // The list groups by status under a bar per group, one line per task with
  // its issue key; it is remembered, and rows drag onto another group.
  await page.locator('[data-task-view="list"]').click();
  await page.locator('.task-list__group[data-task-column="changes"] .task-group', { hasText: "Needs changes" }).waitFor();
  await page.locator('.task-row[data-task="task"] .task-row__key', { hasText: "ENG-7" }).waitFor();
  assert.equal(await page.evaluate(() => localStorage.getItem("convoy.taskView")), "list");
  await page.locator('[data-task-view="board"]').click();
  await page.locator('[data-task-card="task"]').dragTo(page.locator('[data-task-column="queued"]'));
  await page.locator('[data-task-view="list"]').click();
  await page.locator('.task-list__group[data-task-column="queued"] .task-row[data-task="task"]').waitFor();
  if (process.env.CONVOY_TEST_SHOTS) await page.screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/tasks-list.png` });
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
  // The search narrows the list and keeps focus while typing. Checked rows
  // it hides are named, since they import too. Deselect all clears every
  // row, hidden ones included, so searching, deselecting and ticking one
  // imports that one; Select all picks only what the search shows.
  await page.locator("#sweep-filter").pressSequentially("release");
  assert.deepEqual(await page.locator(".sweep__row .import__title > div:first-child").allTextContents(), ["Tidy release notes"]);
  assert.equal(await page.evaluate(() => document.activeElement?.id), "sweep-filter");
  await page.locator(".sweep__hidden", { hasText: "2 selected outside this search" }).waitFor();
  await page.locator('[data-sweep-all="none"]').click();
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 0" }).waitFor();
  await page.locator('[data-sweep-check="0199aaaa-0000-4000-8000-000000000012"]').check();
  await page.locator('[data-action="history-sweep-run"]', { hasText: "Import 1" }).waitFor();
  assert.equal(await page.locator(".sweep__hidden").count(), 0);
  await page.locator("#sweep-filter").fill("nothing like this");
  await page.locator(".sweep__empty").waitFor();
  await page.locator("#sweep-filter").fill("");
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
  // Remove from Convoy takes only sessions and leaves their conversations.
  await page.locator('[data-action="pick-start"]').click();
  assert.equal(await page.locator('[data-action="pick-remove"]').isDisabled(), true);
  await page.locator('[data-pick-row="s:review"] .row__title').click();
  await page.locator('[data-action="pick-remove"]').click();
  await page.locator(".modal", { hasText: "Only Convoy forgets them" }).waitFor();
  await page.locator('.modal [data-action="confirm-generic"]').click();
  await page.locator(".toast", { hasText: "Removed 1 session from Convoy" }).waitFor();
  assert.deepEqual(await page.evaluate(() => window.checkCalls.findLast((c) => c.cmd === "sessions_remove").args.ids), ["review"]);
  await page.locator('[data-action="pick-done"]').click();
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
  // Text fields read in either theme: the commit message is written in the
  // page's text colour, not the system's dark field text.
  for (const theme of ["dark", "light"]) {
    await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
    const [field, text] = await page.evaluate(() => [
      getComputedStyle(document.querySelector("#commit-message")).color,
      getComputedStyle(document.body).color,
    ]);
    assert.equal(field, text, `commit message text in ${theme}`);
    if (process.env.CONVOY_TEST_SHOTS) await page.locator(".commit-bar").screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/commit-${theme}.png` });
  }
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
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
  // A task with a session opens that session on a click; its form is in its
  // menu.
  await page.locator('[data-tab="tasks"]').click();
  await page.locator(".task-row", { hasText: "Checkout retries" }).click();
  await page.locator('.tab-item--selected[data-tab-open="builder"]').waitFor();
  assert.equal(await page.locator(".modal").count(), 0);
  // Escape is always the page's, so the macOS window never takes it as
  // "leave full screen".
  assert.equal(
    await page.evaluate(() => !document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }))),
    true,
  );
  await page.locator('.project-row [data-project="project"]').click();
  await page.locator('[data-tab="tasks"]').click();
  // "Blocked by": the task waits for another, and says so. One that has
  // never run opens its form.
  await page.locator('.task-row[data-task="task"]').click();
  await page.locator('.modal [data-depends-on]').first().check();
  // Related tasks: picked from a list, shown as chips, linked on save.
  await page.locator("#task-title").fill("ENG-7: Test imported task, edited");
  const relatedId = await page.locator('.modal [data-related-add] option[value]:not([value=""])').first().getAttribute("value");
  await page.locator(".modal [data-related-add]").selectOption(relatedId);
  await page.locator(`.modal .related-chip [data-related-remove="${relatedId}"]`).waitFor();
  assert.equal(await page.locator("#task-title").inputValue(), "ENG-7: Test imported task, edited", "picking keeps what was typed");
  if (process.env.CONVOY_TEST_SHOTS) await page.locator(".modal").screenshot({ path: `${process.env.CONVOY_TEST_SHOTS}/related-form.png` });
  await page.locator('[data-action="save-task"]').click();
  await page.waitForFunction(
    (other) => window.checkCalls.some((c) => c.cmd === "task_related" && c.args.id === "task" && c.args.related.includes(other)),
    relatedId,
  );
  await page.locator('.task-row[data-task="task"] .task-blocked[title="Waits for Checkout retries"]').waitFor();
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
  await page.locator('.task-row[data-task="task"] [data-task-more="task"]').click();
  await page.locator('[data-menu-act="task-delete"]').click();
  // A destructive button keeps its colour under the pointer.
  await page.locator('.modal [data-action="confirm-generic"]').hover();
  assert.equal(
    await page.locator('.modal [data-action="confirm-generic"]').evaluate((node) => getComputedStyle(node).backgroundColor),
    "rgb(239, 68, 68)",
  );
  await page.locator('.modal [data-action="confirm-generic"]').click();
  await page.locator('.task-row[data-task="task"]').waitFor({ state: "detached" });

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
  // Rename: from the menu, the name selected, Enter saves; a double-click on
  // a name opens the same sheet, and Escape leaves it unchanged.
  await page.locator('.side-session[data-menu-session="builder"]').click({ button: "right" });
  await page.locator('[data-menu-act="rename"]').click();
  assert.equal(await page.evaluate(() => { const box = document.querySelector("#rename-title"); return document.activeElement === box && box.selectionEnd - box.selectionStart === box.value.length; }), true, "the name is selected");
  await page.keyboard.type("Payments builder");
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_edit" && c.args.id === "builder" && c.args.input.title === "Payments builder"));
  await page.locator("#rename-title").waitFor({ state: "detached" });
  await page.locator('.side-session[data-menu-session="builder"] .side-session__title').dblclick();
  await page.locator("#rename-title").waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#rename-title").waitFor({ state: "detached" });
  await page.locator(".crumb--session").click();
  await page.locator('.menu--session [data-action="rename-current"]').click();
  await page.locator("#rename-title").waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#rename-title").waitFor({ state: "detached" });

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
  // Earlier conversations as background: a session of this project and a
  // saved one, picked from one menu; a chip takes one back out.
  await page.locator('#draft-knowledge option[value="c:claude:0199aaaa-0000-4000-8000-000000000001"]').waitFor({ state: "attached" });
  await page.locator("#draft-knowledge").selectOption("s:builder");
  await page.locator('[data-knowledge-remove="s:builder"]').waitFor();
  await page.locator("#draft-knowledge").selectOption("c:claude:0199aaaa-0000-4000-8000-000000000001");
  await page.locator(".knowledge__chip", { hasText: "Fix flaky login" }).waitFor();
  assert.equal(await page.locator('#draft-knowledge option[value="s:builder"]').count(), 0, "a picked one leaves the menu");
  assert.equal(await page.locator("#draft-title").inputValue(), "Fix flaky checkout", "picking keeps what was typed");
  if (shots) {
    await page.locator("#draft-knowledge").scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${shots}/new-session.png` });
  }
  await page.locator('[data-knowledge-remove="s:builder"]').click();
  await page.locator('#draft-knowledge option[value="s:builder"]').waitFor({ state: "attached" });
  await page.locator('[data-action="create-session"]').click();
  await page.locator('.tab-item--selected[data-tab-open="fresh"]').waitFor();
  const made = await page.evaluate(() => window.checkCalls.filter((c) => ["session_create", "worktree_create"].includes(c.cmd) || (c.cmd === "session_start" && c.args.id === "fresh")).map((c) => ({ cmd: c.cmd, ...c.args })));
  assert.deepEqual(made.map((c) => c.cmd), ["session_create", "worktree_create", "session_start"]);
  assert.equal(made[0].profileId, "work");
  assert.equal(made[0].agent, "codex");
  assert.equal(made[0].view, "terminal", "the sheet starts from Settings' default view");
  assert.equal(made[0].effort, "ultra", "the effort chosen in the sheet");
  assert.deepEqual(made[0].knowledge, [{ agent: "claude", provider_id: "0199aaaa-0000-4000-8000-000000000001", title: "Fix flaky login" }]);
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
  // Each project imports from its own workspace and project: the pickers
  // fill from the connection, each one narrowing the next.
  await page.locator("#link-connection").selectOption("peshbar");
  await page.locator('#link-workspace option[value="4"]').waitFor({ state: "attached" });
  assert.deepEqual(await page.locator(".pref-row__title").filter({ hasText: /^(Workspace|Project)$/ }).allTextContents(), ["Workspace"]);
  await page.locator("#link-workspace").selectOption("4");
  await page.locator('#link-project option[value="3"]').waitFor({ state: "attached" });
  await page.locator("#link-project").selectOption("3");
  await page.waitForFunction(() => window.links?.project?.project?.name === "Amir cargo");
  assert.deepEqual(await page.evaluate(() => window.links.project), {
    connection_id: "peshbar",
    workspace: { id: "4", name: "Peshbar" },
    project: { id: "3", name: "Amir cargo", key: "AMR" },
  });
  // Status sync: switched on, the mapping starts from the suggestion and
  // every change is saved with the link.
  await page.locator('[data-sync-toggle="enabled"]').click();
  await page.locator('[data-sync-status="testing"]').waitFor();
  await page.waitForFunction(() => window.links?.project?.sync?.map?.testing?.name === "QA");
  assert.equal(await page.locator('[data-sync-status="building"]').inputValue(), "In progress");
  assert.equal(await page.locator('[data-sync-status="failed"]').inputValue(), "");
  await page.locator('[data-sync-status="failed"]').selectOption("To do");
  await page.waitForFunction(() => window.links.project.sync.map.failed?.name === "To do");
  await page.locator('[data-sync-toggle="link_pr"]').click();
  await page.waitForFunction(() => window.links.project.sync.link_pr === false);
  await page.getByText("Statuses are matched by name").waitFor();
  if (shots) {
    await page.locator(".sync-map").scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${shots}/project-link.png` });
  }
  await page.locator('[data-action="close-project-settings"]').click();
  await page.locator(".header").waitFor();
  // Import tasks opens on the linked connection and searches only there.
  await page.locator('[data-tab="tasks"]').click();
  await page.locator('[data-action="import"]').click();
  await page.locator(".field__note", { hasText: "Searching in Peshbar › Amir cargo." }).waitFor();
  assert.equal(await page.locator("#imp-connection").inputValue(), "peshbar");
  await page.locator(".import__row", { hasText: "Weigh parcels at intake" }).waitFor();
  if (shots) await page.screenshot({ path: `${shots}/import-linked.png` });
  assert.equal(await page.evaluate(() => window.checkCalls.findLast((c) => c.cmd === "issues_search").args.projectId), "project");
  // Notion searches page titles or a pasted link; it has no "assigned to me".
  await page.locator("#imp-connection").selectOption("notion");
  await page.locator('#imp-query[placeholder="Page title, or a page link"]').waitFor();
  assert.equal(await page.locator("#imp-mine").count(), 0);
  await page.locator(".field__note", { hasText: "Searching all of Notion." }).waitFor();
  await page.keyboard.press("Escape");
  // Integrations from the Tasks tab: the same catalog, and only Done below it.
  await page.locator('.section [data-action="integrations"]').click();
  await page.locator(".modal--catalog .catalog__service").nth(3).waitFor();
  assert.equal(await page.locator(".modal--catalog .modal__foot .button").count(), 1);
  if (shots) {
    await page.waitForTimeout(300); // past the opening animation
    await page.screenshot({ path: `${shots}/integrations.png` });
  }
  await page.keyboard.press("Escape");
  await page.locator(".modal--catalog").waitFor({ state: "detached" });
  // Unlinked again, the import is back to the whole connection.
  await page.evaluate(() => { window.links = {}; });
  // A task whose push failed says so on its card, and Retry sends it again;
  // once it goes through, the chip turns to a tick.
  await page.evaluate(() => {
    window.mockTasks.push({
      id: "synced", project_id: "project", title: "ENG-8: Sync me", details: "", findings: "", agent: "claude", status: "pr", mode: "pr",
      auto_review: false, depends_on: [], blocked_by: [], source: { tracker: "linear", key: "ENG-8" },
      sync: { failed: { status: "pr" }, last: { ok: false, message: "Linear rejected the credentials (HTTP 401).", at: new Date().toISOString() } },
    });
  });
  await page.locator('[data-tab="tasks"]').click();
  const syncChip = page.locator('[data-task="synced"] .sync-chip--failed');
  await syncChip.waitFor();
  assert.match(await syncChip.getAttribute("aria-label"), /Linear ENG-8 · not updated: Linear rejected the credentials/);
  await page.locator('[data-task="synced"]').first().click({ button: "right" });
  await page.locator('[data-menu-act="task-sync-retry"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "task_sync_retry" && c.args.id === "synced"));
  await page.locator('[data-task="synced"] .sync-chip--pending').waitFor();
  await page.evaluate(() => {
    window.mockTasks.find((item) => item.id === "synced").sync = { last: { ok: true, message: "Set to In review", at: new Date().toISOString(), status: "In review" } };
    window.emitTauri("task:sync", "synced");
  });
  await page.locator('[data-task="synced"] .sync-chip--ok').waitFor();

  // ------------------------------------------------------------------ QA --
  // The project's QA: commands found in its files, an agent, automatic runs.
  // Reached from the Settings button in the project's header.
  await page.locator('.header__actions [data-action="project-settings"]').click();
  await page.locator('[data-action="qa-detect"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.test_commands === "npm test\nnpm run lint"));
  await page.locator('[data-proj-select="qa_agent"]').selectOption("codex");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.qa_agent === "codex"));
  await page.locator('[data-proj-select="qa_timeout"]').selectOption("30");
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.qa_timeout === 30));
  await page.locator('[data-proj-toggle="qa_auto"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "project_edit" && c.args.input.qa_auto === true));
  if (shots) {
    await page.locator('[data-action="qa-detect"]').scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${shots}/qa-settings.png` });
  }
  await page.locator('[data-action="close-project-settings"]').click();
  await page.locator('[data-tab="tasks"]').click();

  // A failing check sends the task back, and the findings go straight to
  // a builder that is running.
  await page.evaluate(() => {
    const task = window.mockTasks.find((item) => item.id === "synced");
    task.session_id = "builder";
    window.qaStart = (id) => {
      Object.assign(window.mockTasks.find((item) => item.id === id), {
        status: "changes",
        findings: "The check `npm run lint` failed with exit code 1.",
        qa: { id: "r1", status: "failed", started: new Date().toISOString(), findings: "The check `npm run lint` failed with exit code 1.",
              checks: [
                { command: "npm test", ok: true, code: 0, seconds: 41, output: "42 passed" },
                { command: "npm run lint", ok: false, code: 1, seconds: 3, output: "src/login.ts: unused variable" },
              ] },
      });
      return [
        { kind: "notice", task: id, event: "fixing", detail: "The checks" },
        { kind: "send", task: id, session: "builder", text: "QA found problems with this task." },
      ];
    };
  });
  await page.locator('[data-task="synced"]').first().click({ button: "right" });
  await page.locator('[data-menu-act="task-qa"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "terminal_paste" && c.args.id === "builder" && c.args.text === "QA found problems with this task." && c.args.submit === true));
  await page.locator(".toast", { hasText: "The checks found problems with" }).waitFor();
  await page.locator('[data-task="synced"] .qa-badge--failed', { hasText: "QA failed" }).waitFor();

  // The task form shows each check, the failed one's output open.
  await page.locator('[data-task="synced"] [data-task-more="synced"]').click();
  await page.locator('[data-menu-act="task-edit"]').click();
  await page.locator(".qa-panel", { hasText: "QA failed" }).waitFor();
  assert.deepEqual(await page.locator(".qa-check__command").allTextContents(), ["npm test", "npm run lint"]);
  await page.locator(".qa-check--failed details[open] .qa-check__output", { hasText: "unused variable" }).waitFor();
  if (shots) {
    await page.waitForTimeout(300); // past the opening animation
    await page.screenshot({ path: `${shots}/qa-failed.png` });
  }

  // Run again, it passes: the task waits in testing to be marked done.
  await page.evaluate(() => {
    window.qaStart = (id) => {
      Object.assign(window.mockTasks.find((item) => item.id === id), {
        status: "testing",
        qa: { id: "r2", status: "passed", started: new Date().toISOString(), checks: [
          { command: "npm test", ok: true, code: 0, seconds: 40, output: "" },
          { command: "npm run lint", ok: true, code: 0, seconds: 3, output: "" },
        ] },
      });
      return [{ kind: "notice", task: id, event: "ready", detail: "" }];
    };
  });
  await page.locator('.modal [data-action="task-qa-run"]').click();
  await page.locator(".qa-panel", { hasText: "QA passed" }).waitFor();
  await page.locator('.modal [data-action="task-mark-done"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "task_status" && c.args.id === "synced" && c.args.status === "done"));
  await page.locator(".modal").waitFor({ state: "detached" });

  // A QA agent's verdict, when its session ends: a failure with no running
  // builder waits for the builder's next resume.
  await page.evaluate(() => {
    window.mockSessions.push({ id: "qa1", project_id: "project", title: "QA: Sync me", agent: "codex", provider_id: "pq", started: true, running: false, pinned: false, review_of: null, qa_of: "synced" });
    window.pipelineExit = (id) => (id === "qa1" ? [{ kind: "notice", task: "synced", event: "fixing", detail: "QA" }] : []);
  });
  await page.locator('[data-action="home"]').first().click();
  await page.evaluate(() => window.emitTauri("terminal:exit", { id: "qa1", cause: "exited", code: 0 }));
  await page.locator(".toast", { hasText: "QA found problems with" }).waitFor();
  await page.locator('.project-row [data-project="project"]').click();
  await page.locator(".header").waitFor();
  if (shots) await page.screenshot({ path: `${shots}/sync-chips.png` });

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

  // Plan first: the plan waits in its own column; a comment goes back to
  // the planner, and approving it builds the task.
  await page.evaluate(() => {
    window.mockTasks.push({
      id: "planned", project_id: "project", title: "Refresh tokens", details: "", findings: "", agent: "claude", status: "plan_review",
      mode: "none", auto_review: true, plan_first: true, round: 0, max_rounds: 2, depends_on: [], related: [], skills: [], docs: [],
      plan: { id: "p1", status: "waiting", text: "# Plan\n1. Read the token.", comments: [], session_id: "builder" },
    });
  });
  await page.locator('[data-tab="tasks"]').click();
  await page.locator('[data-task-column="plan_review"] [data-task="planned"]').first().waitFor();
  await page.locator('[data-task="planned"]').first().click({ button: "right" });
  await page.locator('[data-menu-act="task-plan"]').click();
  await page.locator(".plan-text h1", { hasText: "Plan" }).waitFor();
  if (shots) await page.waitForTimeout(300), await page.screenshot({ path: `${shots}/plan-review.png` });
  await page.locator("#plan-comment").fill("Cover logout too.");
  await page.locator('.modal [data-action="plan-changes"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "plan_request_changes" && c.args.comment === "Cover logout too."));
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "terminal_paste" && c.args.text === "Revise the plan: Cover logout too."));
  await page.evaluate(() => Object.assign(window.mockTasks.find((item) => item.id === "planned"), { status: "plan_review", plan: { id: "p2", status: "waiting", text: "1. Read the token.\n2. Cover logout.", comments: [{ at: "", text: "Cover logout too." }], session_id: "builder" } }));
  await page.keyboard.press("Escape");
  await page.locator('[data-task="planned"]').first().click({ button: "right" });
  await page.locator('[data-menu-act="task-plan"]').click();
  await page.locator('.modal [data-action="plan-approve"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "plan_approve" && c.args.taskId === "planned"));
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "task_prepare" && c.args.id === "planned"));

  // The queue starts what the core picks: here two tasks side by side.
  await page.evaluate(() => {
    for (const id of ["side-a", "side-b"]) {
      window.mockTasks.push({ id, project_id: "project", title: `Side ${id}`, details: "", findings: "", agent: "claude", status: "queued", mode: "none", auto_review: false, round: 0, max_rounds: 2, depends_on: [], related: [], skills: [], docs: [] });
    }
    let asked = 0;
    window.queueNext = () => (asked++ === 0 ? ["side-a", "side-b"] : []);
    window.prepared = (id) => {
      window.mockSessions.push({ id: `q-${id}`, project_id: "project", title: `Side ${id}`, agent: "claude", provider_id: "", started: false, running: false, pinned: false, review_of: null, task_id: id });
      return `q-${id}`;
    };
  });
  await page.keyboard.press("Escape");
  await page.locator('[data-action="queue-start"]').click();
  await page.locator('.modal [data-action="confirm-queue"]').click();
  await page.waitForFunction(() => ["side-a", "side-b"].every((id) => window.checkCalls.some((c) => c.cmd === "task_prepare" && c.args.id === id)));

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
  // Each role's instructions: the reviewer's replaces the older template.
  await page.locator('[data-settings-section="Agents"]').click();
  await page.locator('[data-pref-role="review"]').fill("Check the migrations too.");
  await page.locator('[data-pref-role="review"]').blur();
  await page.waitForFunction(() => window.savedSettings.role_instructions?.review === "Check the migrations too." && window.savedSettings.review_template === "");
  await page.locator('[data-settings-section="Notifications"]').click();
  await page.locator('[data-pref-toggle="notifications"]').click();
  await page.waitForFunction(() => window.savedSettings.notifications === true);
  await page.locator("#settings-search").fill("jira");
  assert.equal(await page.locator(".prefs__item").count(), 1, "search narrows the sections");
  await page.locator('[data-settings-section="Integrations"]').click();
  // One card per service, its connections inside: connected ones first,
  // the rest offering Connect.
  await page.locator(".catalog__connection", { hasText: "Linear MCP" }).waitFor();
  assert.deepEqual(await page.locator(".catalog__name").allTextContents(), ["Linear", "Notion", "Peshbar", "Jira"]);
  await page.locator('.catalog__service', { hasText: "Jira" }).getByRole("button", { name: "Connect" }).waitFor();
  await page.locator('.catalog__service', { hasText: "Peshbar" }).getByRole("button", { name: "Add another" }).waitFor();
  await page.locator(".catalog__connection", { hasText: "Peshbar" }).getByText("Token").waitFor();
  if (shots) await page.screenshot({ path: `${shots}/settings-integrations.png` });
  // Peshbar: a token connection, added with its API URL.
  await page.locator('[data-conn-add="peshbar"]').click();
  await page.locator('#conn-site[placeholder="https://management-api.peshbar.com"]').waitFor();
  assert.deepEqual(
    await page.locator("[data-conn-auth]").evaluateAll((items) => items.map((item) => item.dataset.connAuth)),
    ["apiKey", "mcp"],
    "Peshbar connects with a token or the agent's MCP server",
  );
  await page.getByText("Convoy also connects the peshbar MCP server").waitFor();
  await page.keyboard.press("Escape");
  await page.locator("#conn-site").waitFor({ state: "detached" });
  // Notion: an internal integration's secret, or the agent's own MCP server.
  await page.locator('[data-conn-add="notion"]').click();
  await page.locator(".field__label", { hasText: "Internal integration secret" }).waitFor();
  assert.deepEqual(
    await page.locator("[data-conn-auth]").evaluateAll((items) => items.map((item) => item.dataset.connAuth)),
    ["apiKey", "mcp"],
  );
  await page.getByText("share each database with it").waitFor();
  assert.equal(await page.locator("#conn-site").count(), 0, "Notion has no site to enter");
  await page.locator('[data-conn-auth="mcp"]').click();
  await page.getByText("https://mcp.notion.com/mcp").waitFor();
  await page.keyboard.press("Escape");
  await page.locator('[data-conn-auth]').first().waitFor({ state: "detached" });
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
  // A project found by the search opens on a click, as it does unfiltered.
  await page.locator("#project-search").fill("alp");
  await page.locator('[data-project="other"]').waitFor({ state: "detached" });
  await page.locator('[data-project="alpha"]').click();
  await page.locator(".header__name", { hasText: "Alpha" }).waitFor({ timeout: 5000 });
  await page.locator('.project-row--current [data-project="alpha"]').waitFor();
  await page.locator("#project-search").fill("");
  // So does one picked in ⌘K: from Home it leaves Home, and from inside one
  // of its sessions it shows the project rather than staying put.
  const pickProject = async (path) => {
    await page.keyboard.press(`${primary}+k`);
    await page.locator("#palette-input").fill(path);
    await page.locator(".palette__row", { hasText: path }).click();
  };
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("Home");
  await page.locator(".palette__row", { hasText: "Every project at once" }).click();
  await page.locator(".home__greeting").waitFor();
  await pickProject("/fixture/Other");
  await page.locator(".header__name", { hasText: "Other" }).waitFor({ timeout: 5000 });
  await page.locator('.project-row__sessions [data-open="notes"]').first().click();
  await page.locator('.tab-item--selected[data-tab-open="notes"]').waitFor();
  await pickProject("/fixture/Other");
  await page.locator(".header__name", { hasText: "Other" }).waitFor({ timeout: 5000 });
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
  assert.deepEqual(calls.filter((c) => c.cmd === "task_status").map((c) => c.args.status), ["review", "changes", "queued", "done"]);
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
  assert.equal(await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "chat_history" && c.args.id === "chatty").length), 1, "history is read once");
  // Stopped in the terminal, it stays a terminal: what it printed is still
  // there, under a line saying it stopped.
  await page.evaluate(() => window.emitTauri("terminal:data", { id: "chatty", data: "working…" }));
  await page.evaluate(() => {
    window.mockSessions.find((item) => item.id === "chatty").running = false;
    window.emitTauri("terminal:exit", { id: "chatty", cause: "stopped", code: 0 });
  });
  await page.waitForFunction(() => document.querySelector("#terminal-host .xterm-rows")?.textContent.includes("session stopped"));
  assert.equal(await page.locator("#terminal-host").count(), 1);
  assert.equal(await page.locator(".chat-slot--readonly").count(), 0);
  // Opened after a restart, a stopped session shows its saved output as a
  // terminal; only one with next to nothing saved shows its conversation.
  await page.evaluate(() => {
    for (const [id, title] of [["busy", "Email integration"], ["quiet", "Spike"]]) {
      window.mockSessions.push({ id, project_id: "project", title, agent: "claude", provider_id: `p-${id}`, started: true, running: false, pinned: false, review_of: null });
    }
  });
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-open="busy"]').first().click();
  await page.waitForFunction(() => document.querySelector("#terminal-host .xterm-rows")?.textContent.includes("Pushed to main as 0fc2782"));
  assert.equal(await page.locator(".chat-slot--readonly").count(), 0);
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-open="quiet"]').first().click();
  await page.locator(".session-banner", { hasText: "Saved conversation · read-only" }).waitFor();
  await page.locator(".chat-slot--readonly .chat").waitFor();
  assert.equal(await page.locator(".chat-slot--readonly .chat__composer").isVisible(), false);

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

  // ------------------------------------------------------------ helpers --
  // A manager is chosen in the New session sheet; the helpers it starts from
  // its shell nest under it in the sidebar, show as live cards in its chat,
  // and as a menu in its terminal's header.
  await page.locator('[data-project="project"]').first().click();
  await page.locator('.tabbar [data-action="new-session"]').click();
  const managerToggle = page.locator('.modal [data-toggle="manager"]');
  await managerToggle.waitFor();
  assert.equal(await managerToggle.getAttribute("aria-pressed"), "false", "not a manager unless asked");
  await page.locator('.modal .setting', { hasText: "Manager — can start helper sessions" }).waitFor();
  await managerToggle.click();
  assert.equal(await page.locator('.modal [data-toggle="manager"]').getAttribute("aria-pressed"), "true");
  if (shots) await page.screenshot({ path: `${shots}/new-session-manager.png` });
  await page.locator('.choice [data-value="claude"]').click();
  await page.locator('.modal [data-set="view"][data-value="chat"]').click();
  await page.locator("#draft-title").fill("Lead the refactor");
  await page.locator('[data-action="create-session"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_create" && c.args.manager === true && c.args.title === "Lead the refactor"));
  await page.locator('.tab-item--selected[data-tab-open="lead"]').waitFor();
  // Two helpers appear, as the backend says after a spawn.
  await page.evaluate(() => {
    window.mockSessions.push(
      { id: "helper-1", project_id: "project", parent_id: "lead", title: "Write parser tests", agent: "codex", provider_id: "", started: true, running: true, pinned: false, review_of: null, view: "chat", branch: "convoy/parser-tests-1a2b3c" },
      { id: "helper-2", project_id: "project", parent_id: "lead", title: "Update the changelog", agent: "claude", provider_id: "p9", started: true, running: true, pinned: false, review_of: null, view: "chat" },
    );
    window.emitTauri("session:spawned", { id: "helper-2", parent: "lead" });
  });
  const leadRow = page.locator('.project-row__sessions .side-session[data-open="lead"]');
  await page.locator('.side-session--helper[data-open="helper-1"]').waitFor();
  await leadRow.locator(".side-session__helpers", { hasText: "2 helpers · 2 running" }).waitFor();
  assert.deepEqual(
    await page.evaluate(() => {
      const lead = document.querySelector('.project-row__sessions .side-session[data-open="lead"]');
      return [lead.nextElementSibling?.dataset.open, lead.nextElementSibling?.nextElementSibling?.dataset.open];
    }),
    ["helper-1", "helper-2"],
    "helpers sit right under their manager",
  );
  assert.equal(await page.locator('.side-session--helper[data-open="helper-1"] .side-session__branch').textContent(), "↳");
  // The manager's chat: one card per helper, updated by the helpers' events.
  const leadChat = page.locator('.chat[data-chat="lead"]');
  await leadChat.locator(".helper-card").nth(1).waitFor();
  assert.equal(await leadChat.locator(".helper-card").count(), 2);
  const cardOne = leadChat.locator('.helper-card[data-helper="helper-1"]');
  assert.equal(await cardOne.locator(".helper-card__title").textContent(), "Write parser tests");
  await cardOne.locator(".helper-card__status", { hasText: "Working" }).waitFor();
  const emitHelper = (events) => page.evaluate((list) => list.forEach((event) => window.emitTauri("chat:event", { id: "helper-1", event })), events);
  await emitHelper([{ type: "turn_started" }, { type: "approval", request: "h1", kind: "command", title: "Run `cargo test`", detail: "cargo test", always: true }]);
  await leadChat.locator('.helper-card--waiting[data-helper="helper-1"] .helper-card__status', { hasText: "Needs you" }).waitFor();
  await emitHelper([{ type: "approval_resolved", request: "h1" }, { type: "text_done", id: "m1", text: "Added 6 tests; all pass." }, { type: "turn_ended", usage: { input: 1, output: 1, cache_read: 0, cache_write: 0 }, error: null }]);
  await cardOne.locator(".helper-card__status", { hasText: "Finished" }).waitFor();
  await leadRow.locator(".side-session__helpers", { hasText: "2 helpers · 1 running" }).waitFor();
  await leadChat.locator(".helpers__count", { hasText: "2 · 1 running" }).waitFor();
  if (shots) {
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/helpers-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  }
  // Open goes to the helper, whose chat leads back to its manager.
  await cardOne.locator(".helper-card__open").click();
  const helperChat = page.locator('.chat[data-chat="helper-1"]');
  await helperChat.locator(".helpers--of .helpers__parent", { hasText: "Lead the refactor" }).waitFor();
  await helperChat.locator(".helpers__parent").click();
  await page.locator('.tab-item--selected[data-tab-open="lead"]').waitFor();
  // Shown as a terminal, the manager's header has the helpers and a menu.
  await page.evaluate(() => {
    Object.assign(window.mockSessions.find((item) => item.id === "lead"), { view: "terminal", running: false, started: true });
    window.emitTauri("session:spawned", { id: "helper-2", parent: "lead" });
  });
  const helpersButton = page.locator(".workbench__bar .bar-helpers");
  await helpersButton.filter({ hasText: "2 helpers" }).waitFor();
  assert.equal(await helpersButton.getAttribute("title"), "2 helpers · 1 running");
  assert.equal(await helpersButton.locator(".dot--running").count(), 1, "one is still working");
  await helpersButton.click();
  const helpersMenu = page.locator(".helpers-menu");
  await helpersMenu.locator(".helpers-menu__item").nth(1).waitFor();
  assert.equal(await helpersMenu.locator(".helpers-menu__item").count(), 2);
  if (shots) await page.waitForTimeout(200), await page.screenshot({ path: `${shots}/helpers-menu.png` });
  await helpersMenu.locator('.helpers-menu__item[data-open="helper-2"]', { hasText: "Working" }).click();
  await helpersMenu.waitFor({ state: "detached" });
  await page.locator('.tab-item--selected[data-tab-open="helper-2"]').waitFor();
  // The manager's activity names its helpers.
  await page.evaluate(() => window.activity.unshift(
    { id: "h2", at: new Date().toISOString(), kind: "done", session_id: "lead", title: "Lead the refactor", detail: "Helper finished: Write parser tests", project_id: "project", project: "Example" },
    { id: "h1", at: new Date(Date.now() - 1000).toISOString(), kind: "started", session_id: "lead", title: "Lead the refactor", detail: "Started helper: Write parser tests", project_id: "project", project: "Example" },
  ));
  await page.locator('[data-action="activity"]').first().click();
  const feed = page.locator(".activity-feed");
  await feed.locator(".activity__title", { hasText: "helper finished" }).waitFor();
  await feed.locator(".activity__title", { hasText: "started helper" }).waitFor();
  assert.equal(await feed.locator(".activity", { hasText: "started helper" }).locator(".activity__meta").textContent(), "Example · Write parser tests");
  await page.keyboard.press("Escape");
  await feed.waitFor({ state: "detached" });
  // Every string the helpers show has its Russian.
  const helpersSource = await readFile(new URL("../src/views-helpers.js", import.meta.url), "utf8");
  const helperKeys = [...helpersSource.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)].map((match) => match[1]);
  helperKeys.push("Working", "Needs you", "Finished", "Stopped", "Starting", "Helper", "Manager", "Manager — can start helper sessions",
    "The agent can hand work to other agents in this project from its shell, wait for them and read their answers. At most 4 run at once.",
    "started helper", "helper finished", "helper failed", "helper stopped");
  assert.deepEqual(helperKeys.filter((key) => !(key in ru)), [], "untranslated helper strings");
  assert("helper" in (await import("../src/locales/ru.js")).default.plurals, "helper has its Russian plural");

  // ---------------------------------------------------------- automations --
  // A project's Automations tab: empty, then one made in the editor, listed
  // with its schedule, next and last run; switched off and on, run now,
  // edited and deleted.
  await page.locator('[data-project="project"]').first().click();
  await page.locator('[data-tab="automations"]').click();
  const autoEmpty = page.locator(".auto-empty");
  await autoEmpty.getByText("No automations yet").waitFor();
  if (shots) await page.screenshot({ path: `${shots}/automations-empty.png` });
  await autoEmpty.locator('[data-auto="new"]').click();
  const editor = page.locator('.modal[aria-label="New automation"]');
  await editor.waitFor();
  // Weekdays at nine until told otherwise, with the next runs shown.
  assert.equal(await editor.locator('[data-set="auto-kind"][data-value="weekdays"]').getAttribute("aria-pressed"), "true");
  await editor.locator(".auto-preview li").nth(2).waitFor();
  await editor.locator("#auto-title").fill("Morning triage");
  await editor.locator("#auto-prompt").fill("Triage the new issues and summarise them.");
  await editor.locator('[data-auto-day="sat"]').click();
  assert.equal(await editor.locator('[data-auto-day="sat"]').getAttribute("aria-pressed"), "true");
  assert.equal(await editor.locator("#auto-title").inputValue(), "Morning triage", "a click keeps what was typed");
  await editor.locator("#auto-at").fill("07:30");
  // No day at all is refused, live.
  for (const day of ["mon", "tue", "wed", "thu", "fri", "sat"]) await editor.locator(`[data-auto-day="${day}"]`).click();
  await editor.locator(".auto-preview__error", { hasText: "Pick at least one day." }).waitFor();
  for (const day of ["mon", "tue", "wed", "thu", "fri", "sat"]) await editor.locator(`[data-auto-day="${day}"]`).click();
  await editor.locator(".auto-preview li").nth(2).waitFor();
  await editor.locator('[data-toggle="auto-worktree"]').click();
  assert.equal(await editor.locator('[data-toggle="auto-catchup"]').getAttribute("aria-pressed"), "true", "catch-up is on by default");
  if (shots) {
    // Tall enough to show the whole sheet.
    await page.setViewportSize({ width: 1280, height: 1080 });
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/automation-editor-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
    await page.setViewportSize({ width: 1280, height: 840 });
  }
  await editor.locator('[data-action="automation-save"]').click();
  await editor.waitFor({ state: "detached" });
  const created = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "automation_save").at(-1).args.input);
  assert.deepEqual(
    { ...created, model: undefined, effort: undefined },
    {
      id: null, project_id: "project", title: "Morning triage", prompt: "Triage the new issues and summarise them.",
      agent: "claude", model: undefined, effort: undefined, profile_id: null, view: "terminal", worktree: true,
      schedule: { kind: "weekdays", at: "07:30", days: ["mon", "tue", "wed", "thu", "fri", "sat"] },
      enabled: true, catch_up: true, skills: [],
    },
  );
  const autoRow = page.locator('.auto-row[data-automation="auto1"]');
  await autoRow.waitFor();
  assert.equal((await autoRow.locator(".row__title").textContent()).replace(/\s+/g, " ").trim(), "Morning triage worktree");
  const scheduleLine = (await autoRow.locator(".auto-row__schedule").textContent()).replace(/\s+/g, " ").trim();
  assert.match(scheduleLine, /^Mon, Tue, Wed, Thu, Fri, Sat at 07:30.*·next in 1 h$/);
  assert.match(await autoRow.locator(".auto-row__schedule span[title]").getAttribute("title"), /\d/, "the exact time on hover");
  await autoRow.locator(".auto-last", { hasText: "Never run" }).waitFor();
  // A run finished: its result, and a link to its session.
  await page.evaluate(() => {
    window.automations[0].last_run = { at: Math.floor(Date.now() / 1000) - 600, session_id: "builder", status: "done" };
    window.emitTauri("automation:changed", "auto1");
  });
  await autoRow.locator(".auto-last__chip", { hasText: "Done" }).waitFor();
  // Switched off: paused, no next run; and on again.
  await autoRow.locator('[data-toggle="auto-enabled:auto1"]').click();
  await page.locator(".auto-row--off", { hasText: "paused" }).waitFor();
  assert(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "automation_enable" && c.args.id === "auto1" && c.args.enabled === false)));
  await page.locator('[data-toggle="auto-enabled:auto1"]').click();
  await page.locator('.auto-row:not(.auto-row--off)[data-automation="auto1"]').waitFor();
  // Run now: a toast, and the row says it is running until it is not.
  await autoRow.locator('[data-auto="run"]').click();
  await page.locator(".toast", { hasText: "Started Morning triage" }).waitFor();
  await autoRow.locator('[data-auto="run"][disabled]', { hasText: "Running…" }).waitFor();
  await page.evaluate(() => {
    Object.assign(window.automations[0], { running: false, last_run: { ...window.automations[0].last_run, status: "done" } });
    window.emitTauri("automation:finished", { id: "auto1", title: "Morning triage", project_id: "project", session_id: "builder", status: "done", note: "" });
  });
  await page.locator(".toast", { hasText: "Automation finished: Morning triage" }).waitFor();
  await autoRow.locator('[data-auto="run"]:not([disabled])').waitFor();
  // Two more, for the list's look.
  await page.evaluate(() => {
    window.automations.push(
      { id: "auto2", project_id: "project", title: "Dependency check", prompt: "Look for outdated dependencies and open a PR.", agent: "codex", model: "", effort: "", profile_id: null, view: "terminal", worktree: true, schedule: { kind: "every", hours: 6, from: "09:00" }, enabled: true, catch_up: false, history: [], last_run: { at: Math.floor(Date.now() / 1000) - 7200, session_id: "nowhere", status: "failed", note: "Stopped before it finished." }, running: false },
      { id: "auto3", project_id: "project", title: "Weekly release notes", prompt: "Draft release notes from this week's merged PRs.", agent: "claude", model: "", effort: "", profile_id: null, view: "chat", worktree: false, schedule: { kind: "weekdays", at: "17:00", days: ["fri"] }, enabled: false, catch_up: true, history: [], last_run: { at: Math.floor(Date.now() / 1000) - 86400 * 3, session_id: "builder", status: "skipped", note: "Still running" }, running: false },
    );
    window.emitTauri("automation:changed", "");
  });
  await page.locator('.auto-row[data-automation="auto3"]').waitFor();
  assert.match((await page.locator('.auto-row[data-automation="auto2"] .auto-row__schedule').textContent()).replace(/\s+/g, " "), /Every 6 hours from 09:00/);
  assert.match((await page.locator('.auto-row[data-automation="auto3"] .auto-row__schedule').textContent()).replace(/\s+/g, " "), /Fri at 05:00 PM|Fri at 17:00/);
  if (shots) {
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/automations-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  }
  // Edit: the editor opens filled in; a new schedule is sent as one.
  await page.locator('.auto-row[data-automation="auto1"] [data-auto="edit"]').click();
  const editEditor = page.locator('.modal[aria-label="Edit automation"]');
  await editEditor.waitFor();
  assert.equal(await editEditor.locator("#auto-title").inputValue(), "Morning triage");
  assert.equal(await editEditor.locator('[data-toggle="auto-worktree"]').getAttribute("aria-pressed"), "true");
  await editEditor.locator('[data-set="auto-kind"][data-value="every"]').click();
  await editEditor.locator("#auto-hours").selectOption("6");
  await editEditor.locator('[data-set="auto-agent"][data-value="codex"]').click();
  await editEditor.locator('[data-action="automation-save"]').click();
  await editEditor.waitFor({ state: "detached" });
  const edited = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "automation_save").at(-1).args.input);
  assert.deepEqual([edited.id, edited.agent, edited.schedule], ["auto1", "codex", { kind: "every", hours: 6, from: "09:00" }]);
  // The palette has both.
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("New automation");
  await page.locator(".palette__row", { hasText: "New automation…" }).first().waitFor();
  await page.keyboard.press("Enter");
  await page.locator('.modal[aria-label="New automation"]').waitFor();
  await page.keyboard.press("Escape");
  // Home names the next one.
  await page.locator('[data-action="home"]').first().click();
  await page.locator(".home__next", { hasText: "Next automation:" }).waitFor();
  await page.locator(".home__next").click();
  await page.locator('[data-tab="automations"][aria-selected="true"]').waitFor();
  // Delete asks first; the rows go.
  for (const id of ["auto1", "auto2", "auto3"]) {
    await page.locator(`.auto-row[data-automation="${id}"] [data-auto="delete"]`).click();
    await page.locator(".modal", { hasText: "Sessions it already started stay." }).locator('[data-action="confirm-generic"]').click();
    await page.locator(`.auto-row[data-automation="${id}"]`).waitFor({ state: "detached" });
  }
  await autoEmpty.waitFor();
  // Every string the automations show has its Russian.
  const autoSources = await Promise.all(["../src/views-automations.js", "../src/automations.js"].map((path) => readFile(new URL(path, import.meta.url), "utf8")));
  const autoKeys = autoSources.flatMap((source) => [...source.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)].map((match) => match[1]));
  autoKeys.push("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun", "Running", "Done", "Failed", "Skipped", "Automations",
    "New automation…", "Run a prompt on a schedule in the current project", "The current project's scheduled prompts",
    "Automation finished: {title}", "Automation failed: {title}", "Next automation: {title}, {when}",
    "automation started", "automation finished", "automation failed",
    "Still running", "Stopped before it finished.", "Convoy closed while it ran.", "Its last run is still going.",
    "Pick at least one day.", "Pick a time as HH:MM.", "Pick between 1 and 24 hours.", "Give the automation a title.", "Write the prompt the agent runs.");
  assert(autoKeys.length > 60, "the automations' strings were found");
  assert.deepEqual(autoKeys.filter((key) => !(key in ru)), [], "untranslated automation strings");

  // --------------------------------------------------------------- skills --
  // Settings → Skills: empty with three templates, one added with a click,
  // one written in the editor (with a Markdown preview) and set automatic.
  // The New session sheet offers those that suit the agent, the automatic
  // one already picked, and sends what was picked; a chat's chip switches
  // a session's skills; deleting one asks first.
  const bothThemes = async (name) => {
    if (!shots) return;
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  };
  await page.locator('[data-action="settings"]').first().click();
  // The search may still hold what an earlier step typed.
  await page.locator("#settings-search").fill("skill");
  await page.locator('[data-settings-section="Skills"]').click();
  await page.locator("#settings-search").fill("");
  await page.locator(".skill-empty", { hasText: "No skills yet" }).waitFor();
  assert.equal(await page.locator(".skill-template").count(), 4);
  await bothThemes("skills-empty");
  await page.locator('.skill-template[data-skill-template="tests"]').click();
  await page.locator(".skill-row", { hasText: "Write tests first" }).waitFor();
  const fromTemplate = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "skill_save").at(-1).args.input);
  assert.equal(fromTemplate.name, "Write tests first");
  assert.match(fromTemplate.body, /a test that fails/);
  assert.equal(await page.locator(".skill-template").count(), 3, "an added template is no longer offered");

  await page.locator('[data-skill="new"]').first().click();
  const skillEditor = page.locator('.modal[aria-label="New skill"]');
  await skillEditor.waitFor();
  await skillEditor.locator("#skill-name").fill("Pineapple sign-off");
  await skillEditor.locator("#skill-description").fill("A visible check that skills reach the agent");
  await skillEditor.locator("#skill-body").fill("Always end your reply with the word **PINEAPPLE**.");
  await skillEditor.locator('[data-toggle="skill-auto"]').click();
  await skillEditor.locator('[data-skill="preview"]').click();
  await skillEditor.locator(".skill-preview strong", { hasText: "PINEAPPLE" }).waitFor();
  await skillEditor.locator('[data-skill="write"]').click();
  assert.equal(await skillEditor.locator("#skill-body").inputValue(), "Always end your reply with the word **PINEAPPLE**.", "the preview keeps what was written");
  await bothThemes("skill-editor");
  await skillEditor.locator('[data-skill="save"]').click();
  await skillEditor.waitFor({ state: "detached" });
  const written = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "skill_save").at(-1).args.input);
  assert.deepEqual(written, { id: null, name: "Pineapple sign-off", description: "A visible check that skills reach the agent", body: "Always end your reply with the word **PINEAPPLE**.", project_id: null, agents: "both", auto: true, roles: null, projects: null });
  // A Codex-only one in this project, for the agent filter.
  await page.locator('[data-skill="new"]').first().click();
  await page.locator("#skill-name").fill("Codex review style");
  await page.locator("#skill-body").fill("Review like a senior engineer.");
  await page.locator('[data-set="skill-agents"][data-value="codex"]').click();
  await page.locator("#skill-project").selectOption("project");
  await page.locator('[data-skill="save"]').click();
  await page.locator(".skill-row", { hasText: "Codex review style" }).waitFor();
  const codexOnly = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "skill_save").at(-1).args.input);
  assert.deepEqual([codexOnly.agents, codexOnly.project_id, codexOnly.auto], ["codex", "project", false]);
  await page.locator(".skill-row", { hasText: "Pineapple sign-off" }).locator(".skill-tag--auto").waitFor();
  await page.locator(".skill-row", { hasText: "Codex review style" }).locator(".skill-tag", { hasText: "Example" }).waitFor();
  await bothThemes("skills-settings");

  // The agents' own skills: picked, for chosen projects and roles.
  await page.locator('[data-skill="import"]').first().click();
  await page.locator(".modal", { hasText: "Import skills from your agents" }).waitFor();
  await page.locator('[data-import-pick="/home/me/.claude/skills/eskhata-qa/SKILL.md"]').click();
  await page.locator('[data-import-scope="chosen"]').click();
  await page.locator('[data-import-project="project"]').click();
  await page.locator('[data-import-role="qa"]').click();
  if (shots) await page.waitForTimeout(300), await page.screenshot({ path: `${shots}/skill-import.png` });
  await page.locator('.modal [data-skill="import-run"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "skills_import"
    && c.args.paths.length === 1 && c.args.paths[0].endsWith("eskhata-qa/SKILL.md")
    && c.args.projects.includes("project") && c.args.roles.includes("qa") && c.args.roles.includes("build")));
  await page.locator(".toast", { hasText: "Imported 1 skill" }).waitFor();
  const ids = await page.evaluate(() => Object.fromEntries(window.skills.map((item) => [item.name, item.id])));
  await page.locator('[data-action="close-settings"]').click();

  // New session: the automatic skill is picked already; Codex's own only
  // shows for Codex; what is picked is sent.
  await page.locator('[data-project="project"]').first().click();
  await page.locator('.tabbar [data-action="new-session"]').click();
  const sheet = page.locator('.modal[aria-label="New session"]');
  await sheet.locator(".skill-picker").waitFor();
  await sheet.locator('.choice [data-value="claude"]').click();
  const chip = (name) => sheet.locator(`.skill-chip[data-skill-pick="${ids[name]}"]`);
  assert.equal(await chip("Pineapple sign-off").getAttribute("aria-pressed"), "true", "automatic skills are preselected");
  assert.equal(await chip("Write tests first").getAttribute("aria-pressed"), "false");
  assert.equal(await chip("Codex review style").count(), 0, "a Codex skill is not offered for Claude");
  await chip("Write tests first").click();
  await sheet.locator('.choice [data-value="codex"]').click();
  await chip("Codex review style").waitFor();
  assert.equal(await chip("Write tests first").getAttribute("aria-pressed"), "true", "a pick survives a change of agent when it still applies");
  await sheet.locator('.choice [data-value="claude"]').click();
  await chip("Codex review style").waitFor({ state: "detached" });
  await sheet.locator("#draft-title").fill("Skilled session");
  await bothThemes("new-session-skills");
  await page.locator('[data-action="create-session"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "session_create" && c.args.title === "Skilled session"));
  const createdWith = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "session_create").at(-1).args.skills);
  assert.deepEqual(createdWith, [ids["Pineapple sign-off"], ids["Write tests first"]]);
  // Its terminal's header says how many.
  await page.locator(".workbench__bar .skills-chip", { hasText: "2 skills" }).waitFor();

  // A chat's chip: the session's skills, switched on and off from the menu.
  await page.evaluate(() => {
    window.chatHistory["pairing"] = [{ type: "user", id: "k1", text: "Pair with me on the parser." }, { type: "text_done", id: "k2", text: "Sure — where should we start?" }];
    window.mockSessions.push({ id: "pairing", project_id: "project", title: "Pair on the parser", agent: "claude", provider_id: "p7", started: true, running: false, pinned: false, review_of: null, view: "chat", skills: [] });
  });
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-open="pairing"]').first().click();
  const chatChip = page.locator('.chat[data-chat="pairing"] .skills-chip');
  await chatChip.waitFor();
  assert.equal((await chatChip.textContent()).trim(), "0 skills");
  await chatChip.click();
  const skillsMenu = page.locator(".skills-menu");
  await skillsMenu.waitFor();
  assert.equal(await skillsMenu.locator(".skills-menu__item").count(), 2, "only Claude's and both agents' skills");
  await skillsMenu.locator(".skills-menu__note", { hasText: "next starts" }).waitFor();
  await skillsMenu.locator(`[data-skill-toggle="${ids["Pineapple sign-off"]}"]`).click();
  await page.waitForFunction((id) => window.checkCalls.some((c) => c.cmd === "session_skills" && c.args.id === "pairing" && c.args.skills.join() === id), ids["Pineapple sign-off"]);
  assert.equal(await skillsMenu.locator(`[data-skill-toggle="${ids["Pineapple sign-off"]}"]`).getAttribute("aria-checked"), "true");
  await page.waitForFunction(() => document.querySelector('.chat[data-chat="pairing"] .skills-chip')?.textContent.includes("1 skill"));
  await bothThemes("chat-skills-menu");
  await skillsMenu.locator(`[data-skill-toggle="${ids["Pineapple sign-off"]}"]`).click();
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "session_skills").at(-1).args.skills.length === 0);
  await page.keyboard.press("Escape");
  if (await skillsMenu.count()) await page.mouse.click(5, 5);

  // The palette reaches both; delete asks first and the row goes.
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("New skill");
  await page.locator(".palette__row", { hasText: "New skill…" }).first().waitFor();
  await page.keyboard.press("Enter");
  await page.locator('.modal[aria-label="New skill"]').waitFor();
  await page.keyboard.press("Escape");
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("Skills…");
  await page.locator(".palette__row", { hasText: "Skills…" }).first().click();
  await page.locator(".skill-row", { hasText: "Codex review style" }).waitFor();
  await page.locator(".skill-row", { hasText: "Codex review style" }).locator('[data-skill="delete"]').click();
  await page.locator(".modal", { hasText: "It comes off every session" }).locator('[data-action="confirm-generic"]').click();
  await page.locator(".skill-row", { hasText: "Codex review style" }).waitFor({ state: "detached" });
  assert(await page.evaluate(() => window.checkCalls.some((c) => c.cmd === "skill_delete")));
  await page.locator('[data-action="close-settings"]').click();

  // Every string the skills show has its Russian.
  const skillSources = await Promise.all(["../src/views-skills.js", "../src/skills.js"].map((path) => readFile(new URL(path, import.meta.url), "utf8")));
  const skillKeys = skillSources.flatMap((source) => [...source.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)].map((match) => match[1]));
  // The starter templates' names, descriptions and bodies, as written.
  const templateBlock = skillSources[0].slice(skillSources[0].indexOf("export const TEMPLATES"), skillSources[0].indexOf("];", skillSources[0].indexOf("export const TEMPLATES")));
  const templateTexts = [...templateBlock.matchAll(/(?:name|description|body): "((?:[^"\\]|\\.)*)"/g)].map((match) => JSON.parse(`"${match[1]}"`));
  assert.equal(templateTexts.length, 12, "four templates, each with a name, description and body");
  skillKeys.push("Skills", "Skills…", "New skill…", "Instructions you attach to sessions, tasks and automations", "Write instructions once and attach them to sessions",
    "Both agents", "Give the skill a name.", "Write what the skill tells the agent.", "A skill with this name already exists.", "Pick Claude, Codex or both.");
  skillKeys.push(...templateTexts);
  assert(skillKeys.length > 50, "the skills' strings were found");
  assert.deepEqual(skillKeys.filter((key) => !(key.replace(/\\n/g, "\n") in ru)), [], "untranslated skill strings");
  assert(ru.skill === undefined && (await import("../src/locales/ru.js")).default.plurals.skill, "the count has its Russian forms");
  // ---------------------------------------------------------------- usage
  // While the first scan runs the page says so; the numbers arrive with the
  // event that follows it.
  await page.evaluate(() => { window.usageEmpty = true; window.usageScanning = true; document.querySelectorAll(".toast").forEach((node) => node.remove()); });
  await page.locator('[data-action="home"]').first().click();
  await page.locator('[data-action="usage-page"]').click();
  await page.locator(".usage-empty", { hasText: "Reading transcripts" }).waitFor();
  await page.locator(".usage-status", { hasText: "Reading new transcript lines" }).waitFor();
  await page.evaluate(() => { window.usageEmpty = false; window.usageScanning = false; window.emitTauri("usage:updated", null); });
  await page.locator(".usage-tile").first().waitFor();
  assert.equal(await page.locator(".usage-tile").count(), 4);
  const lastUsage = () => page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "usage_stats").at(-1).args);
  assert.equal((await lastUsage()).days, 30);
  assert.equal(await page.locator(".usage-col").count(), 30);
  assert(await page.locator(".usage-bar--claude").count() > 20);
  assert(await page.locator(".usage-bar--codex").count() > 10);
  assert.equal(await page.locator("rect.usage-heat").count(), 84);
  assert.equal(await page.locator(".usage-legend__item").count(), 2);
  assert.match(await page.locator(".usage-tile", { hasText: "Estimated cost" }).textContent(), /estimate/);
  await page.locator(".usage-notice", { hasText: "gpt-6-astra" }).waitFor();
  const usageModels = page.locator(".usage-card", { hasText: "By model" }).locator("tbody tr");
  assert.equal(await usageModels.count(), 3);
  assert.equal((await usageModels.filter({ hasText: "gpt-6-astra" }).locator("td").last().textContent()).trim(), "—");
  const usageProjects = page.locator(".usage-card", { hasText: "By project" }).locator("tbody tr");
  assert.deepEqual((await usageProjects.locator(".usage-table__name").allTextContents()).map((text) => text.trim()), ["Example", "Other", "No project"]);
  // A tooltip on hover, and on the heatmap too.
  await page.locator(".usage-col").nth(25).hover();
  const usageTip = page.locator(".usage-tip");
  await usageTip.waitFor({ state: "visible" });
  assert.match(await usageTip.textContent(), /Claude/);
  assert.match(await usageTip.textContent(), /Estimated cost/);
  await page.locator("rect.usage-heat").nth(80).hover();
  assert.match(await usageTip.textContent(), /tokens/);
  if (shots) {
    await page.setViewportSize({ width: 1280, height: 1240 });
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.locator(".usage-col").nth(22).hover();
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/usage-${theme}.png` });
      await page.locator('[data-scroll="usage"]').evaluate((node) => (node.scrollTop = node.scrollHeight));
      await page.mouse.move(5, 5);
      await page.waitForTimeout(100);
      await page.screenshot({ path: `${shots}/usage-tables-${theme}.png` });
      await page.locator('[data-scroll="usage"]').evaluate((node) => (node.scrollTop = 0));
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
    await page.setViewportSize({ width: 1280, height: 840 });
  }
  await page.mouse.move(5, 5);
  // Range, project and agent each ask again.
  await page.locator('[data-usage-days="7"]').click();
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "usage_stats").at(-1).args.days === 7);
  await page.locator(".usage-col").nth(6).waitFor();
  assert.equal(await page.locator(".usage-col").count(), 7);
  await page.locator("#usage-project").selectOption("other");
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "usage_stats").at(-1).args.projectId === "other");
  await page.locator('[data-usage-agent="codex"]').click();
  await page.waitForFunction(() => window.checkCalls.filter((c) => c.cmd === "usage_stats").at(-1).args.agent === "codex");
  assert.deepEqual(await lastUsage(), { days: 7, projectId: "other", agent: "codex" });
  // One series: no legend box, and the colour stays Codex's.
  await page.locator('[data-usage-agent="codex"][aria-pressed="true"]').waitFor();
  await page.waitForFunction(() => !document.querySelector(".usage--refreshing"));
  assert.equal(await page.locator(".usage-legend").count(), 0);
  assert.equal(await page.locator(".usage-bar--claude").count(), 0);
  assert(await page.locator(".usage-bar--codex").count() > 0);
  // The chart's table twin.
  await page.locator("[data-usage-table]").click();
  assert.equal(await page.locator(".usage-card", { hasText: "Tokens per day" }).locator("tbody tr").count(), 7);
  await page.locator("[data-usage-table]").click();
  await page.locator('[data-usage-agent=""]').click();
  await page.locator("#usage-project").selectOption("");
  await page.locator('[data-usage-days="30"]').click();
  await page.waitForFunction(() => { const a = window.checkCalls.filter((c) => c.cmd === "usage_stats").at(-1).args; return a.days === 30 && !a.projectId && !a.agent; });
  // The palette has it.
  await page.locator('[data-action="home"]').first().click();
  await page.keyboard.press(`${primary}+k`);
  await page.locator("#palette-input").fill("Usage");
  await page.locator(".palette__row", { hasText: "Tokens and estimated cost" }).first().click();
  await page.locator(".usage-tile").first().waitFor();
  // Prices: from the notice to the editor; an edit saves, a model seen with
  // no price is one click to add, and the defaults come back.
  await page.locator(".usage-notice [data-settings-open]").click();
  await page.getByRole("heading", { name: "Usage prices" }).waitFor();
  await page.locator("#settings-search").fill("");
  await page.locator('.prefs__item[data-settings-section="Usage prices"][aria-current="true"]').waitFor();
  const priceRows = page.locator(".usage-prices__table tbody tr");
  await priceRows.nth(1).waitFor();
  assert.equal(await priceRows.count(), 2);
  assert.equal(await page.locator("#price-0-input").inputValue(), "4");
  assert(await page.locator('[data-price-save="1"][disabled]').count());
  await page.locator("#price-0-input").fill("4.5");
  await page.locator(".usage-prices__dirty").waitFor();
  await page.locator('.usage-chip[data-price-add="gpt-6-astra"]').click();
  assert.equal(await priceRows.count(), 3);
  assert.equal(await page.locator("#price-2-model").inputValue(), "gpt-6-astra");
  await page.locator("#price-2-input").fill("1.25");
  await page.locator("#price-2-output").fill("10");
  if (shots) {
    for (const theme of ["light", "dark"]) {
      await page.evaluate((value) => document.documentElement.setAttribute("data-theme", value), theme);
      await page.waitForTimeout(150);
      await page.screenshot({ path: `${shots}/usage-prices-${theme}.png` });
    }
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  }
  await page.locator('[data-price-save="1"]').click();
  await page.locator(".toast", { hasText: "Prices saved" }).waitFor();
  const savedPrices = await page.evaluate(() => window.checkCalls.filter((c) => c.cmd === "usage_prices_save").at(-1).args.prices);
  assert.equal(savedPrices.length, 3);
  assert.equal(savedPrices[0].input, 4.5);
  assert.deepEqual([savedPrices[2].model, savedPrices[2].input, savedPrices[2].output, savedPrices[2].cache_read], ["gpt-6-astra", 1.25, 10, null]);
  await page.locator('[data-price-save="1"][disabled]').waitFor();
  await page.locator('[data-price-remove="2"]').click();
  assert.equal(await priceRows.count(), 2);
  await page.locator('[data-price-reset="1"]').click();
  await page.waitForFunction(() => window.checkCalls.some((c) => c.cmd === "usage_prices_reset"));
  await page.waitForFunction(() => document.querySelectorAll(".usage-prices__table tbody tr").length === 1);
  await page.locator('[data-action="close-settings"]').click();
  // Every string the Usage page and the price editor show has its Russian.
  const usageSources = await Promise.all(["../src/views-usage.js", "../src/usage.js"].map((path) => readFile(new URL(path, import.meta.url), "utf8")));
  const usageKeys = usageSources.flatMap((source) => [...source.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)].map((match) => match[1]));
  usageKeys.push("Usage", "Usage prices", "Tokens and estimated cost by day, model and project", "Model", "Input", "Output", "Cache read", "Cache write", "Reasoning", "Total", "Estimated cost", "Cache write 5m", "Cache write 1h");
  assert(usageKeys.length > 50, "the usage strings were found");
  assert.deepEqual(usageKeys.filter((key) => !(key in ru)), [], "untranslated usage strings");
  for (const word of ["model", "request"]) assert(word in (await import("../src/locales/ru.js")).default.plurals, `${word} has its Russian plural`);

  assert.equal(await page.locator(".toast--error").count(), 0);
  assert.deepEqual(errors, []);
  assert.equal(await page.locator(".toast--error").count(), 0);
  if (process.env.CONVOY_TEST_SCREENSHOT) await page.screenshot({ path: process.env.CONVOY_TEST_SCREENSHOT });
  console.log("PASS: folder → builder → review → feedback; Linear MCP import → per-task model; sidebar menus; settled dialogs; settings page saves; chat session events, approval, composer and view switch; approvals inbox across sessions; manager sessions with nested helpers, live helper cards and the helpers menu; automations: editor, list, switch, run now, edit, delete; skills: templates, editor and preview, picker in New session, chat chip menu, delete; usage page: tiles, charts, tables, filters, tooltips, prices editor; no browser errors. IPC mocked.");
} finally {
  await browser?.close();
  await server.close();
}
