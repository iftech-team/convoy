const path = require('node:path');

function agentArgs(session, resume) {
  const args = session.agent === 'claude'
    ? ['claude', ...(resume ? ['--resume', session.providerID] : ['--session-id', session.providerID])]
    : ['codex', ...(resume ? ['resume', ...(session.providerID ? [session.providerID] : [])] : []), '--no-alt-screen'];
  if (!resume && session.prompt) args.push('--', session.prompt);
  return args;
}

function launchSpec(session, resume, platform = process.platform, env = process.env, bindings = {}) {
  if (bindings._enhanced) return require('./provider.cjs').sessionSpec({ ...session, started: resume }, bindings._settingsFile, env);
  const args = agentArgs(session, resume);
  if (platform === 'win32') {
    // PowerShell 5.1 and npm's .cmd shims do not preserve arbitrary native argv.
    // Until a native-executable resolver exists, enter Windows prompts in the PTY.
    if (!resume && session.prompt) throw new Error('Enter the first message directly in the Windows terminal.');
    // Only fixed flags and a validated UUID reach PowerShell/native npm shims.
    const quote = value => "'" + value.replaceAll("'", "''") + "'";
    const command = args[0];
    const script = `$command = Get-Command '${command}.exe','${command}.cmd' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1; ` +
      `if (!$command) { Write-Error '${command} was not found on PATH. Install it and restart Convoy.'; exit 127 }; ` +
      '& $command.Source ' + args.slice(1).map(quote).join(' ') + '; if ($null -ne $LASTEXITCODE) { exit $LASTEXITCODE } else { exit 1 }';
    return { file: path.win32.join(env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
      args: ['-NoLogo', '-NoProfile', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')] };
  }
  const quote = value => "'" + value.replaceAll("'", "'\"'\"'") + "'";
  // Use a known POSIX shell; $SHELL may be fish and cannot execute POSIX syntax.
  const bound = ['CLAUDE_CONFIG_DIR', 'CODEX_HOME'].filter(key => bindings[key]).map(key => quote(`${key}=${bindings[key]}`));
  return { file: '/bin/bash', args: ['-ilc', 'exec ' + (bound.length ? 'env ' + bound.join(' ') + ' ' : '') + args.map(quote).join(' ')] };
}

function agentEnvironment(source = process.env) {
  const env = { ...source, TERM: 'xterm-256color', COLORTERM: 'truecolor' };
  for (const key of Object.keys(env)) {
    if ((key.startsWith('CLAUDE') && key !== 'CLAUDE_CONFIG_DIR') || key === 'CODEX_THREAD_ID') delete env[key];
  }
  env.CLAUDE_CODE_FORCE_SESSION_PERSISTENCE = '1';
  return env;
}
module.exports = { launchSpec, agentArgs, agentEnvironment };
