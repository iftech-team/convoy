const { randomUUID } = require('node:crypto');
const fields = ['title', 'problem', 'requirements', 'acceptance', 'constraints', 'plan'];
const statuses = ['queued', 'building', 'review', 'changes', 'done', 'failed'];
function text(value, max = 16000, required = false) {
  if (typeof value !== 'string' || value.length > max || (required && !value.trim())) throw new Error('Invalid or missing text.');
  return value;
}
function validatePlanning(state) {
  const ids = new Set();
  for (const collection of ['specs', 'tasks', 'profiles', 'activity']) {
    if (state[collection] === undefined) continue;
    if (!Array.isArray(state[collection])) throw new Error(`Invalid ${collection}.`);
    for (const item of state[collection]) {
      if (!item || typeof item.id !== 'string' || ids.has(item.id)) throw new Error(`Invalid ${collection} record.`);
      ids.add(item.id);
    }
  }
  for (const spec of state.specs || []) {
    fields.forEach(key => text(spec[key], key === 'title' ? 200 : 16000, key === 'title'));
    if (!state.projects.some(p => p.id === spec.projectID) || !Number.isSafeInteger(spec.revision) || spec.revision < 1 ||
        (spec.approvedRevision !== undefined && spec.approvedRevision !== spec.revision)) throw new Error('Invalid specification revision.');
  }
  for (const task of state.tasks || []) {
    text(task.title, 200, true); text(task.details); text(task.findings);
    if (!statuses.includes(task.status) || !['claude', 'codex'].includes(task.agent) || !state.projects.some(p => p.id === task.projectID) ||
        (task.specID && !state.specs?.some(s => s.id === task.specID && s.projectID === task.projectID)) ||
        (task.sessionID && !state.sessions.some(s => s.id === task.sessionID && s.taskID === task.id))) throw new Error('Invalid task.');
    if (task.specRevision !== undefined && (!Number.isSafeInteger(task.specRevision) || task.specRevision < 1)) throw new Error('Invalid task revision.');
  }
  for (const profile of state.profiles || []) {
    text(profile.label, 100, true);
    if (!['claude', 'codex'].includes(profile.agent)) throw new Error('Invalid account provider.');
  }
  for (const session of state.sessions) {
    if (session.profileID && !state.profiles?.some(p => p.id === session.profileID && p.agent === session.agent)) throw new Error('Invalid session account.');
    if (session.taskID && !state.tasks?.some(t => t.id === session.taskID && t.projectID === session.projectID)) throw new Error('Invalid task session.');
  }
  if ((state.activity || []).length > 200) throw new Error('Invalid activity history.');
  for (const item of state.activity || []) {
    text(item.title, 200); text(item.detail, 2000);
    if (!['started', 'resumed', 'exited', 'done', 'waiting', 'hibernated', 'worktree'].includes(item.kind) || typeof item.at !== 'string' || !Number.isFinite(Date.parse(item.at)) || !state.sessions.some(s => s.id === item.sessionID)) throw new Error('Invalid activity event.');
  }
}
function markdown(spec, tasks = []) {
  return `# ${spec.title}\n\nRevision: ${spec.revision} · ${spec.approvedRevision === spec.revision ? 'Approved' : 'Draft'}\n\n` +
    fields.slice(1).map(key => `## ${key[0].toUpperCase() + key.slice(1)}\n\n${spec[key]}\n`).join('\n') +
    '\n## Tasks\n\n' + tasks.filter(t => t.specID === spec.id).map(t => `- [${t.status === 'done' ? 'x' : ' '}] ${t.title} (${t.status})\n  ${t.details}\n  Findings: ${t.findings}`).join('\n') + '\n';
}
const planning = {
  saveSpec(input) {
    const value = Object.fromEntries(fields.map(key => [key, text(input[key] || '', key === 'title' ? 200 : 16000, key === 'title')]));
    return this.update(state => {
      const existing = input.id && state.specs.find(s => s.id === input.id);
      if (input.id && !existing) throw new Error('Specification not found.');
      if (existing) {
        if (!fields.some(key => existing[key] !== value[key])) return;
        Object.assign(existing, value, { revision: existing.revision + 1 });
        delete existing.approvedRevision;
        for (const task of state.tasks.filter(t => t.specID === existing.id && t.status === 'done')) task.status = 'changes';
      } else state.specs.push({ ...value, id: randomUUID(), projectID: input.projectID, revision: 1 });
    });
  },
  approveSpec(id, revision) {
    return this.update(state => {
      const spec = state.specs.find(s => s.id === id);
      if (!spec || spec.revision !== revision) throw new Error('The specification changed. Review the latest revision.');
      spec.approvedRevision = revision;
    });
  },
  saveTask(input) {
    if (input.mode !== undefined && !['none', 'pr', 'push'].includes(input.mode)) throw new Error('Invalid task publication mode.');
    const value = { mode: input.mode || 'none', autoReview: input.autoReview === true, title: text(input.title, 200, true), details: text(input.details || ''), findings: text(input.findings || ''), agent: input.agent };
    return this.update(state => {
      const existing = input.id && state.tasks.find(t => t.id === input.id);
      if (input.id && !existing) throw new Error('Task not found.');
      if (existing) {
        if (existing.status === 'building') throw new Error('Stop the task session before editing the task.');
        const changed = ['title', 'details', 'agent', 'mode', 'autoReview'].some(key => existing[key] !== value[key]);
        Object.assign(existing, value);
        if (changed) { existing.status = 'queued'; delete existing.sessionID; delete existing.specRevision; }
      } else state.tasks.push({ ...value, id: randomUUID(), projectID: input.projectID, specID: input.specID || undefined, status: 'queued' });
    });
  },
  setTaskStatus(id, status) {
    return this.update(state => {
      const task = state.tasks.find(t => t.id === id);
      if (!task || !['queued', 'review', 'changes', 'done'].includes(status)) throw new Error('Invalid task status.');
      if (task.status === 'building') throw new Error('Stop the task session before changing its status.');
      if (status === 'done' && task.specID) {
        const spec = state.specs.find(s => s.id === task.specID);
        if (spec.approvedRevision !== spec.revision) throw new Error('Approve the current specification before accepting this task.');
      }
      task.status = status;
    });
  },
  prepareTask(id, profileID) {
    return this.update(state => {
      const task = state.tasks.find(t => t.id === id);
      if (!task || task.status === 'building') throw new Error('Task unavailable.');
      const spec = state.specs.find(s => s.id === task.specID);
      if (spec && spec.approvedRevision !== spec.revision) throw new Error('Approve the specification before preparing a task session.');
      if (task.sessionID && task.specRevision === spec?.revision && !state.sessions.find(s => s.id === task.sessionID)?.archived) return;
      const publication = task.mode === 'pr' ? 'Commit your changes, push the current branch and create a pull request with gh pr create --fill. Report its URL. Do not merge.' : task.mode === 'push' ? 'Commit and push the current branch to its configured upstream. Never force push. Report the commit.' : 'Do not publish, push, or open a PR.';
      const prompt = `${spec ? markdown(spec) : ''}\n## Current task\n${task.title}\n\n${task.details}\n\nVerify the changes and report your results. ${publication}`;
      if (prompt.length > 32000) throw new Error('The specification and task exceed the session brief limit of 32,000 characters. Shorten them first.');
      const session = { id: randomUUID(), projectID: task.projectID, title: task.title, agent: task.agent, prompt,
        providerID: task.agent === 'claude' ? randomUUID() : '', started: false, taskID: id, profileID: profileID || undefined };
      state.sessions.push(session); task.sessionID = session.id; task.specRevision = spec?.revision;
    });
  },
  addProfile(label, agent) {
    text(label, 100, true);
    return this.update(state => {
      if (state.profiles.some(p => p.agent === agent && p.label.toLowerCase() === label.trim().toLowerCase())) throw new Error('That account label already exists.');
      state.profiles.push({ id: randomUUID(), label: label.trim(), agent });
    });
  },
  record(kind, session, detail) {
    return this.update(state => {
      state.activity.unshift({ id: randomUUID(), at: new Date().toISOString(), kind, sessionID: session.id, title: session.title, detail });
      state.activity = state.activity.slice(0, 200);
    });
  }
};
module.exports = { planning, validatePlanning, markdown, statuses };
