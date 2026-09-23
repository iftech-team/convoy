// Runs against Electron's ABI on each target OS; no provider CLI or model call.
const { app } = require('electron');
const pty = require('node-pty');
const { launchSpec } = require('../src/launch.cjs');
app.whenReady().then(() => {
  const windows = process.platform === 'win32';
  const shell = launchSpec({ agent: 'codex', providerID: '', prompt: '' }, false).file;
  const child = pty.spawn(shell, windows ? ['-NoLogo', '-NoProfile', '-Command', '$line = [Console]::ReadLine(); Write-Output ("CONVOY_OK_" + $line); exit 0'] : ['-c', 'read value; printf "CONVOY_OK_%s\\n" "$value"'], {
    cwd: process.cwd(), env: process.env, cols: 80, rows: 24, name: 'xterm-256color'
  });
  let output = '';
  const timer = setTimeout(() => { child.kill(); console.error('PTY timed out:', output); app.exit(1); }, 15000);
  child.onData(data => { output += data; });
  child.onExit(({ exitCode }) => {
    clearTimeout(timer);
    const passed = exitCode === 0 && output.includes('CONVOY_OK_probe');
    console.log(passed ? 'PTY input/output/exit passed' : `PTY failed: ${exitCode} ${output}`);
    app.exit(passed ? 0 : 1);
  });
  child.resize(120, 35);
  child.write('probe\r');
}).catch(error => { console.error(error); app.exit(1); });
