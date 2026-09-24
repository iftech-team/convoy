const { app, BrowserWindow, ipcMain, dialog, Notification, powerSaveBlocker, shell } = require('electron');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const fs = require('node:fs');
const pty = require('node-pty');
const { Workspace } = require('./workspace.cjs');
const { launchSpec } = require('./launch.cjs');
const { History, plain, paste, reviewBrief } = require('./history.cjs');
const { git, createWorktree, gitStatus } = require('./git.cjs');
const { accountEnvironment } = require('./accounts.cjs');
const files = require('./files.cjs');
const telemetry = require('./telemetry.cjs');
const { codexLimits } = require('./provider.cjs');
const { markdown } = require('./planning.cjs');
const { resizeTerminal, exitDetails } = require('./terminal-lifecycle.cjs');

// Preview data is intentionally separate from the native macOS workspace.
app.setPath('userData', path.join(app.getPath('appData'), 'Convoy Desktop Preview'));
if (process.platform === 'win32') app.setAppUserModelId('com.iftech.convoy.desktop');
if (!app.requestSingleInstanceLock()) app.quit();
else app.whenReady().then(boot).catch(error => {
  dialog.showErrorBox('Unable to open Convoy', error.message);
  app.quit();
});

function boot() {
  const workspace = new Workspace(path.join(app.getPath('userData'), 'workspace.json'));
  const terminals = new Map();
  const queues = new Set();
  const queueBusy = new Set();
  const history = new History(path.join(app.getPath('userData'), 'TerminalHistory'));
  const busy = new Set();
  const telemetryRoot = path.join(app.getPath('userData'), 'telemetry');
  let wakeBlocker;
  const updateWake = () => {
    const mode = workspace.state.settings.keepAwake;
    const awake = mode === 'always' || (mode === 'sessions' && terminals.size > 0);
    if (awake && wakeBlocker === undefined) wakeBlocker = powerSaveBlocker.start('prevent-app-suspension');
    if (!awake && wakeBlocker !== undefined) { powerSaveBlocker.stop(wakeBlocker); wakeBlocker = undefined; }
  };
  const worktreeRoot = path.join(app.getPath('userData'), 'worktrees');
  const page = pathToFileURL(path.join(__dirname, '../build/index.html')).href;
  const win = new BrowserWindow({ width: 1200, height: 800, minWidth: 760, minHeight: 500,
    backgroundColor: '#111318', title: 'Convoy',
    webPreferences: { preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true } });
  win.removeMenu();
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  win.webContents.on('will-navigate', event => event.preventDefault());
  win.webContents.session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  win.webContents.session.setPermissionCheckHandler(() => false);
  app.on('second-instance', () => { if (win.isMinimized()) win.restore(); win.show(); win.focus(); });
  const send = (channel, payload) => { if (!win.isDestroyed()) win.webContents.send(channel, payload); };
  const state = () => ({ ...workspace.state, running: [...terminals.keys()], queues: [...queues] });
  const handle = (name, action) => ipcMain.handle(name, (event, ...args) => {
    if (event.sender !== win.webContents || event.senderFrame !== win.webContents.mainFrame || event.senderFrame.url !== page) throw new Error('Untrusted request.');
    return action(...args);
  });
  handle('workspace:read', state);
  handle('project:open', async () => {
    const result = await dialog.showOpenDialog(win, { properties: ['openDirectory'] });
    if (!result.canceled) workspace.addProject(result.filePaths[0]);
    return state();
  });
  handle('session:create', details => {
    if (details.reviewOf && busy.has(details.reviewOf)) throw new Error('Wait for the worktree operation to finish.');
    workspace.addSession(details); return state();
  });
  handle('session:edit', (id, patch) => {
    if (busy.has(id)) throw new Error('Wait for the worktree operation to finish.');
    workspace.editSession(id, patch, terminals.has(id)); return state();
  });
  handle('settings:save', settings => { workspace.saveSettings(settings); updateWake(); return state(); });
  handle('spec:save', input => { workspace.saveSpec(input); return state(); });
  handle('spec:approve', (id, revision) => { workspace.approveSpec(id, revision); return state(); });
  handle('spec:export', async id => {
    const spec = workspace.state.specs.find(s => s.id === id);
    if (!spec) throw new Error('Specification not found.');
    const content = markdown(spec, workspace.state.tasks);
    const result = await dialog.showSaveDialog(win, { title: 'Export specification', defaultPath: 'specification.md', filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (!result.canceled && result.filePath) fs.writeFileSync(result.filePath, content, 'utf8');
    return !result.canceled;
  });
  handle('task:save', input => { if (workspace.state.tasks.some(t => t.id === input.id && terminals.has(t.sessionID))) throw new Error('Stop the task session before editing.'); workspace.saveTask(input); return state(); });
  handle('task:status', (id, status) => { workspace.setTaskStatus(id, status); return state(); });
  handle('task:prepare', (id, profileID) => { workspace.prepareTask(id, profileID); return state(); });
  handle('profile:add', (label, agent) => { workspace.addProfile(label, agent); return state(); });
  handle('command:save', command => { workspace.saveCommand(command); return state(); });
  handle('command:delete', id => {
    workspace.update(next => { next.quickCommands = next.quickCommands.filter(c => c.id !== id); }); return state();
  });
  const output = id => { workspace.session(id); return plain(terminals.get(id)?.tail || history.read(id)); };
  handle('history:read', output);
  handle('review:brief', id => reviewBrief(workspace.session(id), output(id)));
  const insert = (id, text, submit = false) => {
    const entry = terminals.get(id);
    if (!entry || entry.stopping) throw new Error('Start or resume the target session first.');
    entry.child.write(paste(text, submit));
  };
  handle('terminal:paste', (id, text) => insert(id, text));
  handle('review:feedback', (id, text) => {
    const review = workspace.session(id);
    if (!review.reviewOf) throw new Error('This session is not linked to a builder.');
    insert(review.reviewOf, text);
    return review.reviewOf;
  });
  handle('command:send', (id, commandID) => {
    const session = workspace.session(id);
    const command = workspace.state.quickCommands.find(c => c.id === commandID && (!c.projectID || c.projectID === session.projectID));
    if (!command) throw new Error('Quick command not found for this project.');
    insert(id, command.text, command.submit);
  });
  const repositories = new Map();
  const directoryFor = id => {
    const session = workspace.state.sessions.find(s => s.id === id);
    const project = workspace.state.projects.find(p => p.id === (session?.projectID || id));
    if (!project) throw new Error('Project not found.');
    return session?.workingDirectory || project.path;
  };
  handle('files:snapshot', id => files.snapshot(directoryFor(id)));
  handle('files:read', async (id, request) => { if (request.kind === 'file' && request.structured) return files.fileContent(directoryFor(id), request.path); const text = await files.read(directoryFor(id), request); return request.structured ? { text, hash: files.digest(text) } : text; });
  handle('files:mutate', async (id, action, value) => {
    const requestedDirectory = directoryFor(id);
    const directory = await files.repositoryRoot(requestedDirectory) || requestedDirectory;
    if (repositories.has(directory)) throw new Error('Wait for the current Git operation.');
    repositories.set(directory, true);
    try {
      if (['discard', 'discardHunk', 'revert', 'resetSoft', 'resetMixed'].includes(action) || (action === 'commit' && value?.amend)) {
        const answer = await dialog.showMessageBox(win, { type: 'warning', message: `Confirm ${action}?`, detail: value?.path || value?.commit || 'Amend the latest commit.', buttons: ['Cancel', 'Continue'], defaultId: 0, cancelId: 0 });
        if (answer.response !== 1) return false;
      }
      if (action === 'trash') {
        const status = await files.snapshot(directory);
        if (!status.changes.some(f => f.path === value.path && f.untracked)) throw new Error('Only untracked files can be moved to Trash.');
        files.relative(value.path);
        const answer = await dialog.showMessageBox(win, { type: 'warning', message: 'Move untracked file to Trash?', detail: value.path, buttons: ['Cancel', 'Move to Trash'], defaultId: 0, cancelId: 0 });
        if (answer.response !== 1) return false;
        // Verify the parent rather than following the file's own symlink.
        const root = fs.realpathSync(directory), parent = fs.realpathSync(path.dirname(path.join(root, value.path)));
        if (parent !== root && !parent.startsWith(root + path.sep)) throw new Error('Path points outside repository.');
        await shell.trashItem(path.join(root, value.path)); return true;
      }
      if (action === 'pr') return await require('./repository-tools.cjs').createPR(directory);
      if (action === 'generate') {
        const session = workspace.state.sessions.find(s => s.id === id && s.agent === 'claude') || { agent: 'claude' };
        const account = accountEnvironment(session, workspace.state.profiles, path.join(app.getPath('userData'), 'accounts'));
        account.env.ELECTRON_RUN_AS_NODE = '1'; return await require('./repository-tools.cjs').generateMessage(directory, account.env);
      }
      await files.mutate(directory, action, value); return true;
    } finally { repositories.delete(directory); }
  });
  handle('project:discover', async () => {
    const result = await dialog.showOpenDialog(win, { title: 'Discover projects in a folder', properties: ['openDirectory'] });
    if (!result.canceled) {
      const root = result.filePaths[0];
      for (const directory of await files.discover(root)) workspace.addProject(directory);
      workspace.update(next => { for (const p of next.projects) if (p.path.startsWith(root + path.sep)) p.group = path.basename(root); });
    }
    return state();
  });
  handle('project:edit', (id, patch) => { workspace.editProject(id, patch); return state(); });
  handle('project:reconnect', async id => {
    if (workspace.state.sessions.some(s => s.projectID === id && terminals.has(s.id))) throw new Error('Stop project sessions before reconnecting.');
    const result = await dialog.showOpenDialog(win, { title: 'Reconnect project folder', properties: ['openDirectory'] });
    if (!result.canceled) workspace.editProject(id, { path: fs.realpathSync(result.filePaths[0]) });
    return state();
  });
  handle('project:remove', async id => {
    if (workspace.state.sessions.some(s => s.projectID === id && terminals.has(s.id))) throw new Error('Stop project sessions first.');
    const answer = await dialog.showMessageBox(win, { type: 'warning', message: 'Remove project and its saved sessions?', detail: 'Files, worktrees and provider conversations remain on disk.', buttons: ['Cancel', 'Remove'], defaultId: 0, cancelId: 0 });
    if (answer.response === 1) {
      if (workspace.state.sessions.some(s => s.projectID === id && terminals.has(s.id))) throw new Error('Stop project sessions first.');
      workspace.removeProject(id);
    }
    return state();
  });
  handle('profile:remove', id => {
    if (workspace.state.sessions.some(s => s.profileID === id)) throw new Error('This account is bound to saved sessions. Remove their projects before removing the profile.');
    workspace.update(next => { next.profiles = next.profiles.filter(p => p.id !== id); }); return state();
  });
  const transcriptChoices = new Map();
  handle('transcripts:scan', async (projectID, agent, profileID) => {
    if (!['claude', 'codex'].includes(agent)) throw new Error('Invalid provider.');
    const project = workspace.state.projects.find(p => p.id === projectID); if (!project) throw new Error('Project not found.');
    const account = accountEnvironment({ agent, profileID }, workspace.state.profiles, path.join(app.getPath('userData'), 'accounts'));
    const found = await require('./transcripts.cjs').scan(agent, account.home, project.path);
    transcriptChoices.clear();
    for (const item of found) transcriptChoices.set(item.providerID, { ...item, projectID, profileID, home: account.home });
    return found;
  });
  handle('transcripts:import', id => {
    const item = transcriptChoices.get(id); if (!item) throw new Error('Rescan provider history first.');
    const existing = workspace.state.sessions.find(s => s.providerID === id && s.agent === item.agent && s.agentHome === item.home);
    if (!existing) {
      workspace.addSession({ projectID: item.projectID, agent: item.agent, title: item.title || 'Imported session', profileID: item.profileID });
      workspace.update(next => Object.assign(next.sessions.at(-1), { providerID: id, started: true, agentHome: item.home }));
    }
    return state();
  });
  handle('session:recover', id => {
    if (terminals.has(id) || busy.has(id)) throw new Error('Stop the session before recovering.');
    workspace.recoverSession(id); return state();
  });
  handle('git:status', id => {
    const session = workspace.session(id);
    return gitStatus(session.workingDirectory || workspace.state.projects.find(p => p.id === session.projectID).path);
  });
  const makeWorktree = async (id, branch) => {
    const session = workspace.session(id);
    if (busy.has(id) || terminals.has(id) || session.started || session.workingDirectory || session.reviewOf || session.archived) throw new Error('Create a worktree on a new, stopped coding session.');
    busy.add(id);
    try {
      const project = workspace.state.projects.find(p => p.id === session.projectID);
      const directory = await createWorktree(project.path, worktreeRoot, branch);
      try {
        workspace.update(next => Object.assign(next.sessions.find(s => s.id === id), { workingDirectory: directory, branch, ownsWorktree: true }));
      } catch (error) {
        throw new Error(`Worktree created at ${directory}, but its session could not be saved: ${error.message}. The worktree and branch have been preserved.`);
      }
      workspace.record('worktree', workspace.session(id), `Created worktree on ${branch}`);
      if (project.sharedPaths || project.setupCommand) {
        const answer = await dialog.showMessageBox(win, { type: 'question', message: 'Run this project’s worktree setup?', detail: `Copies: ${project.sharedPaths || 'none'}\nCommand: ${project.setupCommand || 'none'}\nFolder: ${directory}`, buttons: ['Skip setup', 'Run setup'], defaultId: 0, cancelId: 0 });
        if (answer.response === 1) await require('./worktree-setup.cjs').setup(project.path, directory, (project.sharedPaths || '').split('\n').map(s => s.trim()).filter(Boolean), project.setupCommand);
      }
      return state();
    } finally { busy.delete(id); }
  };
  handle('worktree:create', makeWorktree);
  handle('worktree:remove', async id => {
    const session = workspace.session(id);
    const directory = session.workingDirectory;
    if (!session.ownsWorktree || !directory || path.dirname(directory) !== worktreeRoot) throw new Error('Only worktrees created by this app can be removed.');
    const linked = workspace.state.sessions.filter(s => s.workingDirectory === directory);
    if (linked.some(s => terminals.has(s.id) || busy.has(s.id))) throw new Error('Stop all sessions using this worktree first.');
    const result = await dialog.showMessageBox(win, { type: 'warning', message: 'Remove this worktree?',
      detail: `Git will remove ${directory} only if it is clean. The branch will be kept. All ${linked.length} linked sessions will be archived.`,
      buttons: ['Cancel', 'Remove worktree'], defaultId: 0, cancelId: 0 });
    if (result.response !== 1) return state();
    // Recheck after the asynchronous dialog before acquiring the worktree lock.
    const current = workspace.state.sessions.filter(s => s.workingDirectory === directory);
    if (repositories.has(directory) || current.some(s => terminals.has(s.id) || busy.has(s.id))) throw new Error('Stop all sessions using this worktree first.');
    for (const s of current) busy.add(s.id);
    try {
      const project = workspace.state.projects.find(p => p.id === session.projectID);
      await git(project.path, ['worktree', 'remove', directory]); // Never --force; retain the branch.
      workspace.update(next => {
        for (const s of next.sessions.filter(s => s.workingDirectory === directory)) {
          s.archived = true; s.worktreeRemoved = true;
        }
      });
      return state();
    } finally { for (const s of current) busy.delete(s.id); }
  });
  const start = id => {
    if (terminals.has(id)) return state();
    const session = workspace.state.sessions.find(s => s.id === id);
    if (!session) throw new Error('Session not found.');
    if (session.archived || session.worktreeRemoved) throw new Error('This session is archived or its worktree was removed.');
    if (busy.has(id)) throw new Error('Wait for the worktree operation to finish.');
    const project = workspace.state.projects.find(p => p.id === session.projectID);
    const directory = session.workingDirectory || project.path;
    if (!fs.statSync(directory).isDirectory()) throw new Error('Session folder is unavailable.');
    if (session.taskID) {
      const task = workspace.state.tasks.find(t => t.id === session.taskID);
      const specification = workspace.state.specs.find(s => s.id === task.specID);
      if (task.sessionID !== id || (specification && (specification.approvedRevision !== specification.revision || task.specRevision !== specification.revision))) throw new Error('This task brief is outdated or unapproved. Approve the spec and prepare a new task brief.');
    }
    const account = accountEnvironment(session, workspace.state.profiles, path.join(app.getPath('userData'), 'accounts'));
    if (session.profileID) {
      if (session.started && !fs.existsSync(account.home)) throw new Error('This session’s account folder is missing. Restore it before resuming.');
      fs.mkdirSync(account.home, { recursive: true });
    }
    account.env.ELECTRON_RUN_AS_NODE = '1';
    const settingsFile = session.agent === 'claude' ? telemetry.configuration(telemetryRoot, session, process.execPath, workspace.state.settings.claudeUsage) : undefined;
    const spec = launchSpec(session, session.started, process.platform, account.env,
      { _enhanced: true, _settingsFile: settingsFile, [session.agent === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME']: account.home });
    const child = pty.spawn(spec.file, spec.args, { name: 'xterm-256color', cwd: directory, env: account.env, cols: 100, rows: 30 });
    try { workspace.update(next => {
      const savedSession = next.sessions.find(s => s.id === id);
      Object.assign(savedSession, { started: true, agentHome: account.home });
      delete savedSession.lastExit;
      const task = next.tasks.find(t => t.id === session.taskID);
      if (task) { task.status = 'building'; delete task.lastError; }
    }); }
    catch (error) { child.kill(); throw error; }
    const entry = { child, tail: '', stopping: false, savedAt: 0 };
    terminals.set(id, entry); updateWake();
    try { workspace.record(session.started ? 'resumed' : 'started', session, 'Agent process launched'); } catch (error) { send('app:error', error.message); }
    child.onData(data => {
      send('terminal:data', { id, data });
      entry.tail = (entry.tail + data).slice(-48000);
      if (Date.now() - entry.savedAt > 2000) {
        entry.savedAt = Date.now();
        try { history.save(id, entry.tail); } catch (error) { send('app:error', `Could not save terminal output: ${error.message}`); }
      }
      if (session.agent === 'codex') {
        const match = entry.tail.match(/codex resume ([0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12})/i);
        if (match && workspace.state.sessions.find(s => s.id === id).providerID !== match[1]) {
          try {
            workspace.update(next => { next.sessions.find(s => s.id === id).providerID = match[1]; });
            send('workspace:changed', state());
          } catch (error) { send('app:error', error.message); }
        }
      }
    });
    child.onExit(({ exitCode }) => {
      entry.exited = true;
      const lastExit = exitDetails(session, exitCode, entry.tail, entry.stopping);
      try { history.save(id, entry.tail); } catch (error) { send('app:error', `Could not save terminal output: ${error.message}`); }
      terminals.delete(id); updateWake();
      try {
        workspace.update(next => {
          next.sessions.find(s => s.id === id).lastExit = lastExit;
          const task = next.tasks.find(t => t.sessionID === id);
          if (task) {
            const specification = next.specs.find(s => s.id === task.specID);
            task.status = specification && task.specRevision !== specification.revision ? 'changes' : entry.hibernating ? 'review' : entry.stopping || exitCode !== 0 ? 'failed' : 'review';
            if (!entry.hibernating && (entry.stopping || exitCode !== 0)) task.lastError = entry.stopping ? 'Session stopped by user.' : `Agent exited with code ${exitCode}.`;
          }
        });
        workspace.record('exited', session, entry.stopping ? 'Stopped by user' : `Process exited with code ${exitCode}; task completion is unverified`);
      } catch (error) { send('app:error', error.message); }
      send('terminal:exit', { id, exitCode, ...lastExit });
      if (!entry.stopping && exitCode === 0) finishTask(id).catch(error => { queues.delete(session.projectID); send('app:error', error.message); send('workspace:changed', state()); });
      else if (!entry.hibernating) queues.delete(session.projectID);
      send('workspace:changed', state());
      if (!entry.stopping && workspace.state.settings.notifications && !win.isDestroyed() && !win.isFocused() && Notification.isSupported()) {
        try {
        const notification = new Notification({ title: 'Convoy session exited', body: `${session.title} · exit ${exitCode}` });
        notification.on('click', () => { if (!win.isDestroyed()) { if (win.isMinimized()) win.restore(); win.show(); win.focus(); send('session:focus', id); } });
        notification.on('failed', (_event, error) => send('app:error', `Notification could not be shown: ${error}`));
        notification.show();
        } catch (error) { send('app:error', `Notification could not be shown: ${error.message}`); }
      }
    });
    return state();
  };
  handle('terminal:start', start);
  async function runNext(projectID) {
    if (!queues.has(projectID) || queueBusy.has(projectID) || workspace.state.sessions.some(s => s.projectID === projectID && terminals.has(s.id) && workspace.state.tasks.some(t => t.sessionID === s.id && t.status === 'building'))) return;
    queueBusy.add(projectID);
    try {
      const task = workspace.state.tasks.find(t => t.projectID === projectID && t.status === 'queued');
      if (!task) { queues.delete(projectID); return; }
      workspace.prepareTask(task.id);
      const current = workspace.state.tasks.find(t => t.id === task.id);
      const session = workspace.session(current.sessionID);
      if (task.mode === 'pr' && !session.workingDirectory) await makeWorktree(session.id, `convoy/task-${task.id.slice(0, 8)}`);
      if (queues.has(projectID)) start(session.id);
    } finally { queueBusy.delete(projectID); send('workspace:changed', state()); }
  }
  async function finishTask(id) {
    const task = workspace.state.tasks.find(t => t.sessionID === id);
    if (!task || task.status !== 'review') return;
    const spec = workspace.state.specs.find(s => s.id === task.specID);
    if (spec && (spec.approvedRevision !== spec.revision || task.specRevision !== spec.revision)) {
      queues.delete(task.projectID); workspace.update(next => { next.tasks.find(t => t.id === task.id).status = 'changes'; }); return;
    }
    const source = workspace.session(id), project = workspace.state.projects.find(p => p.id === source.projectID);
    if (task.autoReview && !workspace.state.sessions.some(s => s.reviewOf === id)) {
      workspace.addSession({ projectID: source.projectID, title: `Review: ${source.title}`.slice(0, 200), agent: source.agent === 'claude' ? 'codex' : 'claude', reviewOf: id,
        prompt: `${project.reviewTemplate || 'Review correctness, regressions, tests and security. Report findings without editing code.'}\n\n${reviewBrief(source, output(id))}`.slice(0, 32000) });
      start(workspace.state.sessions.at(-1).id);
    }
    await runNext(source.projectID);
  }
  handle('queue:toggle', async projectID => {
    if (queues.has(projectID)) { queues.delete(projectID); return state(); }
    const tasks = workspace.state.tasks.filter(t => t.projectID === projectID && t.status === 'queued');
    if (!tasks.length) throw new Error('No queued tasks.');
    const answer = await dialog.showMessageBox(win, { type: 'question', message: `Run ${tasks.length} queued tasks?`, detail: tasks.map(t => `${t.title}: ${t.mode || 'none'}${t.autoReview ? ', automatic review' : ''}`).join('\n') + '\nRuns installed agents. Publishing occurs only for tasks explicitly configured for PR or push. Failures pause the queue.', buttons: ['Cancel', 'Run queue'], defaultId: 0, cancelId: 0 });
    if (answer.response === 1) { queues.add(projectID); try { await runNext(projectID); } catch (error) { queues.delete(projectID); throw error; } }
    return state();
  });
  handle('terminal:write', (id, data) => {
    if (typeof data !== 'string' || data.length > 65536) throw new Error('Invalid terminal input.');
    terminals.get(id)?.child.write(data);
  });
  handle('terminal:resize', (id, cols, rows) => {
    if (![cols, rows].every(n => Number.isInteger(n) && n >= 1 && n <= 1000)) throw new Error('Invalid terminal size.');
    resizeTerminal(terminals.get(id), cols, rows);
  });
  const stop = id => {
    const entry = terminals.get(id);
    if (!entry || entry.stopping) return;
    entry.stopping = true;
    if (process.platform === 'win32') entry.child.kill();
    else {
      try { process.kill(-entry.child.pid, 'SIGHUP'); }
      catch { entry.child.kill(); }
    }
    if (process.platform !== 'win32') setTimeout(() => {
      if (terminals.get(id) === entry) {
        try { process.kill(-entry.child.pid, 'SIGKILL'); } catch { entry.child.kill('SIGKILL'); }
      }
    }, 1500).unref();
  };
  handle('terminal:stop', stop);
  handle('usage:read', async id => {
    const session = workspace.session(id);
    if (session.agent === 'claude') {
      const usage = telemetry.read(telemetryRoot, id, '.usage');
      return { at: usage?.at, windows: (usage?.windows || []).filter(w => !w.resetsAt || w.resetsAt > Date.now() / 1000) };
    }
    const account = accountEnvironment(session, workspace.state.profiles, path.join(app.getPath('userData'), 'accounts'));
    account.env.ELECTRON_RUN_AS_NODE = '1';
    return { at: Date.now(), windows: await codexLimits(account.env, directoryFor(id)) };
  });
  const monitor = setInterval(() => {
    try {
    for (const [id, entry] of terminals) {
      const value = telemetry.read(telemetryRoot, id, '.status');
      if (!value || value.at <= (entry.statusAt || 0) || value.at < Date.now() - 30 * 60000) continue;
      const changed = entry.agentState !== value.state;
      entry.statusAt = value.at; entry.agentState = value.state;
      if (changed && ['done', 'waiting'].includes(value.state)) workspace.record(value.state, workspace.session(id), value.state === 'done' ? 'Agent completed a turn; acceptance remains unverified' : 'Agent needs input or permission');
      send('agent:status', { id, ...value });
      if (value.state === 'working') { const task = workspace.state.tasks.find(t => t.sessionID === id && t.status === 'review'); if (task) workspace.update(next => { next.tasks.find(t => t.id === task.id).status = 'building'; }); }
      if (value.state === 'done') {
        const task = workspace.state.tasks.find(t => t.sessionID === id && t.status === 'building');
        if (task) { workspace.update(next => { next.tasks.find(t => t.id === task.id).status = 'review'; }); finishTask(id).catch(error => { queues.delete(workspace.session(id).projectID); send('app:error', error.message); }); }
      }
      if (changed && ['done', 'waiting'].includes(value.state) && workspace.state.settings.notifications && !win.isFocused() && Notification.isSupported()) {
        const notification = new Notification({ title: value.state === 'done' ? 'Agent finished a turn' : 'Agent needs your attention', body: workspace.session(id).title });
        notification.on('click', () => { if (!win.isDestroyed()) { win.show(); win.focus(); send('session:focus', id); } });
        notification.on('failed', () => {}); notification.show();
      }
      send('workspace:changed', state());
    }
    const idle = workspace.state.settings.hibernateMinutes;
    if (idle > 0) for (const [id, entry] of terminals) if (!entry.stopping && entry.agentState === 'done' && Date.now() - entry.statusAt > idle * 60000) { entry.hibernating = true; workspace.record('hibernated', workspace.session(id), 'Stopped after completed-turn idle timeout'); stop(id); }
    } catch (error) { send('app:error', error.message); }
  }, 2000);
  monitor.unref(); updateWake();
  win.on('closed', () => clearInterval(monitor));
  let closing = false;
  win.on('close', event => {
    if (!terminals.size || closing) return;
    event.preventDefault();
    const choice = dialog.showMessageBoxSync(win, { type: 'question', message: 'Stop running sessions and quit?',
      detail: 'Agent processes will stop. Saved sessions can be resumed after reopening Convoy.',
      buttons: ['Keep working', 'Stop and quit'], defaultId: 0, cancelId: 0 });
    if (choice !== 1) return;
    closing = true;
    for (const id of terminals.keys()) stop(id);
    setTimeout(() => win.close(), 1800);
  });
  // A renderer failure must not leave invisible sessions running indefinitely.
  win.webContents.on('render-process-gone', () => { for (const id of terminals.keys()) stop(id); });
  app.on('window-all-closed', () => app.quit());
  win.loadURL(page);
}
