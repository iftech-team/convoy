import { installNavigation } from './navigation.js';
import { installFiles } from './files-ui.js';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

const api = window.convoy;
const $ = id => document.getElementById(id);
const defaults = { theme: 'dark', fontSize: 14, scrollback: 10000, defaultAgent: 'claude', notifications: false, keepAwake: 'off', hibernateMinutes: 0 };
let state = { projects: [], sessions: [], running: [], settings: defaults, quickCommands: [] };
let projectID, sessionID, reviewOf, editID, textTarget, textMode, worktreeID, commandID;
let panes = [], specID, taskID, prepareID;
const terminals = new Map();
const gitInfo = new Map();
const agentStates = new Map();
const colorPreference = matchMedia('(prefers-color-scheme: light)');
const showError = error => {
  const message = error.message || String(error);
  $('error').textContent = message; $('error').hidden = false;
  const dialog = document.querySelector('dialog[open]');
  if (dialog) {
    let node = dialog.querySelector('.modal-error');
    if (!node) { node = document.createElement('p'); node.className = 'modal-error'; node.setAttribute('role', 'alert'); dialog.prepend(node); }
    node.textContent = message;
  }
};
async function perform(action) {
  $('error').hidden = true;
  for (const node of document.querySelectorAll('.modal-error')) node.remove();
  try { return await action(); } catch (error) { showError(error); }
}
function update(next) { if (next) { state = { ...next, settings: { ...defaults, ...next.settings }, quickCommands: next.quickCommands || [], specs: next.specs || [], tasks: next.tasks || [], profiles: next.profiles || [], activity: next.activity || [] }; render(); if ($('planning-dialog').open) renderPlanning(); } }
function button(label, selected, click) {
  const node = document.createElement('button');
  node.type = 'button'; node.textContent = label;
  node.classList.toggle('selected', selected);
  node.setAttribute('aria-pressed', String(selected)); node.onclick = click;
  return node;
}
function selected() { return state.sessions.find(s => s.id === sessionID); }
function theme() {
  return state.settings.theme === 'light' || (state.settings.theme === 'system' && colorPreference.matches)
    ? { background: '#fafbfe', foreground: '#202637', cursor: '#3854a4' }
    : { background: '#111318', foreground: '#e4e7ee', cursor: '#88a5ff' };
}
function applySettings() {
  document.documentElement.dataset.theme = theme().background === '#fafbfe' ? 'light' : 'dark';
  for (const entry of terminals.values()) {
    entry.term.options.fontSize = state.settings.fontSize;
    entry.term.options.scrollback = state.settings.scrollback;
    entry.term.options.theme = theme();
  }
}
colorPreference.addEventListener('change', () => { applySettings(); requestAnimationFrame(fitActive); });
function terminal(id) {
  if (terminals.has(id)) return terminals.get(id);
  const host = document.createElement('div'); host.className = 'terminal-host'; host.hidden = id !== sessionID;
  $('terminals').append(host);
  const label = button('', false, () => focusPane(id)); label.className = 'pane-label';
  const surface = document.createElement('div'); surface.className = 'terminal-surface'; host.append(label, surface);
  const term = new Terminal({ cursorBlink: true, fontSize: state.settings.fontSize, scrollback: state.settings.scrollback, theme: theme() });
  const fit = new FitAddon(); term.loadAddon(fit); term.open(surface);
  surface.addEventListener('pointerdown', () => { if (sessionID !== id) focusPane(id); });
  surface.addEventListener('focusin', () => { if (sessionID !== id) focusPane(id); });
  term.onData(data => { if (state.running.includes(id)) api.write(id, data).catch(showError); });
  term.onResize(({ cols, rows }) => api.resize(id, cols, rows).catch(showError));
  const entry = { term, fit, host, label }; terminals.set(id, entry); return entry;
}
function fitActive() {
  for (const [id, entry] of terminals) if (!entry.host.hidden && entry.host.clientWidth > 0) {
    entry.fit.fit(); api.resize(id, entry.term.cols, entry.term.rows).catch(showError);
  }
}
function focusPane(id) { sessionID = id; projectID = state.sessions.find(s => s.id === id).projectID; render(); }
function openSession(id) {
  const session = state.sessions.find(s => s.id === id);
  if (!session) return;
  if (panes.length === 2 && !panes.includes(id)) panes[panes.indexOf(sessionID) < 0 ? 0 : panes.indexOf(sessionID)] = id;
  sessionID = id; projectID = session.projectID;
  if (session.archived) $('show-archived').checked = true;
  $('search').value = ''; render();
}
function render() {
  if (!state.projects.some(p => p.id === projectID)) { projectID = state.projects[0]?.id; sessionID = undefined; }
  const project = state.projects.find(p => p.id === projectID);
  const session = selected();
  const query = $('search').value.toLowerCase();
  const visible = s => ($('show-archived').checked || !s.archived) && (!query || `${s.title} ${s.notes || ''}`.toLowerCase().includes(query));
  $('projects').replaceChildren(...state.projects.filter(p => !query || p.title.toLowerCase().includes(query) || state.sessions.some(s => s.projectID === p.id && visible(s))).map(p => button(`${p.group ? p.group + " / " : ""}${p.icon || ""} ${p.title}`.trim(), p.id === projectID, () => {
    projectID = p.id; sessionID = state.sessions.find(s => s.projectID === p.id && visible(s))?.id; render();
  })));
  $('project-title').textContent = project?.title || 'Your agent workspace';
  $('project-path').textContent = session?.workingDirectory || project?.path || 'Open a folder to get started';
  $('new-session').disabled = !project;
  $('planning').disabled = !project;
  $('sessions').replaceChildren(...state.sessions.filter(s => s.projectID === projectID && visible(s)).sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned)).map(s =>
    button(`${s.pinned ? '★ ' : ''}${state.running.includes(s.id) ? '● ' : ''}${s.title}${agentStates.has(s.id) && state.running.includes(s.id) ? ' · ' + agentStates.get(s.id) : ''}${s.archived ? ' · archived' : ''}`, s.id === sessionID, () => openSession(s.id))));
  $('session-actions').hidden = !session; $('session-context').hidden = !session;
  $('empty').hidden = !!session; $('terminals').hidden = !session;
  if (session) {
    terminal(session.id);
    const profile = state.profiles.find(p => p.id === session.profileID);
    $('session-label').textContent = (session.agent === 'claude' ? 'Claude Code' : 'Codex') + ` · ${profile?.label || 'System account'}` + (session.archived ? ' · archived' : '');
    $('start').textContent = session.started ? (session.agent === 'codex' && !session.providerID ? 'Open resume picker' : 'Resume') : 'Start';
    $('start').disabled = state.running.includes(session.id) || !!session.archived || !!session.worktreeRemoved;
    $('stop').disabled = !state.running.includes(session.id);
    $('insert-prompt').hidden = !session.prompt;
    $('insert-prompt').disabled = !state.running.includes(session.id);
    const git = gitInfo.get(session.id);
    $('git-info').textContent = git ? `${git.branch} · ${git.changedFiles} changed` : (session.branch ? `Branch: ${session.branch}` : '');
    const links = [];
    if (session.reviewOf) links.push(button('← Builder', false, () => openSession(session.reviewOf)));
    for (const review of state.sessions.filter(s => s.reviewOf === session.id && !s.archived)) links.push(button(review.title, false, () => openSession(review.id)));
    $('review-link').replaceChildren(...links);
  }
  if (!session) panes = [];
  else if (!panes.includes(sessionID)) panes = [sessionID];
  $('terminals').classList.toggle('split', panes.length === 2);
  for (const [id, entry] of terminals) {
    entry.host.hidden = !panes.includes(id);
    entry.label.textContent = state.sessions.find(s => s.id === id)?.title || 'Session';
    entry.label.hidden = panes.length !== 2;
    entry.host.classList.toggle('focused-pane', sessionID === id);
    entry.host.style.order = String(panes.indexOf(id));
  }
  $('status').textContent = `${state.running.length} running · Sessions and recent output saved locally · Preview`;
  applySettings(); requestAnimationFrame(fitActive);
  if ($('planning-dialog').open) renderPlanning();
}
$('search').oninput = render;
$('show-archived').onchange = () => { if (selected()?.archived && !$('show-archived').checked) sessionID = undefined; render(); };
$('open-folder').onclick = () => perform(async () => {
  const next = await api.openFolder();
  projectID = next.projects.find(p => !state.projects.some(old => old.id === p.id))?.id || projectID;
  if (!state.sessions.some(s => s.id === sessionID && s.projectID === projectID)) sessionID = undefined;
  update(next);
});
function newSession() {
  reviewOf = undefined; $('session-form').reset();
  $('session-form').elements.agent.value = state.settings.defaultAgent;
  profileOptions($('session-profile'), state.settings.defaultAgent);
  $('session-form-title').textContent = 'New session'; $('session-dialog').showModal();
}
$('new-session').onclick = newSession;
$('cancel').onclick = () => $('session-dialog').close();
for (const node of document.querySelectorAll('[data-close]')) node.onclick = () => $(node.dataset.close).close();
$('session-form').onsubmit = event => {
  event.preventDefault();
  perform(async () => {
    const details = Object.fromEntries(new FormData(event.target));
    const next = await api.createSession({ ...details, projectID, ...(reviewOf ? { reviewOf } : {}) });
    sessionID = next.sessions.at(-1).id; $('session-dialog').close(); update(next);
  });
};
$('start').onclick = () => perform(async () => {
  const id = sessionID;
  terminal(id).term.writeln('\r\n\x1b[90mStarting agent…\x1b[0m');
  update(await api.start(id));
  if (sessionID === id) { fitActive(); terminal(id).term.focus(); }
});
$('stop').onclick = () => perform(() => api.stop(sessionID));
function showText(id, mode, value) {
  textTarget = id; textMode = mode;
  $('text-title').textContent = mode === 'feedback' ? 'Send feedback to builder' : 'Insert saved message';
  $('text-help').textContent = 'Make sure the agent is ready for a message. Text is inserted without pressing Enter; inspect it in the terminal before submitting.';
  $('text-value').value = value; $('text-dialog').showModal();
}
$('insert-prompt').onclick = () => showText(sessionID, 'prompt', selected().prompt);
$('text-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    const target = textMode === 'feedback' ? await api.feedback(textTarget, $('text-value').value) : (await api.paste(textTarget, $('text-value').value), textTarget);
    $('text-dialog').close(); openSession(target); terminal(target).term.focus();
  });
};
$('session-menu').onchange = event => {
  const action = event.target.value; event.target.value = '';
  const session = selected(); if (!session) return;
  perform(async () => {
    if (action === 'recover') { update(await api.recover(session.id)); openSession(state.sessions.at(-1).id); }
    else if (action === 'edit') {
      editID = session.id;
      for (const key of ['title', 'notes', 'providerID']) $('edit-form').elements[key].value = session[key] || '';
      $('edit-form').elements.providerID.disabled = state.running.includes(session.id); $('edit-dialog').showModal();
    } else if (action === 'pin') update(await api.editSession(session.id, { pinned: !session.pinned }));
    else if (action === 'archive') {
      const next = await api.editSession(session.id, { archived: !session.archived });
      if (!session.archived && !$('show-archived').checked) sessionID = undefined;
      update(next);
    } else if (action === 'review') {
      const brief = await api.reviewBrief(session.id);
      reviewOf = session.id; $('session-form').reset();
      $('session-form-title').textContent = 'Review handoff';
      $('session-form').elements.title.value = `Review: ${session.title}`.slice(0, 200);
      $('session-form').elements.agent.value = session.agent === 'claude' ? 'codex' : 'claude';
      profileOptions($('session-profile'), $('session-form').elements.agent.value);
      $('session-form').elements.prompt.value = brief; $('session-dialog').showModal();
    } else if (action === 'feedback') {
      if (!session.reviewOf) throw new Error('Choose a review session linked to a builder.');
      showText(session.id, 'feedback', 'Please address these review findings, verify the changes, and report what you fixed.\n\n' + (await api.history(session.id)).slice(-18000));
    } else if (action === 'usage') {
      $('usage-value').textContent = 'Loading usage…'; $('usage-dialog').showModal();
      const result = await api.usage(session.id);
      $('usage-value').textContent = result.windows.length ? result.windows.map(w => `${w.name}: ${Math.round(w.percent)}% used${w.resetsAt ? ' · resets ' + new Date(w.resetsAt * 1000).toLocaleString() : ''}`).join('\n') : 'Usage is unavailable. Claude reports limits after a supported status-line update.';
    } else if (action === 'history') {
      $('history-value').textContent = await api.history(session.id) || 'No saved output yet.'; $('history-dialog').showModal();
    } else if (action === 'worktree') {
      worktreeID = session.id; $('worktree-form').reset(); $('worktree-dialog').showModal();
    } else if (action === 'remove-worktree') update(await api.removeWorktree(session.id));
    else if (action === 'git') { gitInfo.set(session.id, await api.gitStatus(session.id)); render(); }
    else if (action === 'split') {
      $('split-list').replaceChildren(...state.sessions.filter(s => s.id !== session.id && !s.archived).map(s => button(`${s.title} · ${state.projects.find(p => p.id === s.projectID)?.title}`, false, () => {
        panes = [session.id, s.id]; terminal(s.id); $('split-dialog').close(); render();
      })));
      if (!$('split-list').childElementCount) $('split-list').textContent = 'Create another session to use split view.';
      $('split-dialog').showModal();
    } else if (action === 'unsplit') { panes = [sessionID]; render(); }
  });
};
$('edit-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    update(await api.editSession(editID, Object.fromEntries(new FormData(event.target)))); $('edit-dialog').close();
  });
};
$('worktree-form').onsubmit = event => {
  event.preventDefault(); const submit = event.submitter; if (submit) submit.disabled = true;
  perform(async () => { update(await api.createWorktree(worktreeID, event.target.elements.branch.value)); $('worktree-dialog').close(); })
    .finally(() => { if (submit) submit.disabled = false; });
};
$('settings').onclick = () => {
  for (const [key, value] of Object.entries(state.settings)) if ($('settings-form').elements[key]) {
    const input = $('settings-form').elements[key]; if (input.type === 'checkbox') input.checked = value; else input.value = value;
  }
  $('settings-dialog').showModal();
};
$('settings-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    const settings = Object.fromEntries(new FormData(event.target));
    settings.fontSize = Number(settings.fontSize); settings.scrollback = Number(settings.scrollback);
    settings.claudeUsage = event.target.elements.claudeUsage.checked;
    settings.hibernateMinutes = Number(settings.hibernateMinutes);
    settings.notifications = event.target.elements.notifications.checked;
    update(await api.saveSettings(settings)); $('settings-dialog').close();
  });
};
function commands() {
  $('command-list').replaceChildren(...state.quickCommands.filter(c => !c.projectID || c.projectID === projectID).map(command => {
    const row = document.createElement('div'); row.className = 'command-row';
    const title = document.createElement('strong'); title.textContent = command.title;
    const run = button(command.submit ? 'Insert & submit' : 'Insert', false, () => perform(async () => {
      await api.sendCommand(sessionID, command.id); $('commands-dialog').close(); terminal(sessionID).term.focus();
    }));
    run.disabled = !state.running.includes(sessionID);
    row.append(title, run, button('Edit', false, () => {
      commandID = command.id; $('command-form-title').textContent = 'Edit command';
      const form = $('command-form'); form.elements.title.value = command.title; form.elements.text.value = command.text;
      form.elements.scope.value = command.projectID ? 'project' : 'global'; form.elements.submit.checked = command.submit;
    }), button('Delete', false, () => perform(async () => { update(await api.deleteCommand(command.id)); commands(); })));
    return row;
  }));
}
function resetCommand() { commandID = undefined; $('command-form').reset(); $('command-form-title').textContent = 'Add command'; }
$('quick-commands').onclick = () => { resetCommand(); commands(); $('commands-dialog').showModal(); };
$('new-command').onclick = resetCommand;
$('command-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    const value = Object.fromEntries(new FormData(event.target));
    update(await api.saveCommand({ id: commandID, title: value.title, text: value.text, submit: value.submit === 'on', projectID: value.scope === 'project' ? projectID : undefined }));
    resetCommand(); commands();
  });
};

api.onData(({ id, data }) => terminal(id).term.write(data));
api.onExit(({ id, exitCode }) => terminal(id).term.writeln(`\r\n\x1b[90mSession stopped (exit ${exitCode}). Use Resume to continue.\x1b[0m`));
api.onChange(update); api.onError(showError);
new ResizeObserver(fitActive).observe($('terminals'));
perform(async () => update(await api.read()));

function profileOptions(select, agent) {
  select.replaceChildren(new Option('System account', ''), ...state.profiles.filter(p => p.agent === agent).map(p => new Option(p.label, p.id)));
}
$('session-form').elements.agent.onchange = event => profileOptions($('session-profile'), event.target.value);
function renderProfiles() {
  $('profile-list').replaceChildren(...state.profiles.map(profile => {
    const row = document.createElement('p'); row.textContent = `${profile.label} · ${profile.agent === 'claude' ? 'Claude Code' : 'Codex'}`; row.append(button('Remove profile', false, () => perform(async () => { update(await api.removeProfile(profile.id)); renderProfiles(); }))); return row;
  }));
  if (!state.profiles.length) $('profile-list').textContent = 'No managed profiles. System account uses your normal CLI configuration.';
}
$('accounts').onclick = () => { renderProfiles(); $('accounts-dialog').showModal(); };
$('profile-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    update(await api.addProfile(event.target.elements.label.value, event.target.elements.agent.value));
    event.target.reset(); renderProfiles();
  });
};
$('activity').onclick = () => {
  $('activity-list').replaceChildren(...state.activity.map(item => button(`${new Date(item.at).toLocaleString()} · ${item.title} · ${item.detail}`, false, () => {
    $('activity-dialog').close(); openSession(item.sessionID);
  })));
  if (!state.activity.length) $('activity-list').textContent = 'Session starts and exits will appear here.';
  $('activity-dialog').showModal();
};
function renderPlanning() {
  $('spec-list').replaceChildren(...state.specs.filter(s => s.projectID === projectID).map(spec => {
    const row = document.createElement('div'); row.className = 'planning-row';
    const label = document.createElement('strong'); label.textContent = `${spec.title} · r${spec.revision} · ${spec.approvedRevision === spec.revision ? 'Approved' : 'Draft'}`;
    const approve = button('Approve revision', false, () => perform(async () => { update(await api.approveSpec(spec.id, spec.revision)); renderPlanning(); }));
    approve.disabled = spec.approvedRevision === spec.revision;
    row.append(label, button('Edit', false, () => editSpec(spec)), approve, button('Export Markdown', false, () => perform(() => api.exportSpec(spec.id)))); return row;
  }));
  if (!$('spec-list').childElementCount) $('spec-list').textContent = 'No specifications yet. Tasks can also stand alone.';
  $('task-list').replaceChildren(...state.tasks.filter(t => t.projectID === projectID).map(task => {
    const row = document.createElement('div'); row.className = 'planning-row';
    const label = document.createElement('strong'); label.textContent = `${task.title} · ${task.status}`;
    const status = document.createElement('select'); status.setAttribute('aria-label', `Status for ${task.title}`);
    for (const value of ['queued', 'building', 'review', 'changes', 'done', 'failed']) {
      const option = new Option(value, value); option.disabled = ['building', 'failed'].includes(value); status.append(option);
    }
    status.value = task.status; status.disabled = task.status === 'building';
    status.onchange = () => perform(async () => { update(await api.taskStatus(task.id, status.value)); renderPlanning(); });
    const edit = button('Edit', false, () => editTask(task)); edit.disabled = task.status === 'building';
    row.append(label, status, edit);
    if (task.sessionID) row.append(button('Open session', false, () => { $('planning-dialog').close(); openSession(task.sessionID); }));
    const spec = state.specs.find(s => s.id === task.specID);
    if (!task.sessionID || task.specRevision !== spec?.revision || state.sessions.find(s => s.id === task.sessionID)?.archived) {
      const prepare = button('Prepare session', false, () => { prepareID = task.id; profileOptions($('task-profile'), task.agent); $('planning-dialog').close(); $('prepare-dialog').showModal(); });
      prepare.disabled = task.status === 'building'; row.append(prepare);
    }
    if (task.lastError) { const error = document.createElement('small'); error.textContent = task.lastError; row.append(error); }
    return row;
  }));
  if (!$('task-list').childElementCount) $('task-list').textContent = 'No tasks yet.';
}
$('planning').onclick = () => { renderPlanning(); $('planning-dialog').showModal(); };
function editSpec(spec) {
  specID = spec?.id; $('spec-form').reset();
  $('spec-heading').textContent = spec ? `Specification · revision ${spec.revision}` : 'New specification';
  for (const key of ['title', 'problem', 'requirements', 'acceptance', 'constraints', 'plan']) $('spec-form').elements[key].value = spec?.[key] || '';
  $('planning-dialog').close(); $('spec-dialog').showModal();
}
$('new-spec').onclick = () => editSpec();
$('spec-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    update(await api.saveSpec({ ...Object.fromEntries(new FormData(event.target)), id: specID, projectID }));
    $('spec-dialog').close(); renderPlanning(); $('planning-dialog').showModal();
  });
};
function editTask(task) {
  taskID = task?.id; const form = $('task-form'); form.reset();
  form.elements.specID.replaceChildren(new Option('No specification', ''), ...state.specs.filter(s => s.projectID === projectID).map(s => new Option(s.title, s.id)));
  form.elements.specID.value = task?.specID || ''; form.elements.specID.disabled = !!task;
  for (const key of ['title', 'details', 'findings']) form.elements[key].value = task?.[key] || '';
  form.elements.mode.value = task?.mode || 'none'; form.elements.autoReview.checked = task?.autoReview || false;
  form.elements.agent.value = task?.agent || state.settings.defaultAgent;
  $('planning-dialog').close(); $('task-dialog').showModal();
}
$('new-task').onclick = () => editTask();
$('task-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    update(await api.saveTask({ ...Object.fromEntries(new FormData(event.target)), id: taskID, projectID, autoReview: event.target.elements.autoReview.checked }));
    $('task-dialog').close(); renderPlanning(); $('planning-dialog').showModal();
  });
};
$('prepare-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    const next = await api.prepareTask(prepareID, event.target.elements.profileID.value);
    update(next); $('prepare-dialog').close(); openSession(next.tasks.find(t => t.id === prepareID).sessionID);
  });
};
api.onFocus?.(openSession);

installFiles({ api, $, button, perform, context: () => sessionID || projectID });
$('discover-projects').onclick = () => perform(async () => update(await api.discover()));
$('project-settings').onclick = () => {
  const p = state.projects.find(p => p.id === projectID); if (!p) return;
  for (const key of ['title', 'group', 'icon', 'setupCommand', 'sharedPaths', 'reviewTemplate']) $('project-form').elements[key].value = p[key] || '';
  $('project-dialog').showModal();
};
$('project-form').onsubmit = event => { event.preventDefault(); perform(async () => { update(await api.editProject(projectID, Object.fromEntries(new FormData(event.target)))); $('project-dialog').close(); }); };
$('project-reconnect').onclick = () => perform(async () => update(await api.reconnectProject(projectID)));
$('project-remove').onclick = () => perform(async () => { update(await api.removeProject(projectID)); $('project-dialog').close(); });

api.onStatus(value => { agentStates.set(value.id, value.state); render(); if (value.id === sessionID) $('status').textContent = `Agent: ${value.state}`; });

$('run-queue').onclick = () => perform(async () => { update(await api.toggleQueue(projectID)); renderPlanning(); $('run-queue').textContent = state.queues?.includes(projectID) ? 'Pause queue' : 'Run queue'; });
installNavigation({ $, api, perform, update, state: () => state, openSession, project: () => ({ projectID, sessionID }), selectProject: id => { projectID = id; sessionID = state.sessions.find(s => s.projectID === id && !s.archived)?.id; render(); } });

$('provider-history').onclick = () => { if (!projectID) return; profileOptions($('provider-history-form').elements.profileID, $('provider-history-form').elements.agent.value); $('provider-history-dialog').showModal(); };
$('provider-history-form').elements.agent.onchange = event => profileOptions($('provider-history-form').elements.profileID, event.target.value);
$('provider-history-form').onsubmit = event => { event.preventDefault(); perform(async () => {
  const entries = await api.scanTranscripts(projectID, event.target.elements.agent.value, event.target.elements.profileID.value);
  $('transcript-list').replaceChildren(...entries.map(item => button(item.title, false, () => perform(async () => { update(await api.importTranscript(item.providerID)); $('provider-history-dialog').close(); openSession(state.sessions.find(s => s.providerID === item.providerID).id); }))));
  if (!entries.length) $('transcript-list').textContent = 'No saved conversations found for this folder and account.';
}); };
