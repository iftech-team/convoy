// Regression checks for slow Git responses, project cleanup and terminal resize churn.
const { app, BrowserWindow, ipcMain } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const assert = require('node:assert/strict');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-review-'));
app.setPath('userData', path.join(directory, 'profile'));
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
app.whenReady().then(async () => {
  const state = { projects: [{ id: 'p', title: 'Review fixture', path: directory }], sessions: [{ id: 's', projectID: 'p', title: 'Builder', agent: 'claude', started: true }], running: ['s'] };
  let snapshot = { branch: 'main', changes: [{ path: 'file.txt', index: 'M', worktree: ' ' }], files: ['file.txt'], log: [], branches: ['main'] };
  let snapshotReply = () => structuredClone(snapshot);
  let readReply = (_id, request) => ({ text: `${request.kind} preview`, hash: 'fixture' });
  let mutateReply = () => true;
  let resizeCount = 0, lastRequest;
  ipcMain.handle('workspace:read', () => state);
  ipcMain.handle('files:snapshot', () => snapshotReply());
  ipcMain.handle('files:read', (event, id, request) => { lastRequest = request; return readReply(id, request); });
  ipcMain.handle('files:mutate', (event, id, action, value) => mutateReply(id, action, value));
  ipcMain.handle('terminal:resize', () => { resizeCount++; });
  const win = new BrowserWindow({ show: false, width: 1200, height: 800,
    webPreferences: { preload: path.join(__dirname, '../src/preload.cjs'), sandbox: true, contextIsolation: true, nodeIntegration: false } });
  const evaluate = code => win.webContents.executeJavaScript(code);
  async function until(code) {
    for (let i = 0; i < 100; i++) { if (await evaluate(code)) return; await delay(30); }
    throw new Error(`Timed out: ${code}`);
  }
  async function capture(name) {
    if (!process.env.CONVOY_REVIEW_SCREENSHOTS) return;
    await delay(100);
    fs.writeFileSync(path.join(process.env.CONVOY_REVIEW_SCREENSHOTS, name + '.png'), (await win.webContents.capturePage()).toPNG());
  }
  await win.loadFile(path.join(__dirname, '../build/index.html'));
  await until(`document.getElementById('project-title').textContent === 'Review fixture'`);
  await capture('01-project');
  await evaluate(`document.querySelector('#sessions button').click()`);
  await until(`document.querySelector('.xterm-screen') !== null`);
  await delay(100);
  const initialResizes = resizeCount;
  assert(initialResizes > 0);
  for (let i = 0; i < 10; i++) win.webContents.send('agent:status', { id: 's', state: 'working' });
  await until(`document.getElementById('status').textContent === 'Agent: working'`);
  await delay(100);
  assert.equal(resizeCount, initialResizes, 'status updates must not resize an unchanged PTY');
  win.webContents.send('terminal:data', { id: 's', data: 'Fixture agent is ready.\r\n' });
  await capture('02-session');

  await evaluate(`document.getElementById('files-open').click()`);
  await until(`document.querySelector('#files-list button') !== null`);
  await evaluate(`document.querySelector('#files-list button').click()`);
  await until(`document.getElementById('file-preview').textContent === 'staged preview'`);
  assert.equal(lastRequest.kind, 'staged', 'staged-only rows must open the staged diff');

  // A file read during a slow refresh must not invalidate that refresh.
  const refreshing = deferred(); snapshotReply = () => refreshing.promise;
  await evaluate(`document.getElementById('files-refresh').click()`);
  await until(`document.getElementById('files-refresh').disabled`);
  await evaluate(`document.querySelector('#files-list button').click()`);
  await until(`document.getElementById('file-preview').textContent === 'staged preview'`);
  refreshing.resolve({ ...snapshot, branch: 'refreshed-branch' });
  await until(`document.getElementById('files-branch').textContent === 'refreshed-branch'`);
  snapshotReply = () => structuredClone(snapshot);

  // Staging automatically switches the selected preview to its new location.
  snapshot.changes[0] = { path: 'file.txt', index: ' ', worktree: 'M' };
  await evaluate(`document.getElementById('files-refresh').click()`);
  await until(`document.getElementById('file-preview').textContent === 'unstaged preview'`);
  const staging = deferred();
  mutateReply = async () => { await staging.promise; snapshot.changes[0] = { path: 'file.txt', index: 'M', worktree: ' ' }; return true; };
  await evaluate(`[...document.querySelectorAll('#files-list button')].find(b => b.textContent === 'Stage').click()`);
  await until(`document.getElementById('files-progress').textContent === 'Applying Git operation…'`);
  assert(await evaluate(`[...document.querySelectorAll('[data-file-action]')].every(b => b.disabled)`));
  staging.resolve();
  await until(`document.getElementById('file-preview').textContent === 'staged preview' && !document.getElementById('files-refresh').disabled`);

  // Delayed results from an earlier dialog must not overwrite a reopened draft.
  const generated = deferred(); mutateReply = () => generated.promise;
  await evaluate(`document.getElementById('git-message').value = 'My draft'; document.getElementById('git-generate').click()`);
  await until(`document.getElementById('files-progress').textContent === 'Generating commit message…'`);
  await evaluate(`document.getElementById('files-dialog').close()`);
  await delay(30);
  await evaluate(`document.getElementById('files-open').click()`);
  await until(`document.querySelector('#files-list button') !== null`);
  generated.resolve('Late generated message');
  await until(`!document.getElementById('git-generate').disabled`);
  assert.equal(await evaluate(`document.getElementById('git-message').value`), 'My draft');

  const pendingRead = deferred(); readReply = () => pendingRead.promise;
  await evaluate(`document.querySelector('#files-list button').click()`);
  await until(`document.getElementById('file-preview').textContent === 'Loading…'`);
  await evaluate(`document.getElementById('files-dialog').close()`); await delay(30);
  await evaluate(`document.getElementById('files-open').click()`);
  await until(`document.querySelector('#files-list button') !== null`);
  pendingRead.resolve({ text: 'Stale result', hash: 'old' }); await delay(50);
  assert.notEqual(await evaluate(`document.getElementById('file-preview').textContent`), 'Stale result');
  readReply = () => ({ text: 'diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1,2 +1,2 @@\n-before\n+after\n---literal\n+++literal\n', hash: 'fixture' });
  await evaluate(`document.querySelector('#files-list button').click(); document.getElementById('diff-split').click()`);
  await until(`document.querySelector('.split-diff') !== null`);
  assert.equal(await evaluate(`document.querySelectorAll('.split-diff .removed').length`), 2, 'hunk lines starting with --- are content, not metadata');
  assert.equal(await evaluate(`document.querySelectorAll('.split-diff .added').length`), 2);
  await capture('03-changes');
  win.setSize(760, 500); await delay(100);
  assert(await evaluate(`document.getElementById('files-dialog').scrollWidth <= document.getElementById('files-dialog').clientWidth`), 'Git dialog must fit the minimum window size');
  await capture('04-compact-changes');

  // A resumed PTY still gets its dimensions, even if the pane size did not change.
  await evaluate(`document.getElementById('files-dialog').close()`);
  state.running = []; win.webContents.send('workspace:changed', state);
  await until(`!document.getElementById('start').disabled`);
  const beforeResume = resizeCount;
  state.running = ['s']; win.webContents.send('workspace:changed', state);
  await until(`document.getElementById('start').disabled`); await delay(100);
  assert.equal(resizeCount, beforeResume + 1);
  state.queues = ['p']; win.webContents.send('workspace:changed', state);
  await evaluate(`document.getElementById('planning').click()`);
  await until(`document.getElementById('run-queue').textContent === 'Pause queue'`);
  state.queues = []; win.webContents.send('workspace:changed', state);
  await until(`document.getElementById('run-queue').textContent === 'Run queue'`);
  await evaluate(`document.getElementById('planning-dialog').close()`);

  // Removing the project frees its terminal and ignores any queued output for it.
  await evaluate(`document.getElementById('files-dialog').close()`);
  win.webContents.send('workspace:changed', { projects: [], sessions: [], running: [] });
  await until(`document.querySelectorAll('.terminal-host').length === 0`);
  win.webContents.send('terminal:data', { id: 's', data: 'late output' }); await delay(50);
  assert.equal(await evaluate(`document.querySelectorAll('.terminal-host').length`), 0);
  assert(await evaluate(`document.getElementById('files-open').disabled`));
  console.log('Git race, staged preview, draft preservation, compact layout, terminal resize and cleanup regressions passed');
  win.destroy(); app.exit(0);
}).catch(error => { console.error(error); app.exit(1); });
setTimeout(() => { console.error('Review smoke test timed out'); app.exit(1); }, 20000).unref();
