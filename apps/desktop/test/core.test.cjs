const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { Workspace } = require('../src/workspace.cjs');
const { agentArgs, launchSpec, agentEnvironment } = require('../src/launch.cjs');

function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-test-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return { dir, file: path.join(dir, 'workspace.json') };
}
test('projects and sessions survive relaunch; duplicate folders are ignored', t => {
  const { dir, file } = fixture(t);
  const workspace = new Workspace(file);
  workspace.addProject(dir);
  workspace.addProject(dir);
  assert.equal(workspace.state.projects.length, 1);
  workspace.addSession({ projectID: workspace.state.projects[0].id, title: 'Build it', agent: 'claude', prompt: 'Hello' });
  assert.deepEqual(new Workspace(file).state, workspace.state);
  assert.match(workspace.state.sessions[0].providerID, /^[0-9a-f-]{36}$/);
});
test('corrupt and future workspace files are never overwritten', t => {
  const { file } = fixture(t);
  for (const value of ['broken', JSON.stringify({ schemaVersion: 999, projects: [], sessions: [] }), JSON.stringify({ schemaVersion: 1, projects: [null], sessions: [] })]) {
    fs.writeFileSync(file, value);
    assert.throws(() => new Workspace(file));
    assert.equal(fs.readFileSync(file, 'utf8'), value);
  }
});
test('invalid mutations leave both memory and disk untouched', t => {
  const { file, dir } = fixture(t);
  const workspace = new Workspace(file);
  workspace.addProject(dir);
  const before = fs.readFileSync(file, 'utf8');
  assert.throws(() => workspace.addSession({ projectID: 'missing', title: 'Oops', agent: 'codex' }));
  assert.equal(fs.readFileSync(file, 'utf8'), before);
  assert.equal(workspace.state.sessions.length, 0);
});
test('failed writes do not commit mutations in memory', t => {
  const { dir, file } = fixture(t);
  const workspace = new Workspace(file);
  workspace.addProject(dir);
  workspace.file = dir; // A directory cannot be replaced by the temporary JSON file.
  assert.throws(() => workspace.addSession({ projectID: workspace.state.projects[0].id, title: 'Oops', agent: 'codex' }));
  assert.equal(workspace.state.sessions.length, 0);
});
test('resume uses exact provider identity and never repeats the initial prompt', () => {
  const session = { agent: 'claude', providerID: '123', prompt: 'Do something' };
  assert.deepEqual(agentArgs(session, true), ['claude', '--resume', '123']);
  assert.deepEqual(agentArgs({ ...session, agent: 'codex', providerID: '' }, true), ['codex', 'resume', '--no-alt-screen']);
  assert.deepEqual(agentArgs({ ...session, agent: 'codex' }, true), ['codex', 'resume', '123', '--no-alt-screen']);
});
test('POSIX launch preserves shell metacharacters as literal arguments', { skip: process.platform === 'win32' }, () => {
  const prompt = "quotes ' \" `echo injected` $(echo injected) ; & | %PATH%\nПривет";
  const session = { agent: 'claude', providerID: '123', prompt };
  const spec = launchSpec(session, false, 'linux');
  const script = spec.args[1].replace(/^exec /, 'printf \'%s\\0\' ');
  const result = spawnSync('/bin/bash', ['-c', script], { encoding: 'utf8' });
  assert.equal(result.status, 0);
  assert.deepEqual(result.stdout.split('\0').slice(0, -1), agentArgs(session, false));
});
test('Windows launch encodes fixed provider arguments', () => {
  const session = { agent: 'codex', providerID: '', prompt: '' };
  const spec = launchSpec(session, false, 'win32', { SystemRoot: 'C:\\Windows' });
  assert.match(spec.file, /powershell\.exe$/);
  const decoded = Buffer.from(spec.args.at(-1), 'base64').toString('utf16le');
  assert.ok(decoded.includes("'codex.exe','codex.cmd'"));
  assert.ok(decoded.includes("& $command.Source '--no-alt-screen';"));
});
test('Windows rejects prompt argv until npm shim escaping is supported', () => {
  const session = { agent: 'claude', providerID: '123', prompt: "hello ' \" $(throw 'bad') ; & | %PATH%\nПривет" };
  assert.throws(() => launchSpec(session, false, 'win32'), /directly in the Windows terminal/);
});
test('Windows PowerShell forwards provider flags', { skip: process.platform !== 'win32' }, () => {
  const session = { agent: 'claude', providerID: '123', prompt: '' };
  const spec = launchSpec(session, false, 'win32');
  const source = Buffer.from(spec.args.at(-1), 'base64').toString('utf16le');
  const call = source.slice(source.indexOf('& $command.Source')).replace('& $command.Source', '& claude');
  const instrumented = `function claude { ConvertTo-Json -Compress -InputObject @($args) }; ${call}`;
  const result = spawnSync(spec.file, ['-NoProfile', '-EncodedCommand', Buffer.from(instrumented, 'utf16le').toString('base64')], { encoding: 'utf8' });
  // JSON Unicode escapes keep this independent of the Windows console encoding.
  const parsed = JSON.parse(result.stdout);
  assert.deepEqual(parsed, ['--session-id', '123']);
});
test('agent environment removes parent conversation markers without losing login configuration', () => {
  const source = { PATH: '/bin', CLAUDECODE: '1', CLAUDE_CODE_SESSION_ID: 'parent', CLAUDE_CONFIG_DIR: '/account', CODEX_HOME: '/codex', CODEX_THREAD_ID: 'parent' };
  const env = agentEnvironment(source);
  assert.equal(env.CLAUDECODE, undefined);
  assert.equal(env.CLAUDE_CODE_SESSION_ID, undefined);
  assert.equal(env.CODEX_THREAD_ID, undefined);
  assert.equal(env.CLAUDE_CONFIG_DIR, '/account');
  assert.equal(env.CODEX_HOME, '/codex');
  assert.equal(source.CLAUDECODE, '1');
});
