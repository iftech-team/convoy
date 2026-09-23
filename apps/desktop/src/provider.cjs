const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { agentArgs } = require('./launch.cjs');
const quote = value => "'" + value.replaceAll("'", "'\"'\"'") + "'";
function windowsExecutable(agent, env = process.env) {
  const key = Object.keys(env).find(k => k.toLowerCase() === 'path');
  const dirs = (env[key] || '').split(';').filter(Boolean);
  for (const directory of dirs) {
    const exe = path.join(directory, `${agent}.exe`);
    if (fs.existsSync(exe)) return { file: exe, prefix: [] };
    const module = agent === 'claude' ? '@anthropic-ai/claude-code/cli.js' : '@openai/codex/bin/codex.js';
    for (const root of [path.join(directory, 'node_modules'), path.resolve(directory, '..')]) {
      const script = path.join(root, module);
      if (fs.existsSync(script)) return { file: process.execPath, prefix: [script] };
    }
  }
  throw new Error(`${agent} was not found. Install the native CLI or its standard npm package and restart Convoy.`);
}
function providerSpec(agent, args, env, platform = process.platform) {
  if (platform === 'win32') { const resolved = windowsExecutable(agent, env); return { file: resolved.file, args: [...resolved.prefix, ...args] }; }
  const bindings = ['CLAUDE_CONFIG_DIR', 'CODEX_HOME', 'ELECTRON_RUN_AS_NODE'].filter(k => env[k]).map(k => quote(`${k}=${env[k]}`));
  return { file: '/bin/bash', args: ['-ilc', `exec env ${bindings.join(' ')} ${[agent, ...args].map(quote).join(' ')}`] };
}
function sessionSpec(session, settingsFile, env) {
  const args = agentArgs(session, session.started).slice(1);
  // Insert flags before the explicit positional prompt separator.
  const flags = [...(session.model ? ['--model', session.model] : []), ...(session.agent === 'claude' && settingsFile ? ['--settings', settingsFile] : [])];
  const separator = args.indexOf('--'); args.splice(separator < 0 ? args.length : separator, 0, ...flags);
  return providerSpec(session.agent, args, env);
}
function rateWindows(result, now = Date.now() / 1000) {
  const buckets = result.rateLimitsByLimitId || (result.rateLimits ? { codex: result.rateLimits } : {});
  const windows = [];
  for (const [name, bucket] of Object.entries(buckets)) for (const key of ['primary', 'secondary']) {
    const value = bucket?.[key];
    if (!value || !Number.isFinite(value.usedPercent) || value.usedPercent < 0 || value.usedPercent > 100 || (value.resetsAt != null && (!Number.isFinite(value.resetsAt) || value.resetsAt <= now))) continue;
    windows.push({ name: `${name} ${key}`, percent: value.usedPercent, minutes: value.windowDurationMins, resetsAt: value.resetsAt });
  }
  return windows;
}
function codexLimits(env, cwd, spawnProcess = spawn) {
  return new Promise((resolve, reject) => {
    const spec = providerSpec('codex', ['app-server', '--listen', 'stdio://'], env);
    const child = spawnProcess(spec.file, spec.args, { env, cwd, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    let buffer = '', total = 0, finished = false;
    const finish = (error, value) => { if (finished) return; finished = true; clearTimeout(timeout); child.stdin.end(); child.kill(); error ? reject(error) : resolve(value); };
    const timeout = setTimeout(() => finish(new Error('Codex usage request timed out.')), 25000);
    const send = value => child.stdin.write(JSON.stringify(value) + '\n');
    child.on('error', error => finish(error)); child.on('exit', () => finish(new Error('Codex could not read usage. Check installation and sign-in.')));
    child.stdin.on('error', error => finish(error));
    child.stdout.on('data', chunk => {
      total += chunk.length; if (total > 2 * 1024 * 1024) return finish(new Error('Usage response exceeded the size limit.'));
      buffer += chunk.toString();
      while (buffer.includes('\n') && !finished) {
        const index = buffer.indexOf('\n'), line = buffer.slice(0, index); buffer = buffer.slice(index + 1);
        let message; try { message = JSON.parse(line); } catch { continue; }
        if (![1, 2].includes(message.id)) continue;
        if (message.error) return finish(new Error('Usage unavailable for this account. Check the CLI sign-in.'));
        if (message.id === 1) { send({ method: 'initialized' }); send({ id: 2, method: 'account/rateLimits/read' }); }
        if (message.id === 2) return finish(null, rateWindows(message.result || {}));
      }
    });
    send({ id: 1, method: 'initialize', params: { clientInfo: { name: 'convoy', version: '0.1.0' } } });
  });
}
module.exports = { windowsExecutable, providerSpec, sessionSpec, rateWindows, codexLimits };
