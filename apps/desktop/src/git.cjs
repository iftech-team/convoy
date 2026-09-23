const { execFile } = require('node:child_process');
const { promisify } = require('node:util');
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const execute = promisify(execFile);
async function git(directory, args) {
  // Do not inherit a parent agent's repository override variables.
  const env = { ...process.env, GIT_OPTIONAL_LOCKS: '0', GIT_TERMINAL_PROMPT: '0' };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES']) delete env[key];
  try {
    const { stdout } = await execute('git', ['--literal-pathspecs', '-C', directory, ...args], { env, windowsHide: true, timeout: 60000, maxBuffer: 2 * 1024 * 1024 });
    return stdout;
  } catch (error) { throw new Error(error.stderr?.trim() || error.message); }
}
async function createWorktree(repo, root, branch) {
  if (typeof branch !== 'string' || !branch || branch.length > 150 || branch.startsWith('-')) throw new Error('Enter a valid new branch name.');
  await git(repo, ['check-ref-format', '--branch', branch]);
  await git(repo, ['rev-parse', '--verify', 'HEAD']);
  fs.mkdirSync(root, { recursive: true });
  const directory = path.join(root, randomUUID());
  await git(repo, ['worktree', 'add', '--no-track', '-b', branch, directory, 'HEAD']);
  return directory;
}
async function gitStatus(directory) {
  const branch = (await git(directory, ['rev-parse', '--abbrev-ref', 'HEAD'])).trim();
  const changes = await git(directory, ['status', '--porcelain=v1', '-z']);
  // -z rename records have an additional pathname; status text is displayed, not parsed as commands.
  const records = changes.split('\0');
  let count = 0;
  for (let i = 0; i < records.length; i++) {
    if (!records[i]) continue;
    count++;
    if (/^[RC]|^.[RC]/.test(records[i])) i++;
  }
  return { branch, changedFiles: count };
}
module.exports = { git, createWorktree, gitStatus };
