const fs = require('node:fs/promises');
const path = require('node:path');
const { spawn } = require('node:child_process');
const { relative } = require('./files.cjs');
async function setup(repo, destination, sharedPaths, command) {
  const root = await fs.realpath(repo), target = await fs.realpath(destination);
  for (const name of sharedPaths) {
    relative(name);
    if (name.split(/[\\/]/).some(p => p === '.git')) throw new Error('Git metadata cannot be shared.');
    const source = await fs.realpath(path.join(root, name));
    if (!source.startsWith(root + path.sep)) throw new Error('Shared path points outside the project.');
    const output = path.join(target, name);
    let checked = target;
    for (const part of path.relative(target, path.dirname(output)).split(path.sep).filter(Boolean)) {
      checked = path.join(checked, part);
      try {
        const stat = await fs.lstat(checked);
        if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error('Shared destination contains a symlink or non-directory.');
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        await fs.mkdir(checked);
      }
    }
    const parent = await fs.realpath(path.dirname(output));
    if (parent !== target && !parent.startsWith(target + path.sep)) throw new Error('Shared destination points outside the worktree.');
    // Copies avoid Windows symlink privilege requirements and never overwrite tracked files.
    await fs.cp(source, output, { recursive: true, force: false, errorOnExist: true, dereference: false });
  }
  if (command) await new Promise((resolve, reject) => {
    const windows = process.platform === 'win32';
    const child = spawn(windows ? path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe') : '/bin/bash', windows ? ['-NoLogo', '-NoProfile', '-Command', command] : ['-lc', command], { cwd: target, windowsHide: true, stdio: 'ignore' });
    const timer = setTimeout(() => { child.kill(); reject(new Error('Worktree setup exceeded 120 seconds. Worktree has been retained.')); }, 120000);
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.on('exit', code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error(`Setup exited with code ${code}. Worktree has been retained.`)); });
  });
}
module.exports = { setup };
