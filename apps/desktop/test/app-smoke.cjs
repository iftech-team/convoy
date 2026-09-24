// Real app/IPC/persistence/PTY lifecycle, substituting only the provider command
// and folder picker. This never invokes a model or touches the user's workspace.
const { app, BrowserWindow, dialog } = require('electron');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const { git } = require('../src/git.cjs');
const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-app-'));
app.setPath('appData', directory);
dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [directory] });
dialog.showMessageBox = async () => ({ response: 1 });
dialog.showSaveDialog = async () => ({ canceled: false, filePath: path.join(directory, 'export.md') });
const launch = require('../src/launch.cjs');
const originalLaunch = launch.launchSpec;
launch.launchSpec = session => {
  const file = originalLaunch({ agent: 'codex', providerID: '', prompt: '' }, false).file;
  if (session.title === 'Missing conversation') {
    const message = 'No conversation found with session ID: ' + session.providerID;
    return { file, args: process.platform === 'win32' ? ['-NoLogo', '-NoProfile', '-Command', 'Write-Output "' + message + '"; exit 1'] : ['-c', 'printf "%s\\n" "' + message + '"; exit 1'] };
  }
  if (session.title === 'Success task') return { file, args: process.platform === 'win32' ? ['-NoLogo', '-NoProfile', '-Command', 'Write-Output "SUCCESS"; exit 0'] : ['-c', 'printf "SUCCESS\\n"; exit 0'] };
  return process.platform === 'win32'
    ? { file, args: ['-NoLogo', '-NoProfile', '-Command', 'Write-Output "FIXTURE_READY"; $line = [Console]::ReadLine(); Write-Output ("RECEIVED_" + $line); Start-Sleep -Seconds 30'] }
    : { file, args: ['-c', 'printf "FIXTURE_READY\\n"; read value; printf "RECEIVED_%s\\n" "$value"; sleep 30'] };
};
require('../src/main.cjs');
const timeout = setTimeout(() => { console.error('App smoke test timed out'); app.exit(1); }, 45000);
app.whenReady().then(async () => {
  let win;
  async function until(check) {
    for (let i = 0; i < 150; i++) {
      const result = await check();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('App condition timed out');
  }
  win = await until(() => BrowserWindow.getAllWindows()[0]);
  const evaluate = script => win.webContents.executeJavaScript(script);
  await until(async () => !win.webContents.isLoading() && await evaluate('!!window.convoy'));
  await evaluate('window.testOutput = ""; window.convoy.onData(({data}) => window.testOutput += data); true');
  let state = await evaluate('window.convoy.openFolder()');
  assert.equal(state.projects.length, 1);
  state = await evaluate(`window.convoy.createSession(${JSON.stringify({ projectID: state.projects[0].id, agent: 'codex', title: 'Lifecycle test' })})`);
  const id = JSON.stringify(state.sessions[0].id);
  state = await evaluate(`window.convoy.start(${id})`);
  assert.equal(state.running.length, 1);
  await assert.rejects(evaluate(`window.convoy.editSession(${id}, {archived: true})`));
  await until(async () => (await evaluate('window.testOutput')).includes('FIXTURE_READY'));
  await evaluate(`window.convoy.resize(${id}, 120, 35)`);
  await evaluate(`window.convoy.write(${id}, ${JSON.stringify('probe\r')})`);
  await until(async () => (await evaluate('window.testOutput')).includes('RECEIVED_probe'));
  // A duplicate start must retain the same PTY.
  state = await evaluate(`window.convoy.start(${id})`);
  assert.equal(state.running.length, 1);
  await evaluate(`window.convoy.stop(${id})`);
  await until(async () => (await evaluate('window.convoy.read()')).running.length === 0);
  const saved = JSON.parse(fs.readFileSync(path.join(directory, 'Convoy Desktop Preview/workspace.json'), 'utf8'));
  assert.equal(saved.sessions[0].started, true);
  assert.ok((await evaluate(`window.convoy.history(${id})`)).includes('RECEIVED_probe'));
  state = await evaluate(`window.convoy.editSession(${id}, {title: 'Renamed', notes: 'Ready for review', pinned: true})`);
  assert.equal(state.sessions[0].title, 'Renamed');
  const brief = await evaluate(`window.convoy.reviewBrief(${id})`);
  assert.ok(brief.includes('Ready for review'));
  const projectID = state.projects[0].id;
  state = await evaluate(`window.convoy.createSession(${JSON.stringify({ projectID, title: 'Review', agent: 'claude', reviewOf: saved.sessions[0].id, prompt: brief })})`);
  await assert.rejects(evaluate(`window.convoy.feedback(${JSON.stringify(state.sessions[1].id)}, 'Fix the findings')`)); // builder stopped
  await git(directory, ['init']);
  await git(directory, ['-c', 'user.name=Convoy Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgSign=false', 'commit', '--allow-empty', '-m', 'initial']);
  state = await evaluate(`window.convoy.createSession(${JSON.stringify({ projectID, title: 'Isolated task', agent: 'codex' })})`);
  const isolatedID = state.sessions.at(-1).id;
  state = await evaluate(`window.convoy.createWorktree(${JSON.stringify(isolatedID)}, 'convoy/smoke')`);
  const checkout = state.sessions.at(-1).workingDirectory;
  assert.ok(fs.existsSync(checkout));
  state = await evaluate(`window.convoy.createSession(${JSON.stringify({ projectID, title: 'Worktree review', agent: 'claude', reviewOf: isolatedID })})`);
  assert.equal(state.sessions.at(-1).workingDirectory, checkout);
  state = await evaluate(`window.convoy.removeWorktree(${JSON.stringify(isolatedID)})`);
  assert.equal(fs.existsSync(checkout), false);
  assert.equal(state.sessions.at(-1).archived, true);
  assert.equal(state.sessions.at(-2).archived, true);
  await assert.rejects(evaluate(`window.convoy.start(${JSON.stringify(isolatedID)})`));
  state = await evaluate(`window.convoy.addProfile('Work', 'codex')`);
  const profileID = state.profiles[0].id;
  state = await evaluate(`window.convoy.saveSpec(${JSON.stringify({ projectID, title: 'Planned feature', acceptance: 'Tests pass' })})`);
  const specID = state.specs[0].id;
  state = await evaluate(`window.convoy.saveTask(${JSON.stringify({ projectID, specID, title: 'Implement feature', agent: 'codex' })})`);
  const taskID = state.tasks[0].id;
  await assert.rejects(evaluate(`window.convoy.prepareTask(${JSON.stringify(taskID)}, ${JSON.stringify(profileID)})`));
  await evaluate(`window.convoy.approveSpec(${JSON.stringify(specID)}, 1)`);
  state = await evaluate(`window.convoy.prepareTask(${JSON.stringify(taskID)}, ${JSON.stringify(profileID)})`);
  const taskSessionID = state.tasks[0].sessionID;
  await evaluate(`window.convoy.exportSpec(${JSON.stringify(specID)})`);
  assert.match(fs.readFileSync(path.join(directory, 'export.md'), 'utf8'), /Tests pass/);
  state = await evaluate(`window.convoy.start(${JSON.stringify(taskSessionID)})`);
  assert.equal(state.tasks[0].status, 'building');
  const accountHome = state.sessions.find(s => s.id === taskSessionID).agentHome;
  assert.ok(fs.existsSync(accountHome));
  assert.ok(accountHome.startsWith(path.join(directory, 'Convoy Desktop Preview', 'accounts')));
  await evaluate(`window.convoy.stop(${JSON.stringify(taskSessionID)})`);
  await until(async () => (await evaluate('window.convoy.read()')).running.length === 0);
  state = await evaluate('window.convoy.read()');
  assert.equal(state.tasks[0].status, 'failed');
  assert.ok(state.activity.some(item => item.sessionID === taskSessionID && item.kind === 'exited'));
  state = await evaluate(`window.convoy.saveSpec(${JSON.stringify({ ...state.specs[0], acceptance: 'More tests pass' })})`);
  await assert.rejects(evaluate(`window.convoy.start(${JSON.stringify(taskSessionID)})`));
  state = await evaluate(`window.convoy.saveTask(${JSON.stringify({ projectID, title: 'Success task', agent: 'codex' })})`);
  const successTaskID = state.tasks.at(-1).id;
  state = await evaluate(`window.convoy.prepareTask(${JSON.stringify(successTaskID)})`);
  await evaluate(`window.convoy.start(${JSON.stringify(state.tasks.at(-1).sessionID)})`);
  await until(async () => (await evaluate('window.convoy.read()')).tasks.at(-1).status === 'review');
  assert.equal(await evaluate('typeof require'), 'undefined');
  for (let i = 0; i < 2; i++) await evaluate(`window.convoy.saveTask(${JSON.stringify({ projectID, title: 'Success task', agent: 'codex', mode: 'none' })})`);
  await evaluate(`window.convoy.toggleQueue(${JSON.stringify(projectID)})`);
  await until(async () => { const next = await evaluate('window.convoy.read()'); return next.tasks.slice(-2).every(t => t.status === 'review') && next.queues.length === 0; });
  const fileState = await evaluate(`window.convoy.files(${JSON.stringify(projectID)})`);
  assert.ok(Array.isArray(fileState.changes)); assert.ok(fileState.log.length > 0);
  const telemetry = require('../src/telemetry.cjs');
  const helperOutput = path.join(directory, 'hook-output');
  const hook = require('node:child_process').spawnSync(process.execPath, [path.join(__dirname, '../src/telemetry.cjs'), helperOutput], { input: JSON.stringify({ hook_event_name: 'PermissionRequest', prompt: 'Do not persist this prompt' }), env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' }, encoding: 'utf8' });
  assert.equal(hook.status, 0, hook.stderr);
  const captured = fs.readFileSync(helperOutput + '.status', 'utf8'); assert.equal(JSON.parse(captured).state, 'waiting'); assert.doesNotMatch(captured, /persist this prompt/);
  state = await evaluate(`window.convoy.createSession(${JSON.stringify({ projectID, title: 'Missing conversation', agent: 'claude', prompt: 'Do not replay me' })})`);
  const failedSession = state.sessions.at(-1);
  await evaluate(`window.convoy.start(${JSON.stringify(failedSession.id)})`);
  await until(async () => (await evaluate('window.convoy.read()')).sessions.at(-1).lastExit?.resumeMissing);
  await evaluate(`window.convoy.resize(${JSON.stringify(failedSession.id)}, 120, 35)`);
  await evaluate("document.querySelector('#projects button').click(); [...document.querySelectorAll('#sessions button')].find(b => b.textContent.includes('Missing conversation')).click()");
  await until(() => evaluate("!document.querySelector('#session-recovery').hidden"));
  assert.match(await evaluate("document.querySelector('#session-exit-message').textContent"), /could not find/);
  if (process.env.CONVOY_RECOVERY_SCREENSHOT) fs.writeFileSync(process.env.CONVOY_RECOVERY_SCREENSHOT, (await win.webContents.capturePage()).toPNG());
  await evaluate("document.querySelector('#recover-session').click()");
  state = await until(async () => { const next = await evaluate('window.convoy.read()'); return next.sessions.at(-1).title.endsWith('· recovery') && next; });
  const recovered = state.sessions.at(-1);
  assert.equal(recovered.started, false);
  assert.equal(recovered.prompt, '');
  assert.equal(recovered.lastExit, undefined);
  assert.notEqual(recovered.providerID, failedSession.providerID);
  assert.equal(state.running.includes(recovered.id), false);
  assert.equal(state.sessions.find(s => s.id === failedSession.id).lastExit.resumeMissing, true);
  await until(() => evaluate("document.querySelector('#session-recovery').hidden"));
  console.log('Real app IPC, PTY lifecycle, worktrees, planning gates, Markdown export, account binding, and activity passed');
  clearTimeout(timeout);
  win.destroy();
  app.exit(0);
}).catch(error => { console.error(error); app.exit(1); });
