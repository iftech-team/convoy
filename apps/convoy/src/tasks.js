// The Tasks tab, and importing Linear and Jira issues into it.
//
// Every rule — the request, the parsing, dedupe, the task brief — lives in
// convoy-core. This file draws state and passes input back, as main.js does.

import { invoke } from "@tauri-apps/api/core";

const MODELS = {
  claude: [["opus", "Opus · most capable"], ["sonnet", "Sonnet · balanced"], ["haiku", "Haiku · fastest"]],
  codex: [["gpt-5-codex", "GPT-5 Codex"], ["gpt-5", "GPT-5"], ["o3", "o3"]],
};
const AGENTS = [["claude", "Claude Code"], ["codex", "Codex"]];
const MODES = [["pr", "Pull request"], ["push", "Push"], ["none", "Commit only"]];
const STATUS = {
  queued: ["Queued", "state"],
  building: ["Building", "state state--running"],
  review: ["Needs review", "state"],
  changes: ["Changes requested", "state state--failed"],
  done: ["Done", "state state--done"],
  failed: ["Failed", "state state--failed"],
};
const AUTH = {
  apiKey: { linear: "API key", jira: "API token" },
  password: { linear: "Login & password", jira: "Login & password" },
  mcp: { linear: "Agent MCP server", jira: "Agent MCP server" },
};
const authOptions = (kind) => (kind === "linear" ? ["apiKey", "mcp"] : ["apiKey", "password", "mcp"]);
const trackerName = (kind) => (kind === "jira" ? "Jira" : "Linear");

export function installTasks({ state, escape, icons, call, callDone, toast, render, loadWorkspace, start, launch }) {
  state.tasks = [];
  state.integrations = [];
  // Bumped by every search: an answer for an older search, or for another
  // connection, is dropped rather than shown under the current one.
  let searchSeq = 0;
  let parseSeq = 0;

  const project = () => state.projects.find((item) => item.id === state.projectId);
  const connection = () => state.integrations.find((item) => item.id === state.dialog?.connectionId);

  // ------------------------------------------------------------ helpers --

  const modelList = (agent) =>
    `<datalist id="models-${agent}">${MODELS[agent].map(([id, name]) => `<option value="${id}">${escape(name)}</option>`).join("")}</datalist>`;

  const agentSelect = (attrs, value) =>
    `<select ${attrs}>${AGENTS.map(([id, name]) => `<option value="${id}"${id === value ? " selected" : ""}>${name}</option>`).join("")}</select>`;

  const modelInput = (attrs, agent, value) =>
    `<input ${attrs} list="models-${agent}" value="${escape(value || "")}" placeholder="Agent default" spellcheck="false" />`;

  const sourceBadge = (source) => {
    if (!source) return "";
    const label = `${trackerName(source.tracker)} ${source.key}`;
    return source.url
      ? `<a class="badge badge--muted" href="${escape(source.url)}" target="_blank" rel="noreferrer" title="Open ${escape(label)}">${escape(source.key)}</a>`
      : `<span class="badge badge--muted" title="${escape(label)}${source.viaMCP ? " — fetched by the agent via MCP" : ""}">${escape(source.key)}</span>`;
  };

  // Keys of issues already imported into this project from the same site.
  const importedKeys = () => {
    const kind = connection()?.kind;
    const taken = new Set(
      state.tasks
        .filter((task) => task.source && task.source.tracker === kind)
        .map((task) => `${task.source.origin || ""}\n${task.source.key}`),
    );
    return new Set(candidates().filter((issue) => taken.has(`${issue.origin || ""}\n${issue.key}`)).map((issue) => issue.key));
  };

  const candidates = () => {
    const draft = state.dialog;
    if (!draft || draft.kind !== "import") return [];
    const current = connection();
    if (current?.auth === "mcp") return draft.manualIssues;
    return draft.resultsFor === draft.connectionId ? draft.results : [];
  };

  const chosen = () => {
    const taken = importedKeys();
    return candidates().filter((issue) => state.dialog.selected.has(issue.key) && !taken.has(issue.key));
  };

  // ---------------------------------------------------------------- data --

  async function loadTasks() {
    if (!state.projectId) {
      state.tasks = [];
      return;
    }
    state.tasks = (await call("tasks_for", { projectId: state.projectId })) ?? [];
  }

  async function loadIntegrations() {
    state.integrations = (await call("integrations_list")) ?? [];
  }

  async function search() {
    const draft = state.dialog;
    const current = connection();
    const seq = ++searchSeq;
    if (!current || current.auth === "mcp") {
      draft.loading = false;
      return render();
    }
    draft.loading = true;
    draft.message = null;
    render();
    const connectionId = current.id;
    let issues;
    try {
      issues = await invoke("issues_search", {
        connectionId,
        query: draft.query,
        mineOnly: draft.mineOnly,
        rawJql: current.kind === "jira" && draft.rawJql,
      });
    } catch (error) {
      if (seq !== searchSeq || state.dialog !== draft || draft.connectionId !== connectionId) return;
      Object.assign(draft, { loading: false, results: [], resultsFor: connectionId, message: String(error) });
      return render();
    }
    if (seq !== searchSeq || state.dialog !== draft || draft.connectionId !== connectionId) return;
    draft.results = issues;
    draft.resultsFor = connectionId;
    draft.loading = false;
    const taken = importedKeys();
    draft.selected = new Set(issues.map((issue) => issue.key).filter((key) => !taken.has(key)));
    draft.message = issues.length ? null : "No matching open issues.";
    render();
  }

  async function parseManual() {
    const draft = state.dialog;
    const seq = ++parseSeq;
    const issues = await call("issues_parse_keys", { connectionId: draft.connectionId, text: draft.manual });
    if (seq !== parseSeq || state.dialog !== draft || !issues) return;
    draft.manualIssues = issues;
    draft.selected = new Set(issues.map((issue) => issue.key));
    render();
  }

  async function openImport() {
    await Promise.all([loadIntegrations(), loadTasks()]);
    const first = state.integrations[0];
    state.dialog = {
      kind: "import",
      connectionId: first?.id ?? null,
      query: "",
      mineOnly: true,
      rawJql: false,
      manual: "",
      manualIssues: [],
      results: [],
      resultsFor: null,
      selected: new Set(),
      overrides: {},
      agent: state.settings.default_agent || "claude",
      model: "",
      mode: "pr",
      autoReview: false,
      runNow: false,
      loading: false,
      message: null,
    };
    if (first && first.auth !== "mcp") return search();
    render();
  }

  async function runImport() {
    const draft = state.dialog;
    const issues = chosen();
    if (!issues.length) return;
    const overrides = Object.fromEntries(
      Object.entries(draft.overrides).filter(([key]) => issues.some((issue) => issue.key === key)),
    );
    const result = await call("issues_import", {
      projectId: state.projectId,
      connectionId: draft.connectionId,
      issues,
      options: { agent: draft.agent, model: draft.model, mode: draft.mode, auto_review: draft.autoReview, overrides },
    });
    if (!result) return;
    state.dialog = null;
    state.tab = "tasks";
    await loadTasks();
    const skipped = result.skipped ? `, ${result.skipped} already imported` : "";
    toast(`Imported ${result.created.length} task${result.created.length === 1 ? "" : "s"}${skipped}`);
    if (draft.runNow) {
      let started = 0;
      for (const id of result.created) if (await runTask(id, { background: true })) started += 1;
      if (started) toast(`Started ${started} task${started === 1 ? "" : "s"}`);
    }
    await loadWorkspace();
  }

  /** Prepares the task's session and starts it; in the foreground it opens the terminal. */
  async function runTask(id, { background = false } = {}) {
    const sessionId = await call("task_prepare", { id });
    if (!sessionId) return false;
    await loadWorkspace();
    if (background) return launch(sessionId);
    await start(sessionId);
    return true;
  }

  async function saveConnection(test) {
    const draft = state.dialog;
    const input = { ...draft.connection, secret: draft.secret };
    const saved = await call("integration_save", { input });
    if (!saved) return;
    await loadIntegrations();
    if (test && saved.auth !== "mcp") {
      draft.connection = { ...saved };
      draft.secret = "";
      draft.testing = true;
      render();
      try {
        const issues = await invoke("issues_search", { connectionId: saved.id, query: "", mineOnly: true, rawJql: false });
        draft.result = { ok: true, text: `Connected — ${issues.length} open issue${issues.length === 1 ? "" : "s"} assigned to you.` };
      } catch (error) {
        draft.result = { ok: false, text: String(error) };
      }
      draft.testing = false;
      return render();
    }
    state.dialog = { kind: "integrations" };
    render();
  }

  // --------------------------------------------------------------- views --

  function taskList() {
    const rows = state.tasks
      .map((task) => {
        const [label, css] = STATUS[task.status] ?? [task.status, "state"];
        const meta = [
          task.agent === "claude" ? "Claude Code" : "Codex",
          task.model ? `<code>${escape(task.model)}</code>` : "default model",
          MODES.find(([id]) => id === task.mode)?.[1],
          task.last_error ? escape(task.last_error) : null,
        ]
          .filter(Boolean)
          .join(" · ");
        const runnable = ["queued", "failed", "changes"].includes(task.status);
        return `
        <div class="row">
          <span class="row__mark">${task.agent === "claude" ? "✳" : "◉"}</span>
          <div class="row__body">
            <div class="row__title">${escape(task.title)}</div>
            <div class="row__meta">${meta}</div>
          </div>
          <div class="row__actions">
            ${sourceBadge(task.source)}
            <span class="${css}"><span class="state__dot"></span>${label}</span>
            ${runnable ? `<button class="button" data-task-edit="${escape(task.id)}" title="Agent and model">${icons.gear}</button>` : ""}
            ${
              runnable
                ? `<button class="button button--primary" data-task-run="${escape(task.id)}">${icons.play} Run</button>`
                : task.session_id
                  ? `<button class="button button--quiet" data-open="${escape(task.session_id)}" title="Open the terminal">${icons.open}</button>`
                  : ""
            }
            ${
              task.status === "review"
                ? `<button class="button" data-task-done="${escape(task.id)}">Mark done</button>`
                : ""
            }
          </div>
        </div>`;
      })
      .join("");

    return `
      <div class="section">
        <h2 class="section__title">Tasks <span class="section__count">${state.tasks.length}</span></h2>
        <span class="section__spacer"></span>
        <button class="button button--icon" data-action="tasks-refresh" title="Refresh">${icons.refresh}</button>
        <button class="button" data-action="integrations">Linear &amp; Jira…</button>
        <button class="button button--primary" data-action="import">${icons.plus} Import issues</button>
      </div>
      <p class="section__hint">
        Import Linear or Jira issues, choose the agent and model for all of them or per issue, then run each in its
        own session.
      </p>
      <div class="list">${
        rows ||
        `<div class="empty"><h2 class="empty__title">No tasks yet</h2>
           <p class="empty__text">Import issues from Linear or Jira to start.</p></div>`
      }</div>`;
  }

  function importDialog() {
    const draft = state.dialog;
    const current = connection();
    const list = candidates();
    const taken = importedKeys();
    const picked = chosen();

    if (!state.integrations.length) {
      return modal("Import issues", `In ${escape(project()?.title ?? "")}`, `
        <p class="empty__text">Connect Linear or Jira first: an API key, a Jira login, or the agent’s own MCP server.</p>`,
        `<button class="button" data-dismiss="1">Cancel</button>
         <button class="button button--primary" data-action="integrations">Connect Linear or Jira</button>`);
    }

    const source = `
      <div class="field">
        <span class="field__label">From</span>
        <div class="import__source">
          <select id="imp-connection">${state.integrations
            .map((item) => `<option value="${escape(item.id)}"${item.id === draft.connectionId ? " selected" : ""}>${escape(item.name)} · ${AUTH[item.auth][item.kind]}</option>`)
            .join("")}</select>
          <button class="button button--quiet" data-action="integrations">Manage…</button>
        </div>
      </div>`;

    const finder =
      current?.auth === "mcp"
        ? `<label class="field">
             <span class="field__label">Issue keys or links</span>
             <textarea id="imp-manual" spellcheck="false" placeholder="ENG-123 Optional title&#10;https://linear.app/acme/issue/ENG-124/…">${escape(draft.manual)}</textarea>
             <span class="field__note">The agent reads each issue itself through its ${trackerName(current.kind)} MCP server, which must be configured in Claude Code or Codex.</span>
           </label>`
        : `<div class="import__search">
             <input id="imp-query" value="${escape(draft.query)}" spellcheck="false"
                    placeholder="${current?.kind === "jira" ? (draft.rawJql ? "JQL, e.g. project in (PAY, OPS)" : "Text, issue key, or JQL") : "Title text or issue key, e.g. ENG-123"}" />
             ${current?.kind === "jira" ? `<label class="import__check"><input type="checkbox" id="imp-jql"${draft.rawJql ? " checked" : ""} /> JQL</label>` : ""}
             <label class="import__check"><input type="checkbox" id="imp-mine"${draft.mineOnly ? " checked" : ""} /> Assigned to me</label>
             <button class="button" data-action="import-search"${draft.loading ? " disabled" : ""}>${draft.loading ? "Searching…" : "Search"}</button>
           </div>`;

    const rows = list
      .map((issue) => {
        const done = taken.has(issue.key);
        const pick = draft.overrides[issue.key] ?? { agent: draft.agent, model: draft.model };
        const custom = Boolean(draft.overrides[issue.key]);
        return `
        <div class="import__row${done ? " import__row--done" : ""}">
          <input type="checkbox" data-issue="${escape(issue.key)}"${draft.selected.has(issue.key) && !done ? " checked" : ""}${done ? " disabled" : ""} />
          <code class="import__key">${escape(issue.key)}</code>
          <div class="import__title">
            <div>${escape(issue.title)}</div>
            <div class="import__meta">${done ? "Already imported" : escape([issue.status, issue.priority].filter(Boolean).join(" · "))}</div>
          </div>
          ${
            done
              ? ""
              : `<div class="import__pick${custom ? " import__pick--custom" : ""}" title="Agent and model for this issue only">
                   ${agentSelect(`data-row-agent="${escape(issue.key)}"`, pick.agent)}
                   ${modelInput(`data-row-model="${escape(issue.key)}"`, pick.agent, pick.model)}
                 </div>`
          }
        </div>`;
      })
      .join("");

    const allKeys = list.filter((issue) => !taken.has(issue.key)).map((issue) => issue.key);
    const everything = allKeys.length && allKeys.every((key) => draft.selected.has(key));
    const body = `
      ${source}
      ${finder}
      <div class="field">
        <span class="field__label">Issues ${list.length ? `· ${picked.length} of ${list.length} selected` : ""}
          ${allKeys.length ? `<button class="button button--quiet" data-action="import-all">${everything ? "Select none" : "Select all"}</button>` : ""}</span>
        <div class="import__list">${
          rows ||
          `<div class="import__empty${draft.message ? " import__empty--error" : ""}">${escape(
            draft.message ?? (draft.loading ? "Loading…" : current?.auth === "mcp" ? "Paste issue keys or links above." : "No issues loaded."),
          )}</div>`
        }</div>
        ${draft.message && rows ? `<span class="field__note">${escape(draft.message)}</span>` : ""}
      </div>
      <div class="import__defaults">
        <label class="field">
          <span class="field__label">Agent for all</span>
          ${agentSelect('id="imp-agent"', draft.agent)}
        </label>
        <label class="field">
          <span class="field__label">Model for all</span>
          ${modelInput('id="imp-model"', draft.agent, draft.model)}
        </label>
        <label class="field">
          <span class="field__label">When done</span>
          <select id="imp-mode">${MODES.map(([id, name]) => `<option value="${id}"${id === draft.mode ? " selected" : ""}>${name}</option>`).join("")}</select>
        </label>
      </div>
      ${modelList("claude")}${modelList("codex")}`;

    const foot = `
      <label class="import__check"><input type="checkbox" id="imp-review"${draft.autoReview ? " checked" : ""} /> Auto-review</label>
      <label class="import__check"><input type="checkbox" id="imp-run"${draft.runNow ? " checked" : ""} /> Run immediately</label>
      <span class="section__spacer"></span>
      <button class="button" data-dismiss="1">Cancel</button>
      <button class="button button--primary" data-action="import-run"${picked.length ? "" : " disabled"}>
        ${draft.runNow ? icons.play : ""} Import${picked.length ? ` ${picked.length}` : ""}
      </button>`;
    return modal("Import issues", `In ${escape(project()?.title ?? "")}`, body, foot, "modal--wide");
  }

  function integrationsDialog() {
    const rows = state.integrations
      .map(
        (item) => `
        <div class="setting">
          <div class="setting__text">
            <div class="setting__title">${escape(item.name)}</div>
            <div class="setting__hint">${escape([trackerName(item.kind), AUTH[item.auth][item.kind], item.site, item.username].filter(Boolean).join(" · "))}</div>
          </div>
          <div class="setting__control">
            <button class="button" data-conn-edit="${escape(item.id)}">Edit</button>
            <button class="button button--danger" data-conn-remove="${escape(item.id)}">Remove</button>
          </div>
        </div>`,
      )
      .join("");
    return modal("Linear & Jira", "Import issues as tasks from the Tasks tab.", `
      <div class="group">
        <div class="group__label">Connections</div>
        ${rows || '<p class="setting__hint">None yet.</p>'}
      </div>
      <p class="modal__note">
        Keys, tokens and passwords are kept in <code>integrations.json</code> beside the workspace, readable by your
        user only. MCP connections store nothing: the agent fetches each issue with its own MCP server.
      </p>`,
      `<button class="button" data-dismiss="1">Close</button>
       <button class="button" data-conn-add="jira">${icons.plus} Jira</button>
       <button class="button button--primary" data-conn-add="linear">${icons.plus} Linear</button>`);
  }

  function connectionDialog() {
    const draft = state.dialog;
    const item = draft.connection;
    const needsSecret = item.auth !== "mcp";
    const help =
      item.auth === "mcp"
        ? item.kind === "linear"
          ? "Add Linear’s MCP server to your agent, e.g. claude mcp add --transport http linear https://mcp.linear.app/mcp, then import by issue key or link."
          : "Add the Atlassian MCP server to your agent, then import by issue key or link."
        : item.kind === "linear"
          ? "Create a key in Linear → Settings → Security & access → Personal API keys."
          : item.auth === "password"
            ? "Jira Server / Data Center with basic authentication. Jira Cloud needs an API token instead."
            : "Jira Cloud: your account email plus an API token from id.atlassian.com. Data Center: leave the email empty and paste a personal access token.";
    const secretLabel = item.kind === "linear" ? "Personal API key" : item.auth === "password" ? "Password" : "API token or personal access token";
    const body = `
      <label class="field">
        <span class="field__label">Name</span>
        <input id="conn-name" value="${escape(item.name)}" spellcheck="false" />
      </label>
      <div class="field">
        <span class="field__label">Connect with</span>
        <div class="choice">${authOptions(item.kind)
          .map((auth) => `<button data-conn-auth="${auth}" aria-pressed="${item.auth === auth}">${AUTH[auth][item.kind]}</button>`)
          .join("")}</div>
      </div>
      ${
        item.kind === "jira" && needsSecret
          ? `<label class="field">
               <span class="field__label">Site URL</span>
               <input id="conn-site" value="${escape(item.site || "")}" placeholder="https://acme.atlassian.net" spellcheck="false" />
             </label>
             <label class="field">
               <span class="field__label">${item.auth === "password" ? "Username" : "Email"}</span>
               <input id="conn-user" value="${escape(item.username || "")}" spellcheck="false" />
             </label>`
          : ""
      }
      ${
        needsSecret
          ? `<label class="field">
               <span class="field__label">${secretLabel}</span>
               <input id="conn-secret" type="password" value="${escape(draft.secret)}"
                      placeholder="${draft.hasSecret ? "Saved — leave empty to keep" : ""}" autocomplete="off" />
             </label>`
          : ""
      }
      <span class="field__note">${escape(help)}</span>
      ${draft.result ? `<span class="field__note" style="color:var(${draft.result.ok ? "--state-done" : "--state-failed"})">${escape(draft.result.text)}</span>` : ""}`;
    const foot = `
      <button class="button" data-action="integrations">Back</button>
      <span class="section__spacer"></span>
      ${needsSecret ? `<button class="button" data-action="conn-test"${draft.testing ? " disabled" : ""}>${draft.testing ? "Testing…" : "Save & test"}</button>` : ""}
      <button class="button button--primary" data-action="conn-save">Save</button>`;
    return modal(`${item.id ? "Edit" : "Add"} ${trackerName(item.kind)} connection`, "", body, foot);
  }

  function taskDialog() {
    const draft = state.dialog;
    return modal("Agent and model", escape(draft.title), `
      <label class="field">
        <span class="field__label">Agent</span>
        ${agentSelect('id="task-agent"', draft.agent)}
      </label>
      <label class="field">
        <span class="field__label">Model</span>
        ${modelInput('id="task-model"', draft.agent, draft.model)}
        <span class="field__note">Passed as --model; leave empty for the CLI default.</span>
      </label>
      ${modelList("claude")}${modelList("codex")}`,
      `<button class="button" data-dismiss="1">Cancel</button>
       <button class="button button--primary" data-action="task-save">Save</button>`);
  }

  const modal = (title, hint, body, foot, extra = "") => `
    <div class="scrim" data-dismiss="1">
      <div class="modal ${extra}" role="dialog" aria-modal="true" aria-label="${escape(title)}">
        <div class="modal__head">
          <div class="modal__title">${escape(title)}</div>
          ${hint ? `<div class="modal__hint">${hint}</div>` : ""}
        </div>
        <div class="modal__body">${body}</div>
        <div class="modal__foot">${foot}</div>
      </div>
    </div>`;

  function dialog() {
    switch (state.dialog?.kind) {
      case "import":
        return importDialog();
      case "integrations":
        return integrationsDialog();
      case "connection":
        return connectionDialog();
      case "task":
        return taskDialog();
      default:
        return null;
    }
  }

  // ------------------------------------------------------------- events --

  // Values typed into the connection form, read back before any re-render.
  function readConnection() {
    const draft = state.dialog;
    if (draft?.kind !== "connection") return;
    const value = (id, fallback) => document.querySelector(id)?.value ?? fallback;
    draft.connection.name = value("#conn-name", draft.connection.name);
    draft.connection.site = value("#conn-site", draft.connection.site);
    draft.connection.username = value("#conn-user", draft.connection.username);
    draft.secret = value("#conn-secret", draft.secret);
  }

  async function onClick(target) {
    const data = target.dataset;
    if (data.taskRun) return runTask(data.taskRun), true;
    if (data.taskDone) {
      if (await callDone("task_status", { id: data.taskDone, status: "done" })) await loadTasks();
      return render(), true;
    }
    if (data.taskEdit) {
      const task = state.tasks.find((item) => item.id === data.taskEdit);
      state.dialog = { kind: "task", id: task.id, title: task.title, agent: task.agent, model: task.model || "" };
      return render(), true;
    }
    if (data.issue) {
      const selected = state.dialog.selected;
      if (target.checked) selected.add(data.issue);
      else selected.delete(data.issue);
      return render(), true;
    }
    if (data.connAdd) {
      state.dialog = {
        kind: "connection",
        connection: { id: "", kind: data.connAdd, auth: "apiKey", name: trackerName(data.connAdd), site: "", username: "" },
        secret: "",
        hasSecret: false,
      };
      return render(), true;
    }
    if (data.connEdit) {
      const item = state.integrations.find((entry) => entry.id === data.connEdit);
      state.dialog = { kind: "connection", connection: { ...item }, secret: "", hasSecret: item.has_secret };
      return render(), true;
    }
    if (data.connRemove) {
      if (await callDone("integration_remove", { id: data.connRemove })) await loadIntegrations();
      return render(), true;
    }
    if (data.connAuth) {
      readConnection();
      state.dialog.connection.auth = data.connAuth;
      state.dialog.result = null;
      return render(), true;
    }

    switch (data.action) {
      case "import":
        return openImport(), true;
      case "tasks-refresh":
        await loadTasks();
        return render(), true;
      case "integrations":
        await loadIntegrations();
        state.dialog = { kind: "integrations" };
        return render(), true;
      case "import-search":
        return search(), true;
      case "import-all": {
        const taken = importedKeys();
        const keys = candidates().map((issue) => issue.key).filter((key) => !taken.has(key));
        const all = keys.every((key) => state.dialog.selected.has(key));
        state.dialog.selected = all ? new Set() : new Set(keys);
        return render(), true;
      }
      case "import-run":
        return runImport(), true;
      case "conn-save":
        readConnection();
        return saveConnection(false), true;
      case "conn-test":
        readConnection();
        return saveConnection(true), true;
      case "task-save": {
        const draft = state.dialog;
        const agent = document.querySelector("#task-agent")?.value ?? draft.agent;
        const model = document.querySelector("#task-model")?.value ?? draft.model;
        if (!(await callDone("task_agent", { id: draft.id, agent, model }))) return true;
        state.dialog = null;
        await loadTasks();
        return render(), true;
      }
    }
    return false;
  }

  /** Typing in dialog fields updates the draft without re-rendering; returns true when handled. */
  function onInput(field) {
    const draft = state.dialog;
    if (draft?.kind === "import") {
      if (field.id === "imp-query") draft.query = field.value;
      else if (field.id === "imp-model") draft.model = field.value;
      else if (field.id === "imp-manual") {
        draft.manual = field.value;
        parseManual();
      } else if (field.dataset.rowModel) {
        const key = field.dataset.rowModel;
        const pick = draft.overrides[key] ?? { agent: draft.agent, model: draft.model };
        draft.overrides[key] = { ...pick, model: field.value };
        if (draft.overrides[key].agent === draft.agent && draft.overrides[key].model === draft.model) delete draft.overrides[key];
      } else return false;
      return true;
    }
    return draft?.kind === "connection" || draft?.kind === "task";
  }

  /** Selects and checkboxes. */
  function onChange(field) {
    const draft = state.dialog;
    if (draft?.kind === "task" && field.id === "task-agent") {
      draft.agent = field.value;
      draft.model = "";
      return render(), true;
    }
    if (draft?.kind !== "import") return false;
    switch (field.id) {
      case "imp-connection":
        Object.assign(draft, { connectionId: field.value, results: [], resultsFor: null, selected: new Set(), overrides: {}, message: null, manual: "", manualIssues: [] });
        search();
        return true;
      case "imp-mine":
        draft.mineOnly = field.checked;
        search();
        return true;
      case "imp-jql":
        draft.rawJql = field.checked;
        return render(), true;
      case "imp-agent":
        draft.agent = field.value;
        draft.model = "";
        return render(), true;
      case "imp-mode":
        draft.mode = field.value;
        return true;
      case "imp-review":
        draft.autoReview = field.checked;
        return true;
      case "imp-run":
        draft.runNow = field.checked;
        return render(), true;
    }
    if (field.dataset.rowAgent) {
      const key = field.dataset.rowAgent;
      draft.overrides[key] = { agent: field.value, model: "" };
      if (field.value === draft.agent && !draft.model) delete draft.overrides[key];
      return render(), true;
    }
    return false;
  }

  /** Enter in the search box searches; elsewhere main.js decides. */
  function onEnter(field) {
    if (state.dialog?.kind === "import" && field.id === "imp-query") return search(), true;
    if (state.dialog?.kind === "connection") return readConnection(), saveConnection(false), true;
    return false;
  }

  return { taskList, dialog, loadTasks, onClick, onInput, onChange, onEnter };
}
