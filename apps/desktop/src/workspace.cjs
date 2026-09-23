const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { planning, validatePlanning } = require('./planning.cjs');

function validate(state) {
  if (!state || ![1, 2, 3].includes(state.schemaVersion) || !Array.isArray(state.projects) || !Array.isArray(state.sessions)) {
    throw new Error('Unsupported or damaged desktop workspace. The file has not been changed.');
  }
  const ids = new Set();
  for (const item of [...state.projects, ...state.sessions]) {
    if (!item || typeof item.id !== 'string' || ids.has(item.id) || typeof item.title !== 'string') throw new Error('Invalid workspace records.');
    ids.add(item.id);
  }
  for (const project of state.projects) {
    if (typeof project.path !== 'string' || !path.isAbsolute(project.path)) throw new Error('Invalid project path.');
  }
  for (const session of state.sessions) {
    if (!['claude', 'codex'].includes(session.agent) || typeof session.providerID !== 'string' ||
        typeof session.prompt !== 'string' || typeof session.started !== 'boolean' ||
        !state.projects.some(p => p.id === session.projectID)) throw new Error('Invalid session record.');
    if (session.providerID && !/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(session.providerID)) throw new Error('Invalid provider session ID.');
    if (session.agent === 'claude' && !session.providerID) throw new Error('Missing Claude session ID.');
    for (const key of ['notes', 'branch']) if (session[key] !== undefined && typeof session[key] !== 'string') throw new Error('Invalid session metadata.');
    for (const key of ['archived', 'pinned']) if (session[key] !== undefined && typeof session[key] !== 'boolean') throw new Error('Invalid session flag.');
    if (session.workingDirectory !== undefined && (typeof session.workingDirectory !== 'string' || !path.isAbsolute(session.workingDirectory))) throw new Error('Invalid worktree path.');
    if (session.agentHome !== undefined && (typeof session.agentHome !== 'string' || !path.isAbsolute(session.agentHome))) throw new Error('Invalid account path.');
    if (session.reviewOf && !state.sessions.some(s => s.id === session.reviewOf && s.id !== session.id && s.projectID === session.projectID)) throw new Error('Invalid review relationship.');
  }
  if (state.settings) validateSettings(state.settings);
  if (state.quickCommands !== undefined) {
    if (!Array.isArray(state.quickCommands)) throw new Error('Invalid quick commands.');
    const commandIDs = new Set();
    for (const command of state.quickCommands) {
      if (!command || typeof command.id !== 'string' || commandIDs.has(command.id) || typeof command.title !== 'string' || !command.title.trim() || command.title.length > 200 ||
          typeof command.text !== 'string' || !command.text.trim() || command.text.length > 32000 || typeof command.submit !== 'boolean' ||
          (command.projectID && !state.projects.some(p => p.id === command.projectID))) throw new Error('Invalid quick command.');
      commandIDs.add(command.id);
    }
  }
  validatePlanning(state);
  return state;
}

const defaults = { fontSize: 14, scrollback: 10000, theme: 'dark', defaultAgent: 'claude', notifications: false, keepAwake: 'off', hibernateMinutes: 0, shortcuts: {}, claudeUsage: false };
function validateSettings(settings) {
  if (!Number.isInteger(settings.fontSize) || settings.fontSize < 10 || settings.fontSize > 24 ||
      !Number.isInteger(settings.scrollback) || settings.scrollback < 1000 || settings.scrollback > 50000 ||
      !['dark', 'light', 'system'].includes(settings.theme) || !['claude', 'codex'].includes(settings.defaultAgent)) throw new Error('Invalid settings.');
  if (settings.claudeUsage !== undefined && typeof settings.claudeUsage !== 'boolean') throw new Error('Invalid Claude usage setting.');
  if (settings.shortcuts !== undefined) {
    const allowed = ['palette', 'newSession', 'files', 'next', 'previous', 'settings', 'search'];
    if (!settings.shortcuts || typeof settings.shortcuts !== 'object' || Array.isArray(settings.shortcuts) || Object.entries(settings.shortcuts).some(([k, v]) => !allowed.includes(k) || typeof v !== 'string' || !/^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$/.test(v)) || new Set(Object.values(settings.shortcuts)).size !== Object.values(settings.shortcuts).length) throw new Error('Invalid or duplicate shortcut.');
  }
  if (settings.keepAwake !== undefined && !['off', 'always', 'sessions'].includes(settings.keepAwake)) throw new Error('Invalid keep-awake setting.');
  if (settings.hibernateMinutes !== undefined && (!Number.isInteger(settings.hibernateMinutes) || settings.hibernateMinutes < 0 || settings.hibernateMinutes > 1440)) throw new Error('Invalid hibernation delay.');
  if (settings.notifications !== undefined && typeof settings.notifications !== 'boolean') throw new Error('Invalid notification setting.');
}

class Workspace {
  constructor(file) {
    this.file = file;
    this.state = fs.existsSync(file) ? validate(JSON.parse(fs.readFileSync(file, 'utf8'))) : { schemaVersion: 3, projects: [], sessions: [] };
    // Additive migration: keep all existing IDs/provider identities and unknown fields.
    this.state = { ...this.state, schemaVersion: 3, settings: { ...defaults, ...this.state.settings }, quickCommands: this.state.quickCommands || [],
      specs: this.state.specs || [], tasks: this.state.tasks || [], profiles: this.state.profiles || [], activity: this.state.activity || [] };
    for (const task of this.state.tasks) if (task.status === 'building') { task.status = 'failed'; task.lastError = 'App closed while the session was running. Resume its saved session explicitly.'; }
  }
  update(change) {
    const next = structuredClone(this.state);
    change(next);
    validate(next);
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    const temp = `${this.file}.${randomUUID()}.tmp`;
    try {
      fs.writeFileSync(temp, JSON.stringify(next, null, 2), { mode: 0o600 });
      fs.renameSync(temp, this.file);
    } finally { fs.rmSync(temp, { force: true }); }
    this.state = next;
    return next;
  }
  editProject(id, patch) {
    const keys = ['title', 'group', 'color', 'icon', 'path', 'setupCommand', 'sharedPaths', 'reviewTemplate'];
    if (!patch || Object.keys(patch).some(k => !keys.includes(k)) || Object.values(patch).some(v => typeof v !== 'string' || v.length > 4096)) throw new Error('Invalid project settings.');
    if (patch.title !== undefined && !patch.title.trim()) throw new Error('Enter a project name.');
    if (patch.path && (!path.isAbsolute(patch.path) || !fs.statSync(patch.path).isDirectory())) throw new Error('Invalid folder.');
    return this.update(next => { const project = next.projects.find(p => p.id === id); if (!project) throw new Error('Project not found.'); Object.assign(project, patch); });
  }
  removeProject(id) {
    return this.update(next => {
      const removed = new Set(next.sessions.filter(s => s.projectID === id).map(s => s.id));
      for (const key of ['projects', 'sessions', 'specs', 'tasks', 'quickCommands']) next[key] = next[key].filter(item => key === 'projects' ? item.id !== id : item.projectID !== id);
      next.activity = next.activity.filter(item => !removed.has(item.sessionID));
    });
  }
  recoverSession(id) {
    const old = this.session(id);
    if (old.worktreeRemoved) throw new Error('Restore the worktree before creating a recovery session.');
    return this.update(next => {
      const fresh = { ...old, id: randomUUID(), title: `${old.title.slice(0, 180)} · recovery`, started: false, providerID: old.agent === 'claude' ? randomUUID() : '', archived: false, ownsWorktree: false, prompt: '' };
      delete fresh.taskID;
      next.sessions.push(fresh);
    });
  }
  addProject(directory) {
    if (!fs.statSync(directory).isDirectory()) throw new Error('Choose a folder.');
    const resolved = fs.realpathSync(directory);
    return this.update(state => {
      const same = p => process.platform === 'win32' ? p.path.toLowerCase() === resolved.toLowerCase() : p.path === resolved;
      if (!state.projects.some(same)) state.projects.push({ id: randomUUID(), title: path.basename(resolved) || resolved, path: resolved });
    });
  }
  addSession({ projectID, agent, title, prompt = '', reviewOf, profileID, model = '' }) {
    if (!['claude', 'codex'].includes(agent) || typeof title !== 'string' || !title.trim() || title.length > 200 || typeof prompt !== 'string' || prompt.length > 32000) throw new Error('Invalid session details.');
    if (typeof model !== 'string' || model.length > 100 || !/^[a-zA-Z0-9._:/-]*$/.test(model)) throw new Error('Invalid model name.');
    return this.update(state => {
      const source = reviewOf ? this.session(reviewOf) : null;
      if (source && source.projectID !== projectID) throw new Error('Reviews must use the builder project.');
      state.sessions.push({ id: randomUUID(), projectID, agent, model, title: title.trim(), prompt,
        providerID: agent === 'claude' ? randomUUID() : '', started: false, notes: '', archived: false, pinned: false, ...(profileID ? { profileID } : {}),
        ...(source ? { reviewOf, workingDirectory: source.workingDirectory, branch: source.branch } : {}) });
    });
  }
  session(id) {
    const session = this.state.sessions.find(s => s.id === id);
    if (!session) throw new Error('Session not found.');
    return session;
  }
  editSession(id, patch, running = false) {
    const allowed = ['title', 'notes', 'providerID', 'archived', 'pinned'];
    if (!patch || Object.keys(patch).some(key => !allowed.includes(key))) throw new Error('Invalid session edit.');
    if (patch.title !== undefined && (typeof patch.title !== 'string' || !patch.title.trim() || patch.title.length > 200)) throw new Error('Enter a session name.');
    if (patch.notes !== undefined && (typeof patch.notes !== 'string' || patch.notes.length > 32000)) throw new Error('Notes are too long.');
    if (running && (patch.archived === true || patch.providerID !== undefined)) throw new Error('Stop the session before archiving or changing its provider ID.');
    this.session(id);
    return this.update(next => Object.assign(next.sessions.find(s => s.id === id), patch));
  }
  saveSettings(settings) {
    const next = Object.fromEntries(Object.keys(defaults).map(key => [key, settings[key] ?? this.state.settings[key] ?? defaults[key]]));
    validateSettings(next);
    return this.update(state => { state.settings = next; });
  }
  saveCommand(command) {
    return this.update(state => {
      const value = { id: command.id || randomUUID(), title: command.title, text: command.text, submit: command.submit, projectID: command.projectID || undefined };
      const index = state.quickCommands.findIndex(c => c.id === value.id);
      if (index < 0) state.quickCommands.push(value); else state.quickCommands[index] = value;
    });
  }
}
Object.assign(Workspace.prototype, planning);
module.exports = { Workspace, validate, defaults };
