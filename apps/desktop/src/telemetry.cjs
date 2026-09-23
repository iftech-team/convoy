const fs = require('node:fs');
const path = require('node:path');
const { createHash, randomUUID } = require('node:crypto');
const key = value => createHash('sha256').update(value).digest('hex');
function sanitize(input, now = Date.now()) {
  const event = input.hook_event_name;
  const states = { SessionStart: 'idle', UserPromptSubmit: 'working', PreToolUse: input.tool_name === 'AskUserQuestion' ? 'waiting' : 'working', PostToolUse: 'working', PermissionRequest: 'waiting', Notification: input.notification_type === 'idle_prompt' ? 'done' : 'waiting', Stop: 'done', SessionEnd: 'ended' };
  const windows = [];
  for (const name of ['five_hour', 'seven_day']) {
    const value = input.rate_limits?.[name];
    if (value && Number.isFinite(value.used_percentage) && value.used_percentage >= 0 && value.used_percentage <= 100 && (value.resets_at == null || (Number.isFinite(value.resets_at) && value.resets_at > now / 1000))) windows.push({ name, percent: value.used_percentage, resetsAt: value.resets_at });
  }
  return { at: now, ...(states[event] ? { state: states[event], event } : {}), ...(input.rate_limits ? { windows } : {}) };
}
function configuration(root, session, executable = process.execPath, usage = false) {
  fs.mkdirSync(root, { recursive: true });
  const output = path.join(root, key(session.id));
  const helper = path.join(__dirname.replace(/app\.asar(?=[\\/])/, 'app.asar.unpacked'), 'telemetry.cjs');
  // Exec-form hooks have no shell quoting; Electron's node mode is inherited from the CLI environment.
  const command = { type: 'command', command: executable, args: [helper, output], timeout: 5 };
  const hooks = Object.fromEntries(['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PermissionRequest', 'Notification', 'Stop', 'SessionEnd'].map(event => [event, [{ matcher: '', hooks: [command] }]]));
  const quote = s => "'" + s.replaceAll("'", "'\"'\"'") + "'";
  const psQuote = s => "'" + s.replaceAll("'", "''") + "'";
  const statusCommand = process.platform === 'win32'
    ? 'powershell.exe -NoLogo -NoProfile -EncodedCommand ' + Buffer.from("$env:ELECTRON_RUN_AS_NODE='1'; & " + [executable, helper, output].map(psQuote).join(' '), 'utf16le').toString('base64')
    : 'ELECTRON_RUN_AS_NODE=1 ' + [executable, helper, output].map(quote).join(' ');
  const file = output + '.settings.json';
  fs.writeFileSync(file, JSON.stringify({ hooks, ...(usage ? { statusLine: { type: 'command', command: statusCommand } } : {}) }), { mode: 0o600 });
  // Old Stop data must not mark a fresh launch complete.
  for (const suffix of ['.status', '.usage']) fs.rmSync(output + suffix, { force: true });
  return file;
}
function read(root, id, suffix) {
  try { const file = path.join(root, key(id) + suffix); if (fs.statSync(file).size > 16000) return; return JSON.parse(fs.readFileSync(file, 'utf8')); } catch { return; }
}
if (require.main === module) {
  let input = '', size = 0;
  process.stdin.on('data', chunk => { size += chunk.length; if (size > 2 * 1024 * 1024) process.exit(0); input += chunk; });
  process.stdin.on('end', () => {
    try {
      const value = sanitize(JSON.parse(input));
      const suffix = value.state ? '.status' : '.usage';
      if (!value.state && !value.windows) return;
      const file = process.argv[2] + suffix, temporary = file + '.' + randomUUID();
      fs.writeFileSync(temporary, JSON.stringify(value), { mode: 0o600 }); fs.renameSync(temporary, file);
      if (suffix === '.usage') process.stdout.write(value.windows.map(w => `${w.name}: ${Math.round(w.percent)}%`).join(' · '));
    } catch { /* Hooks must never block the agent on malformed or unavailable telemetry. */ }
  });
}
module.exports = { sanitize, configuration, read };
