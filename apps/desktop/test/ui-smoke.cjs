// Exercise the bundled renderer and sandboxed preload without launching a provider.
const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const assert = require('node:assert/strict');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-ui-'));
app.setPath('userData', path.join(directory, 'profile'));
app.whenReady().then(async () => {
  const state = { projects: [{ id: 'p', title: 'Example project', path: directory }], sessions: [], running: [], quickCommands: [], profiles: [], specs: [], tasks: [] };
  let sentCommand, failStart = false;
  ipcMain.handle('files:snapshot', () => ({ branch: 'main', changes: [{ path: 'test.js', index: ' ', worktree: 'M' }], files: ['test.js'], log: [], branches: ['main'] }));
  ipcMain.handle('files:read', () => ({ text: 'diff --git a/test.js b/test.js\n--- a/test.js\n+++ b/test.js\n@@ -1 +1 @@\n-before\n+after\n', hash: 'fixture' }));
  ipcMain.handle('workspace:read', () => state);
  ipcMain.handle('session:create', (_event, details) => {
    state.sessions.push({ ...details, id: state.sessions.length ? 'review' + (state.sessions.length > 1 ? state.sessions.length : '') : 's', started: false, providerID: '' });
    return state;
  });
  ipcMain.handle('terminal:resize', () => {});
  ipcMain.handle('terminal:start', (_event, id) => { if (failStart) throw new Error('Fixture launch failed'); if (!state.running.includes(id)) state.running.push(id); state.sessions.find(s => s.id === id).started = true; return state; });
  ipcMain.handle('settings:save', (_event, settings) => { state.settings = settings; return state; });
  ipcMain.handle('session:edit', (_event, id, patch) => { Object.assign(state.sessions.find(s => s.id === id), patch); return state; });
  ipcMain.handle('review:brief', () => 'Please review the builder changes.');
  ipcMain.handle('history:read', () => 'Saved output from a previous run.');
  ipcMain.handle('command:save', (_event, command) => { state.quickCommands.push({ ...command, id: 'q' }); return state; });
  ipcMain.handle('command:send', (_event, id, commandID) => { sentCommand = { id, commandID }; });
  ipcMain.handle('profile:add', (_event, label, agent) => { state.profiles.push({ id: 'account', label, agent }); return state; });
  ipcMain.handle('spec:save', (_event, input) => { state.specs.push({ ...input, id: 'spec', revision: 1 }); return state; });
  ipcMain.handle('spec:approve', () => { state.specs[0].approvedRevision = 1; return state; });
  ipcMain.handle('task:save', (_event, input) => { state.tasks.push({ ...input, id: 'task', status: 'queued' }); return state; });
  ipcMain.handle('task:prepare', (_event, _id, profileID) => {
    state.tasks[0].sessionID = 'task-session'; state.tasks[0].specRevision = 1;
    state.sessions.push({ id: 'task-session', projectID: 'p', agent: 'codex', title: state.tasks[0].title, profileID, started: false, prompt: 'Task brief', providerID: '' }); return state;
  });
  const win = new BrowserWindow({ show: false, width: 1200, height: 800,
    webPreferences: { preload: path.join(__dirname, '../src/preload.cjs'), sandbox: true, contextIsolation: true, nodeIntegration: false } });
  await win.loadFile(path.join(__dirname, '../build/index.html'));
  const evaluate = script => win.webContents.executeJavaScript(script);
  async function until(script) {
    for (let i = 0; i < 100; i++) {
      if (await evaluate(script)) return;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`UI condition timed out: ${script}; app error: ${await evaluate('document.getElementById("error").textContent')}`);
  }
  await until('document.getElementById("project-title").textContent === "Example project"');
  assert.equal(await evaluate('typeof require'), 'undefined');
  await evaluate('document.getElementById("new-session").click()');
  assert.equal(await evaluate('document.getElementById("session-dialog").open'), true);
  await evaluate(`document.querySelector('[name="title"]').value = 'Smoke session'; document.getElementById('session-form').requestSubmit()`);
  await until('document.getElementById("sessions").textContent.includes("Smoke session")');
  await until("document.querySelector('.session-ready').hidden");
  assert.equal(state.running.includes('s'), true, 'Creating a session starts its agent without another click');
  await until('document.getElementById("start").disabled');
  win.webContents.send('terminal:data', { id: 's', data: '\r\nConvoy terminal renderer is connected.\r\n' });
  await until('document.querySelector(".xterm-screen") !== null');
  assert.equal(await evaluate('document.getElementById("error").hidden'), true);
  await evaluate('document.getElementById("settings").click()');
  await evaluate(`document.getElementById('settings-form').elements.theme.value = 'light'; document.getElementById('settings-form').elements.fontSize.value = '18'; document.getElementById('settings-form').requestSubmit()`);
  await until('document.documentElement.dataset.theme === "light"');
  assert.equal(state.settings.fontSize, 18);
  async function menu(action) {
    await evaluate(`document.getElementById('session-menu').value = ${JSON.stringify(action)}; document.getElementById('session-menu').dispatchEvent(new Event('change'))`);
  }
  await menu('edit');
  await until('document.getElementById("edit-dialog").open');
  assert.equal(await evaluate('document.getElementById("edit-form").elements.providerID.disabled'), true);
  await evaluate(`document.getElementById('edit-form').elements.title.value = 'Renamed builder'; document.getElementById('edit-form').elements.notes.value = 'Ready for review'; document.getElementById('edit-form').requestSubmit()`);
  await until('document.getElementById("sessions").textContent.includes("Renamed builder")');
  await menu('history');
  await until('document.getElementById("history-dialog").open');
  assert.equal(await evaluate('document.getElementById("history-value").textContent'), 'Saved output from a previous run.');
  await evaluate('document.getElementById("history-dialog").close()');
  await evaluate('document.getElementById("quick-commands").click()');
  await evaluate(`document.getElementById('command-form').elements.title.value = 'Run checks'; document.getElementById('command-form').elements.text.value = 'Run the tests'; document.getElementById('command-form').requestSubmit()`);
  await until('document.getElementById("command-list").textContent.includes("Run checks")');
  await evaluate('document.querySelector("#command-list button").click()');
  await until('!document.getElementById("commands-dialog").open');
  assert.deepEqual(sentCommand, { id: 's', commandID: 'q' });
  await menu('review');
  await until('document.getElementById("session-dialog").open');
  assert.equal(await evaluate('document.getElementById("session-form").elements.agent.value'), 'codex');
  assert.equal(await evaluate('document.getElementById("session-form").elements.prompt.value'), 'Please review the builder changes.');
  await evaluate('document.getElementById("session-form").requestSubmit()');
  await until('document.getElementById("review-link").textContent.includes("Builder")');
  assert.equal(state.sessions[1].reviewOf, 's');
  await menu('split');
  await until('document.getElementById("split-dialog").open');
  await evaluate('document.querySelector("#split-list button").click()');
  await until('document.querySelectorAll(".terminal-host:not([hidden])").length === 2');
  await evaluate('document.querySelectorAll(".terminal-host:not([hidden]) .pane-label")[0].click()');
  await until('document.getElementById("session-label").textContent.includes("Claude Code")');
  assert.equal(state.running.length, 2);
  assert.ok(await evaluate('[...document.querySelectorAll(".terminal-host:not([hidden])")].every(n => n.clientWidth > 100 && n.clientHeight > 100)'));
  if (process.env.CONVOY_TEST_SCREENSHOT) {
    await new Promise(resolve => setTimeout(resolve, 200));
    fs.writeFileSync(process.env.CONVOY_TEST_SCREENSHOT + '.split.png', (await win.webContents.capturePage()).toPNG());
  }
  win.setSize(820, 700);
  await until('window.innerWidth < 900');
  assert.ok(await evaluate(`(() => { const panes = [...document.querySelectorAll('.terminal-host:not([hidden])')].map(n => n.getBoundingClientRect()); return Math.abs(panes[0].left - panes[1].left) < 3 && Math.abs(panes[0].top - panes[1].top) > 100; })()`));
  win.setSize(1200, 800);
  await until('window.innerWidth > 900');
  await menu('unsplit');
  await until('document.querySelectorAll(".terminal-host:not([hidden])").length === 1');
  assert.equal(state.running.length, 2);
  await evaluate('document.getElementById("accounts").click()');
  await evaluate(`document.getElementById('profile-form').elements.label.value = 'Work'; document.getElementById('profile-form').elements.agent.value = 'codex'; document.getElementById('profile-form').requestSubmit()`);
  await until('document.getElementById("profile-list").textContent.includes("Work")');
  await evaluate('document.getElementById("accounts-dialog").close(); document.getElementById("planning").click(); document.getElementById("new-spec").click()');
  await evaluate(`document.getElementById('spec-form').elements.title.value = 'Accessible login'; document.getElementById('spec-form').elements.acceptance.value = 'Keyboard navigation works'; document.getElementById('spec-form').requestSubmit()`);
  await until('document.getElementById("spec-list").textContent.includes("Accessible login")');
  await evaluate('document.querySelectorAll("#spec-list button")[1].click()');
  await until('document.getElementById("spec-list").textContent.includes("Approved")');
  await evaluate('document.getElementById("new-task").click()');
  await evaluate(`document.getElementById('task-form').elements.title.value = 'Implement keyboard access'; document.getElementById('task-form').elements.agent.value = 'codex'; document.getElementById('task-form').elements.specID.value = 'spec'; document.getElementById('task-form').requestSubmit()`);
  await until('document.getElementById("task-list").textContent.includes("Implement keyboard access")');
  await evaluate('document.querySelectorAll("#task-list button")[1].click()');
  await until('document.getElementById("prepare-dialog").open');
  await evaluate(`document.getElementById('task-profile').value = 'account'; document.getElementById('prepare-form').requestSubmit()`);
  await until('document.getElementById("session-label").textContent.includes("Work")');
  assert.equal(state.sessions.at(-1).profileID, 'account');
  await evaluate('document.getElementById("planning").click()');
  assert.equal(await evaluate('document.getElementById("error").hidden'), true);
  if (process.env.CONVOY_TEST_SCREENSHOT) {
    await new Promise(resolve => setTimeout(resolve, 300));
    fs.writeFileSync(process.env.CONVOY_TEST_SCREENSHOT, (await win.webContents.capturePage()).toPNG());
  }
  await evaluate("for (const dialog of document.querySelectorAll('dialog[open]')) dialog.close(); document.getElementById('files-open').click()");
  await until("document.getElementById('files-list').textContent.includes('test.js')");
  await evaluate("document.querySelector('#files-list button').click()");
  await until("document.getElementById('file-preview').textContent.includes('after')");
  await evaluate("document.getElementById('diff-split').click()");
  assert.equal(await evaluate("document.querySelectorAll('.split-diff tr').length > 0"), true);
  await new Promise(resolve => setTimeout(resolve, 200));
  if (process.env.CONVOY_TEST_SCREENSHOT) fs.writeFileSync(process.env.CONVOY_TEST_SCREENSHOT + '.files.png', (await win.webContents.capturePage()).toPNG());
  await evaluate("document.getElementById('files-dialog').close(); document.getElementById('palette-open').click(); document.getElementById('palette-search').value = 'settings'; document.getElementById('palette-search').dispatchEvent(new Event('input')); document.querySelector('#palette-list button').click()");
  assert.equal(await evaluate("document.getElementById('settings-dialog').open"), true);
  failStart = true;
  await evaluate("document.getElementById('settings-dialog').close(); document.getElementById('new-session').click()");
  const countBeforeFailure = state.sessions.length;
  await evaluate("document.querySelector('#session-form [name=title]').value = 'Launch failure'; document.getElementById('session-form').requestSubmit(); document.getElementById('session-form').requestSubmit()");
  await until("document.getElementById('error').textContent.includes('Fixture launch failed')");
  assert.equal(state.sessions.length, countBeforeFailure + 1, 'Duplicate submits must not create duplicate sessions');
  assert.equal(state.sessions.at(-1).started, false, 'A launch failure preserves the saved session for retry');
  assert.equal(await evaluate("document.getElementById('start').disabled"), false);
  console.log('Renderer settings, reviews, split focus, account creation, spec approval, and task preparation passed');
  win.destroy();
  app.exit(0);
}).catch(error => { console.error(error); app.exit(1); });
setTimeout(() => { console.error('UI smoke test timed out'); app.exit(1); }, 15000).unref();
