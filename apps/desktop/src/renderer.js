import { installUsage } from './usage-ui.js';
import { installNavigation } from './navigation.js';
import { installFiles } from './files-ui.js';
import { icon, agentIcon, projectTint, hydrateIcons } from './icons.js';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

const api = window.convoy;
const $ = id => document.getElementById(id);
const defaults = { theme: 'dark', fontSize: 14, scrollback: 10000, defaultAgent: 'claude', notifications: false, keepAwake: 'off', hibernateMinutes: 0 };
let state = { projects: [], sessions: [], running: [], settings: defaults, quickCommands: [] };
let projectID, sessionID, reviewOf, editID, textTarget, textMode, worktreeID, commandID;
let panes = [], specID, taskID, prepareID, homeMode = false;
const terminals = new Map();
const removedSessions = new Set();
const gitInfo = new Map();
const agentStates = new Map();
const expanded = new Set();
const colorPreference = matchMedia('(prefers-color-scheme: light)');
hydrateIcons();
document.documentElement.dataset.platform = api.platform || '';
const agentName = agent => agent === 'claude' ? 'Claude Code' : 'Codex';
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
function update(next) { if (next) {
  const saved = new Set(next.sessions.map(s => s.id));
  for (const [id, entry] of terminals) if (!saved.has(id)) { removedSessions.add(id); entry.term.dispose(); entry.host.remove(); terminals.delete(id); gitInfo.delete(id); agentStates.delete(id); }
  // A resumed PTY starts with fresh dimensions, even if its hidden pane never moved.
  for (const [id, entry] of terminals) if (!next.running.includes(id) || !state.running.includes(id)) entry.size = undefined;
  panes = panes.filter(id => saved.has(id));
  state = { ...next, settings: { ...defaults, ...next.settings }, quickCommands: next.quickCommands || [], specs: next.specs || [], tasks: next.tasks || [], profiles: next.profiles || [], activity: next.activity || [] }; render(); } }
function button(label, selected, click) {
  const node = document.createElement('button');
  node.type = 'button'; node.textContent = label;
  node.classList.toggle('selected', selected);
  node.setAttribute('aria-pressed', String(selected)); node.onclick = click;
  return node;
}
function text(tag, value, className) { const node = document.createElement(tag); node.textContent = value; if (className) node.className = className; return node; }
function selected() { return state.sessions.find(s => s.id === sessionID); }
function sessionState(session) {
  if (!session) return { key: 'idle', label: 'Not running' };
  if (startingSessions.has(session.id)) return { key: 'working', label: 'Starting' };
  if (!state.running.includes(session.id)) return { key: 'idle', label: session.started ? 'Not running' : 'Saved' };
  const raw = (agentStates.get(session.id) || 'running').toLowerCase();
  if (/wait|attention|permission|input/.test(raw)) return { key: 'waiting', label: 'Waiting for you' };
  if (/done|finish|complete/.test(raw)) return { key: 'done', label: 'Finished — needs review' };
  if (/work|busy|thinking|running/.test(raw) && raw !== 'running') return { key: 'working', label: 'Working' };
  return { key: 'running', label: raw === 'running' ? 'Running' : raw };
}
function stateDot(session) { const dot = document.createElement('span'); dot.className = 'state-dot'; dot.dataset.state = sessionState(session).key; return dot; }
function tintFor(project) { return project?.color || projectTint(project?.title || ''); }
function projectIcon(project) {
  if (project?.icon) return text('span', project.icon, 'project-icon');
  const node = icon('folder', 14, 'project-icon'); return node;
}
function theme() {
  return state.settings.theme === 'light' || (state.settings.theme === 'system' && colorPreference.matches)
    ? { background: '#ffffff', foreground: '#1d1d1f', cursor: '#5e6bd1', selectionBackground: 'rgba(94, 107, 209, .25)' }
    : { background: '#08090a', foreground: '#d9dbe0', cursor: '#8c99eb', selectionBackground: 'rgba(140, 153, 235, .3)' };
}
function applySettings() {
  document.documentElement.dataset.theme = theme().background === '#ffffff' ? 'light' : 'dark';
  const key = JSON.stringify([state.settings.fontSize, state.settings.scrollback, theme()]);
  for (const entry of terminals.values()) {
    if (entry.settingsKey === key) continue;
    entry.settingsKey = key;
    entry.term.options.fontSize = state.settings.fontSize;
    entry.term.options.scrollback = state.settings.scrollback;
    entry.term.options.theme = theme();
  }
}
colorPreference.addEventListener('change', () => { applySettings(); scheduleFit(); });
function terminal(id) {
  if (terminals.has(id)) return terminals.get(id);
  const host = document.createElement('div'); host.className = 'terminal-host'; host.hidden = id !== sessionID;
  $('terminals').append(host);
  const label = button('', false, () => focusPane(id)); label.className = 'pane-label';
  const surface = document.createElement('div'); surface.className = 'terminal-surface'; host.append(label, surface);
  const ready = document.createElement('section'); ready.className = 'session-ready';
  const heading = document.createElement('h2'); heading.textContent = 'Ready to start';
  const description = document.createElement('p'); description.textContent = 'This session is saved. Start the agent to open its terminal in this workspace.';
  const launch = button('Start agent', false, () => { focusPane(id); $('start').click(); }); launch.className = 'primary';
  ready.append(heading, description, launch); host.append(ready);
  const term = new Terminal({ cursorBlink: true, fontSize: state.settings.fontSize, scrollback: state.settings.scrollback, theme: theme(), fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, "Cascadia Mono", monospace' });
  const fit = new FitAddon(); term.loadAddon(fit); term.open(surface);
  surface.addEventListener('pointerdown', () => { if (sessionID !== id) focusPane(id); });
  surface.addEventListener('focusin', () => { if (sessionID !== id) focusPane(id); });
  term.onData(data => { if (state.running.includes(id)) api.write(id, data).catch(showError); });
  const entry = { term, fit, host, label, ready, launch }; terminals.set(id, entry); return entry;
}
let fitFrame;
function scheduleFit() {
  if (fitFrame !== undefined) return;
  fitFrame = requestAnimationFrame(() => { fitFrame = undefined; fitActive(); });
}
function fitActive() {
  for (const [id, entry] of terminals) if (!entry.host.hidden && entry.host.clientWidth > 0) {
    entry.fit.fit();
    const size = `${state.running.includes(id)}:${entry.term.cols}:${entry.term.rows}`;
    if (entry.size !== size) {
      entry.size = size;
      if (state.running.includes(id)) api.resize(id, entry.term.cols, entry.term.rows).catch(showError);
    }
  }
}
function focusPane(id) { sessionID = id; projectID = state.sessions.find(s => s.id === id).projectID; homeMode = false; render(); }
function openSession(id) {
  const session = state.sessions.find(s => s.id === id);
  if (!session) return;
  if (panes.length === 2 && !panes.includes(id)) panes[panes.indexOf(sessionID) < 0 ? 0 : panes.indexOf(sessionID)] = id;
  sessionID = id; projectID = session.projectID; homeMode = false;
  if (session.archived) $('show-archived').checked = true;
  $('search').value = ''; render();
}
function selectProject(id) { projectID = id; homeMode = false; sessionID = state.sessions.find(s => s.projectID === id && !s.archived)?.id; expanded.add(id); render(); }
function paneLabel(entry, session) {
  const project = state.projects.find(p => p.id === session?.projectID);
  const bar = document.createElement('span'); bar.className = 'tint-bar';
  entry.label.style.setProperty('--tint', tintFor(project));
  entry.label.replaceChildren(bar, projectIcon(project), text('span', project?.title || 'Project', 'pane-project'), text('span', '/', 'crumb-sep'), stateDot(session), agentIcon(session?.agent || 'claude', 12), text('span', session?.title || 'Session'));
}
function sessionTab(session, index) {
  const project = state.projects.find(p => p.id === session.projectID);
  const tab = button('', session.id === sessionID, () => openSession(session.id)); tab.classList.add('tab');
  tab.style.setProperty('--tint', tintFor(project));
  tab.title = `${project?.title || 'Project'} · ${agentName(session.agent)}${state.running.includes(session.id) ? '' : ' · not running'}`;
  const bar = document.createElement('span'); bar.className = 'tint-bar';
  const body = document.createElement('span'); body.className = 'tab-text';
  const title = text('span', `${session.pinned ? '★ ' : ''}${session.title}${session.archived ? ' · archived' : ''}`, 'tab-title');
  const subtitle = text('span', project?.title || agentName(session.agent), 'tab-project');
  body.append(title, subtitle);
  tab.append(bar, stateDot(session), agentIcon(session.agent, 12), body);
  const pane = panes.length === 2 ? panes.indexOf(session.id) : -1;
  if (pane >= 0) { const badge = text('span', String(pane + 1), 'pane-badge'); tab.append(badge); }
  else if (index < 9) tab.append(text('span', `${navigator.platform.startsWith('Mac') ? '⌘' : 'Ctrl+'}${index + 1}`, 'tab-key'));
  return tab;
}
function sidebarSession(item) {
  const child = button('', item.id === sessionID, () => openSession(item.id)); child.classList.add('sidebar-session');
  child.dataset.running = String(state.running.includes(item.id));
  child.title = `${agentName(item.agent)} · ${sessionState(item).label}${item.archived ? ' · archived' : ''}`;
  child.append(stateDot(item));
  if (item.pinned) child.append(icon('pin', 8, 'pin'));
  child.append(text('span', item.title, 'name'));
  if (item.branch) { const chip = text('span', item.branch, 'branch-chip'); chip.prepend(icon('branch', 9)); chip.style.height = '16px'; chip.style.fontSize = '9px'; child.append(chip); }
  else { child.append(agentIcon(item.agent, 11)); if (item.reviewOf) child.append(text('span', 'review', 'review-tag')); }
  const detail = document.createElement('small');
  detail.textContent = [agentName(item.agent), item.pinned ? 'Pinned' : '', item.archived ? 'Archived' : '', state.running.includes(item.id) ? agentStates.get(item.id) || 'Running' : ''].filter(Boolean).join(' · ');
  child.append(detail);
  return child;
}
function projectRow(p, visible, query) {
  const sessions = state.sessions.filter(s => s.projectID === p.id && visible(s)).sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned));
  const row = button('', p.id === projectID && !homeMode, () => selectProject(p.id));
  row.classList.add('project-row'); row.title = p.path; row.style.setProperty('--tint', tintFor(p));
  const open = !!query || expanded.has(p.id);
  const chevron = document.createElement('span'); chevron.className = 'chevron';
  if (sessions.length) {
    chevron.append(icon(open ? 'chevronDown' : 'chevronRight', 9));
    chevron.onclick = event => { event.stopPropagation(); if (expanded.has(p.id)) expanded.delete(p.id); else expanded.add(p.id); render(); };
  }
  row.append(chevron, projectIcon(p), text('span', p.title, 'name'));
  const active = sessions.filter(s => state.running.includes(s.id));
  if (active.length) { const dot = document.createElement('span'); dot.className = 'running-dot'; const attention = active.some(s => sessionState(s).key === 'waiting'); dot.dataset.attention = String(attention); dot.title = attention ? 'A session needs you' : 'Running session'; row.append(dot); }
  const count = text('span', String(state.sessions.filter(s => s.projectID === p.id && !s.archived).length), 'project-count'); row.append(count);
  return { row, children: open ? sessions.map(sidebarSession) : [] };
}
function render() {
  if (!state.projects.some(p => p.id === projectID)) { projectID = state.projects[0]?.id; sessionID = undefined; }
  if (projectID && !expanded.size) expanded.add(projectID);
  const project = homeMode ? undefined : state.projects.find(p => p.id === projectID);
  const session = homeMode ? undefined : selected();
  const query = $('search').value.toLowerCase();
  const visible = s => ($('show-archived').checked || !s.archived) && (!query || `${s.title} ${s.notes || ''}`.toLowerCase().includes(query));
  const projects = state.projects.filter(p => !query || p.title.toLowerCase().includes(query) || state.sessions.some(s => s.projectID === p.id && visible(s)));
  $('projects-empty').hidden = projects.length > 0;
  $('projects-empty').textContent = state.projects.length ? 'No matching projects' : 'No projects yet.';
  const groups = new Map();
  for (const p of projects) { const key = p.group || ''; if (!groups.has(key)) groups.set(key, []); groups.get(key).push(p); }
  $('projects').replaceChildren(...[...groups.entries()].map(([name, members]) => {
    const group = document.createElement('div'); group.className = 'project-group';
    if (name) { group.classList.add('grouped'); const label = document.createElement('div'); label.className = 'project-group-label'; label.append(icon('folderFill', 11), text('span', name)); group.append(label); }
    for (const p of members) { const { row, children } = projectRow(p, visible, query); group.append(row, ...children); }
    return group;
  }));
  $('show-archived').closest('label').hidden = !state.projects.length;
  $('project-title').textContent = project?.title || 'Your agent workspace';
  $('project-path').textContent = session?.workingDirectory || project?.path || 'Open a folder to get started';
  $('new-session').disabled = !project; $('empty-action').disabled = !project;
  for (const id of ['planning', 'provider-history', 'project-settings']) $(id).disabled = !project;
  $('files-open').disabled = !project && !session;
  const hasSessions = state.sessions.some(s => s.projectID === projectID && !s.archived);
  $('workbench-header').hidden = !project || !!session;
  $('empty-title').textContent = !project ? 'A home for your coding agents' : hasSessions ? 'Pick a session in the sidebar' : 'Start your first session';
  $('empty-description').textContent = !project ? 'Open a project folder to keep your Claude Code and Codex sessions together.' : `Run Claude Code or Codex inside Convoy. Each session keeps its own terminal, notes and saved output for ${project.title}.`;
  $('empty-cta').textContent = project ? 'New session' : 'Open folder';
  $('empty-footer').textContent = project ? `${project.title} · Project folder` : 'Uses your installed CLI tools and existing logins. Agents show their permission prompts in the terminal.';
  const tabs = state.sessions.filter(s => s.projectID === projectID && visible(s)).sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned));
  $('sessions').replaceChildren(...tabs.map(sessionTab));
  $('sessions-empty').hidden = tabs.length > 0;
  $('session-actions').hidden = !session; $('session-context').hidden = !session; $('saved-banner').hidden = !session;
  const failed = session?.lastExit && !session.lastExit.stopped && session.lastExit.code !== 0 && !state.running.includes(session.id);
  $('session-recovery').hidden = !failed;
  if (failed) {
    $('session-exit-message').textContent = session.lastExit.resumeMissing
      ? 'Claude could not find this conversation. Check the original account and folder, or create a fresh recovery session.'
      : `The agent exited with code ${session.lastExit.code}. Check the terminal error before retrying. If no conversation was saved, create a recovery session.`;
    $('recover-session').disabled = !!session.worktreeRemoved;
  }
  $('empty').hidden = !!session; $('terminals').hidden = !session;
  if (session) {
    terminal(session.id);
    const profile = state.profiles.find(p => p.id === session.profileID);
    $('crumb-project').textContent = project?.title || 'Project';
    $('crumb-session').textContent = session.title; $('crumb-session').title = session.title;
    $('session-label').textContent = agentName(session.agent) + ` · ${profile?.label || 'System account'}` + (session.archived ? ' · archived' : '');
    const current = sessionState(session);
    $('session-state').querySelector('.state-dot').dataset.state = current.key;
    $('session-state').querySelector('.state-text').textContent = current.label;
    $('start').textContent = startingSessions.has(session.id) ? 'Starting…' : state.running.includes(session.id) ? 'Running' : session.started ? (session.agent === 'codex' && !session.providerID ? 'Open resume picker' : 'Resume') : 'Start';
    $('start').disabled = startingSessions.has(session.id) || state.running.includes(session.id) || !!session.archived || !!session.worktreeRemoved;
    $('start').hidden = state.running.includes(session.id) && !startingSessions.has(session.id);
    $('saved-banner').hidden = state.running.includes(session.id) || startingSessions.has(session.id) || !session.started || failed;
    $('stop').disabled = !state.running.includes(session.id);
    $('insert-prompt').hidden = !session.prompt;
    $('insert-prompt').disabled = !state.running.includes(session.id);
    const git = gitInfo.get(session.id);
    const gitText = git ? `${git.branch} · ${git.changedFiles} changed` : (session.branch || '');
    $('git-info').replaceChildren(...(gitText ? [icon('branch', 10), text('span', gitText)] : []));
    $('git-info').classList.toggle('active', !!session.branch);
    const links = [];
    if (session.reviewOf) links.push(button('← Builder', false, () => openSession(session.reviewOf)));
    for (const review of state.sessions.filter(s => s.reviewOf === session.id && !s.archived)) links.push(button(review.title, false, () => openSession(review.id)));
    $('review-link').replaceChildren(...links);
    $('session-context').hidden = !links.length && !session.prompt;
  }
  if (!session) panes = [];
  else if (!panes.includes(sessionID)) panes = [sessionID];
  $('terminals').classList.toggle('split', panes.length === 2);
  $('layout-one').classList.toggle('active', panes.length < 2); $('layout-two').classList.toggle('active', panes.length === 2);
  $('layout-two').disabled = !session;
  for (const [id, entry] of terminals) {
    entry.host.hidden = !panes.includes(id);
    const savedSession = state.sessions.find(s => s.id === id);
    paneLabel(entry, savedSession);
    entry.ready.hidden = !savedSession || savedSession.started || state.running.includes(id) || startingSessions.has(id);
    entry.launch.textContent = savedSession?.agent === 'claude' ? 'Start Claude Code' : 'Start Codex';
    entry.launch.disabled = !!savedSession?.archived || !!savedSession?.worktreeRemoved;
    entry.label.hidden = panes.length !== 2;
    entry.host.classList.toggle('focused-pane', sessionID === id);
    entry.host.style.order = String(panes.indexOf(id));
  }
  $('status').replaceChildren(icon('terminal', 11), text('span', `${state.running.length} running`));
  $('home').classList.toggle('active', homeMode || !project);
  applySettings(); scheduleFit();
  if ($('planning-dialog').open) renderPlanning();
}
$('search').oninput = render;
$('show-archived').onchange = () => { if (selected()?.archived && !$('show-archived').checked) sessionID = undefined; render(); };
$('open-folder').onclick = () => perform(async () => {
  const next = await api.openFolder();
  projectID = next.projects.find(p => !state.projects.some(old => old.id === p.id))?.id || projectID;
  if (!state.sessions.some(s => s.id === sessionID && s.projectID === projectID)) sessionID = undefined;
  homeMode = false; expanded.add(projectID); update(next);
});
$('open-folder-footer').onclick = () => $('open-folder').click();
function agentInputs(form) { return form.querySelector('[name="agent"]:checked')?.value || form.elements.agent.value; }
function syncSessionSheet() {
  const form = $('session-form'), agent = agentInputs(form);
  $('session-submit').textContent = reviewOf ? 'Launch reviewer' : agent === 'claude' ? 'Start Claude Code' : 'Start Codex';
}
function newSession() {
  reviewOf = undefined; $('session-form').reset();
  $('session-form').elements.agent.value = state.settings.defaultAgent;
  profileOptions($('session-profile'), state.settings.defaultAgent);
  const project = state.projects.find(p => p.id === projectID);
  $('session-project-chip').replaceChildren(projectIcon(project), text('span', project?.title || 'Project'));
  $('session-project-chip').style.setProperty('--tint', tintFor(project));
  $('session-project-path').textContent = project?.path || '';
  $('prompt-label').textContent = 'First message (optional)';
  $('prompt-help').textContent = 'Uses your installed agent, login and approval settings.';
  $('session-form-title').textContent = 'New session'; syncSessionSheet(); $('session-dialog').showModal();
  $('session-form').elements.title.focus();
}
$('new-session').onclick = newSession;
$('empty-action').onclick = newSession;
$('empty-cta').onclick = () => (projectID && !homeMode ? $('new-session') : $('open-folder')).click();
$('cancel').onclick = () => $('session-dialog').close();
for (const node of document.querySelectorAll('[data-close]')) node.onclick = () => $(node.dataset.close).close();
let creatingSession = false;
const startingSessions = new Set();
$('session-form').onsubmit = event => {
  event.preventDefault();
  if (creatingSession) return;
  creatingSession = true;
  const submit = event.target.querySelector('[type="submit"]'); submit.disabled = true;
  perform(async () => {
    try {
      const details = Object.fromEntries(new FormData(event.target));
      if (!details.title.trim()) details.title = autoTitle(details.agent, details.prompt);
      const next = await api.createSession({ ...details, projectID, ...(reviewOf ? { reviewOf } : {}) });
      const id = next.sessions.at(-1).id;
      sessionID = id; $('session-dialog').close(); update(next);
      await startSession(id);
    } finally { creatingSession = false; submit.disabled = false; }
  });
};
function autoTitle(agent, prompt = '') {
  const line = prompt.split('\n').map(l => l.replace(/^[#*>\-\s]+/, '').trim()).find(Boolean);
  if (line) return line.length > 60 ? line.slice(0, 59) + '…' : line;
  const now = new Date();
  return `${agentName(agent)} · ${now.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}, ${now.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: false })}`;
}
async function startSession(id) {
  if (!id || startingSessions.has(id) || state.running.includes(id)) return;
  startingSessions.add(id);
  const entry = terminal(id);
  entry.ready.hidden = true; entry.launch.disabled = true;
  if (sessionID === id) { $('start').disabled = true; $('start').textContent = 'Starting…'; }
  entry.term.writeln('\r\n\x1b[90mStarting agent…\x1b[0m');
  try {
    update(await api.start(id));
    if (sessionID === id) { fitActive(); entry.term.focus(); }
  } finally { startingSessions.delete(id); render(); }
}
$('start').onclick = () => perform(() => startSession(sessionID));
$('stop').onclick = () => perform(() => api.stop(sessionID));
$('recover-session').onclick = () => perform(async () => {
  const next = await api.recover(sessionID); update(next); openSession(next.sessions.at(-1).id);
});
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
function openSplit(session) {
  $('split-list').replaceChildren(...state.sessions.filter(s => s.id !== session.id && !s.archived).map(s => {
    const row = button('', false, () => { panes = [session.id, s.id]; terminal(s.id); $('split-dialog').close(); render(); });
    row.append(agentIcon(s.agent, 14), text('span', s.title), text('span', state.projects.find(p => p.id === s.projectID)?.title || '', 'kind'));
    return row;
  }));
  if (!$('split-list').childElementCount) $('split-list').textContent = 'Create another session to use split view.';
  $('split-dialog').showModal();
}
$('layout-two').onclick = () => { const session = selected(); if (session && panes.length < 2) openSplit(session); };
$('layout-one').onclick = () => { if (panes.length === 2) { panes = [sessionID]; render(); } };
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
      $('session-form-title').textContent = 'Start a review';
      $('session-form').elements.title.value = `Review: ${session.title}`.slice(0, 200);
      $('session-form').elements.agent.value = session.agent === 'claude' ? 'codex' : 'claude';
      profileOptions($('session-profile'), $('session-form').elements.agent.value);
      const project = state.projects.find(p => p.id === session.projectID);
      $('session-project-chip').replaceChildren(projectIcon(project), text('span', project?.title || 'Project'));
      $('session-project-chip').style.setProperty('--tint', tintFor(project));
      $('session-project-path').textContent = project?.path || '';
      $('prompt-label').textContent = 'Review brief — edit before sending';
      $('prompt-help').textContent = 'The other agent receives this brief as its first message.';
      $('session-form').elements.prompt.value = brief; syncSessionSheet(); $('session-dialog').showModal();
    } else if (action === 'feedback') {
      if (!session.reviewOf) throw new Error('Choose a review session linked to a builder.');
      showText(session.id, 'feedback', 'Please address these review findings, verify the changes, and report what you fixed.\n\n' + (await api.history(session.id)).slice(-18000));
    } else if (action === 'usage') {
      openUsage(session.id);
    } else if (action === 'history') {
      $('history-value').textContent = await api.history(session.id) || 'No saved output yet.'; $('history-dialog').showModal();
    } else if (action === 'worktree') {
      worktreeID = session.id; $('worktree-form').reset(); $('worktree-dialog').showModal();
    } else if (action === 'remove-worktree') update(await api.removeWorktree(session.id));
    else if (action === 'git') { gitInfo.set(session.id, await api.gitStatus(session.id)); render(); }
    else if (action === 'split') openSplit(session);
    else if (action === 'unsplit') { panes = [sessionID]; render(); }
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
    const title = document.createElement('strong'); title.append(icon(command.submit ? 'check' : 'bolt', 11), text('span', command.title));
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

api.onData(({ id, data }) => { if (!removedSessions.has(id)) terminal(id).term.write(data); });
api.onExit(({ id, exitCode, stopped, resumeMissing }) => {
  if (removedSessions.has(id)) return;
  const hint = resumeMissing ? 'Conversation not found. Use Create recovery session for a new conversation.'
    : stopped || exitCode === 0 ? 'Use Resume to continue.' : 'Check the error above before retrying; a recovery session starts a new conversation.';
  terminal(id).term.writeln(`\r\n\x1b[90mProcess exited (code ${exitCode}). Output is kept below. ${hint}\x1b[0m`);
});
api.onChange(update); api.onError(showError);
new ResizeObserver(scheduleFit).observe($('terminals'));
perform(async () => update(await api.read()));

function profileOptions(select, agent) {
  select.replaceChildren(new Option('System account', ''), ...state.profiles.filter(p => p.agent === agent).map(p => new Option(p.label, p.id)));
}
$('session-form').addEventListener('change', event => { if (event.target.name === 'agent') { profileOptions($('session-profile'), event.target.value); syncSessionSheet(); } });
function renderProfiles() {
  $('profile-list').replaceChildren(...state.profiles.map(profile => {
    const row = document.createElement('p'); row.append(agentIcon(profile.agent, 14), text('span', `${profile.label} · ${agentName(profile.agent)}`)); row.append(button('Remove', false, () => perform(async () => { update(await api.removeProfile(profile.id)); renderProfiles(); }))); return row;
  }));
  if (!state.profiles.length) { const empty = text('p', 'No managed accounts. System account uses your normal CLI configuration.'); $('profile-list').replaceChildren(empty); }
}
$('accounts').onclick = () => { renderProfiles(); $('accounts-dialog').showModal(); };
$('profile-form').onsubmit = event => {
  event.preventDefault(); perform(async () => {
    update(await api.addProfile(event.target.elements.label.value, event.target.elements.agent.value));
    event.target.reset(); renderProfiles();
  });
};
$('activity').onclick = () => {
  $('activity-list').replaceChildren(...state.activity.map(item => {
    const row = button('', false, () => { $('activity-dialog').close(); openSession(item.sessionID); });
    row.append(icon('bell', 12), text('span', `${item.title} · ${item.detail}`), text('span', new Date(item.at).toLocaleString(), 'kind'));
    return row;
  }));
  if (!state.activity.length) $('activity-list').replaceChildren(text('p', 'Session starts and exits will appear here.'));
  $('activity-dialog').showModal();
};
const taskStatus = { queued: ['clock', 'Queued'], building: ['play', 'Running'], review: ['checkCircle', 'Needs review'], changes: ['warning', 'Changes requested'], done: ['checkCircle', 'Done'], failed: ['x', 'Failed'] };
function renderPlanning() {
  $('run-queue').textContent = state.queues?.includes(projectID) ? 'Pause queue' : 'Run queue';
  $('spec-list').replaceChildren(...state.specs.filter(s => s.projectID === projectID).map(spec => {
    const row = document.createElement('div'); row.className = 'planning-row';
    const approved = spec.approvedRevision === spec.revision;
    const label = document.createElement('strong'); label.append(icon('doc', 12), text('span', spec.title), text('span', `r${spec.revision} · ${approved ? 'Approved' : 'Draft'}`, approved ? 'muted' : 'muted'));
    const approve = button('Approve revision', false, () => perform(async () => { update(await api.approveSpec(spec.id, spec.revision)); renderPlanning(); }));
    approve.disabled = approved;
    row.append(label, button('Edit', false, () => editSpec(spec)), approve, button('Export Markdown', false, () => perform(() => api.exportSpec(spec.id)))); return row;
  }));
  if (!$('spec-list').childElementCount) $('spec-list').replaceChildren(text('p', 'No specifications yet. Tasks can also stand alone.'));
  $('task-list').replaceChildren(...state.tasks.filter(t => t.projectID === projectID).map(task => {
    const row = document.createElement('div'); row.className = 'planning-row';
    const [glyph, statusLabel] = taskStatus[task.status] || ['clock', task.status];
    const label = document.createElement('strong');
    const symbol = icon(glyph, 13, `status-icon status-tint-${task.status}`);
    label.append(symbol, agentIcon(task.agent || 'claude', 12), text('span', task.title), text('span', statusLabel, 'muted'));
    const status = document.createElement('select'); status.setAttribute('aria-label', `Status for ${task.title}`);
    for (const value of ['queued', 'building', 'review', 'changes', 'done', 'failed']) {
      const option = new Option(taskStatus[value][1], value); option.disabled = ['building', 'failed'].includes(value); status.append(option);
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
  if (!$('task-list').childElementCount) $('task-list').replaceChildren(text('p', 'No tasks yet. Describe the work; the agent takes it, implements, verifies, then opens a PR or pushes.'));
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
  $('task-heading').textContent = task ? 'Edit task' : 'New task';
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
$('git-info').onclick = () => $('files-open').click();
$('crumb-project').onclick = () => { sessionID = undefined; render(); };
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
function goHome() { homeMode = true; sessionID = undefined; render(); }
function toggleSidebar() {
  const hidden = document.querySelector('.workspace').classList.toggle('sidebar-hidden');
  $('sidebar-toggle').title = hidden ? 'Show sidebar' : 'Hide sidebar'; $('sidebar-toggle').classList.toggle('active', !hidden);
  try { localStorage.setItem('showSidebar', String(!hidden)); } catch {}
  scheduleFit();
}
try { if (localStorage.getItem('showSidebar') === 'false') toggleSidebar(); else $('sidebar-toggle').classList.add('active'); } catch { $('sidebar-toggle').classList.add('active'); }
$('sidebar-toggle').onclick = toggleSidebar;
$('home').onclick = goHome; $('go-home').onclick = goHome;
$('sidebar-options').onclick = () => $('settings').click();
installNavigation({ $, api, perform, update, state: () => state, openSession, project: () => ({ projectID, sessionID }), selectProject, extra: { sidebar: toggleSidebar, home: goHome, tab: index => { const tabs = [...$('sessions').querySelectorAll('.tab')]; tabs[index]?.click(); } } });

$('provider-history').onclick = () => { if (!projectID) return; profileOptions($('provider-history-form').elements.profileID, $('provider-history-form').elements.agent.value); $('provider-history-dialog').showModal(); };
$('provider-history-form').elements.agent.onchange = event => profileOptions($('provider-history-form').elements.profileID, event.target.value);
$('provider-history-form').onsubmit = event => { event.preventDefault(); perform(async () => {
  const entries = await api.scanTranscripts(projectID, event.target.elements.agent.value, event.target.elements.profileID.value);
  $('transcript-list').replaceChildren(...entries.map(item => {
    const row = button('', false, () => perform(async () => { update(await api.importTranscript(item.providerID)); $('provider-history-dialog').close(); openSession(state.sessions.find(s => s.providerID === item.providerID).id); }));
    row.append(agentIcon(event.target.elements.agent.value, 14), text('span', item.title), text('span', 'Resume', 'kind')); return row;
  }));
  if (!entries.length) $('transcript-list').replaceChildren(text('p', 'No saved conversations found for this folder and account.'));
}); };

const openUsage = installUsage({ $, api, state: () => state, selected: () => sessionID });
