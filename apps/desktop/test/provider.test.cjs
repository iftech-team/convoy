const { test } = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { PassThrough } = require('node:stream');
const { sanitize } = require('../src/telemetry.cjs');
const { rateWindows, codexLimits, sessionSpec } = require('../src/provider.cjs');
test('telemetry stores only status and validated quota windows', () => {
  const result = sanitize({ hook_event_name: 'PreToolUse', tool_name: 'AskUserQuestion', prompt: 'secret', transcript: 'secret', rate_limits: { five_hour: { used_percentage: 42, resets_at: 500 }, seven_day: { used_percentage: 101 } } }, 100000);
  assert.equal(result.state, 'waiting'); assert.equal(result.windows.length, 1); assert.doesNotMatch(JSON.stringify(result), /secret/);
  assert.equal(sanitize({ hook_event_name: 'Stop' }).state, 'done');
  assert.deepEqual(rateWindows({ rateLimits: { primary: { usedPercent: 12, resetsAt: 5 } } }, 10), []);
  assert.equal(rateWindows({ rateLimitsByLimitId: { codex: { primary: { usedPercent: 12 } } } })[0].percent, 12);
});
test('Codex usage performs only initialization and rate-limit read', async () => {
  const requests = [];
  const spawn = () => {
    const child = new EventEmitter(); child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.kill = () => {};
    child.stdin.on('data', data => {
      const request = JSON.parse(data.toString()); requests.push(request.method);
      if (request.id === 1) queueMicrotask(() => child.stdout.write(JSON.stringify({ id: 1, result: {} }) + '\n'));
      if (request.id === 2) queueMicrotask(() => child.stdout.write(JSON.stringify({ id: 2, result: { rateLimits: { primary: { usedPercent: 25 } } } }) + '\n'));
    }); return child;
  };
  // Resolution on Windows requires an installed CLI; protocol behavior is platform independent.
  if (process.platform === 'win32') return;
  const windows = await codexLimits({}, process.cwd(), spawn);
  assert.equal(windows[0].percent, 25);
  assert.deepEqual(requests, ['initialize', 'initialized', 'account/rateLimits/read']);
});
test('enhanced launch passes model/settings before a literal prompt and never replays on resume', { skip: process.platform === 'win32' }, () => {
  const session = { agent: 'claude', providerID: 'abc', prompt: '$(touch never)', model: 'sonnet', started: false };
  const result = sessionSpec(session, '/tmp/settings file.json', { CLAUDE_CONFIG_DIR: '/tmp/account' });
  assert.match(result.args[1], /--model.*sonnet.*--settings.*settings file.json.*--.*touch never/);
  assert.doesNotMatch(sessionSpec({ ...session, started: true }, undefined, {}).args[1], /touch never/);
});
test('Windows npm resolver launches scripts directly with literal argument vectors', t => {
  const fs = require('node:fs'), os = require('node:os'), path = require('node:path');
  const { providerSpec } = require('../src/provider.cjs');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-resolver-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const directory = path.join(root, 'node_modules', '@openai', 'codex', 'bin'); fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(path.join(directory, 'codex.js'), 'process.stdout.write(JSON.stringify(process.argv.slice(2)))');
  const args = ['--model', 'gpt-example', '--', 'spaces "quotes" & %VAR% $(command) `literal`\nsecond line'];
  const spec = providerSpec('codex', args, { PATH: root }, 'win32');
  const result = require('node:child_process').spawnSync(spec.file, spec.args, { encoding: 'utf8', env: { ...process.env, ELECTRON_RUN_AS_NODE: '1' } });
  assert.equal(result.status, 0, result.stderr); assert.deepEqual(JSON.parse(result.stdout), args);
});
test('provider history matches folders and excludes Codex subagents', async t => {
  const fs = require('node:fs'), os = require('node:os'), path = require('node:path');
  const { scan } = require('../src/transcripts.cjs');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-history-')), project = path.join(root, 'project');
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const dir = path.join(root, 'sessions', '2026'); fs.mkdirSync(dir, { recursive: true });
  const id = '11111111-1111-4111-8111-111111111111';
  const payload = { id, cwd: project };
  fs.writeFileSync(path.join(dir, 'rollout-a.jsonl'), JSON.stringify({ type: 'session_meta', payload }) + '\n' + JSON.stringify({ type: 'event_msg', payload: { type: 'user_message', message: 'Fix parser' } }));
  fs.writeFileSync(path.join(dir, 'rollout-b.jsonl'), JSON.stringify({ type: 'session_meta', payload: { ...payload, parent_thread_id: 'parent' } }));
  assert.equal((await scan('codex', root, project)).length, 1);
  assert.equal((await scan('codex', root, project))[0].title, 'Fix parser');
  assert.equal((await scan('codex', root, path.join(root, 'other'))).length, 0);
});
