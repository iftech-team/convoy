const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { createHash } = require('node:crypto');
const { git } = require('./git.cjs');
const excluded = new Set(['node_modules', 'vendor', 'target', 'dist', 'build', 'DerivedData', 'Pods', 'Carthage', 'venv', 'env', '__pycache__', 'coverage']);
const markers = new Set(['.git', 'package.json', 'Cargo.toml', 'Package.swift', 'composer.json', 'pyproject.toml', 'setup.py', 'requirements.txt', 'go.mod', 'pubspec.yaml', 'Gemfile', 'pom.xml', 'build.gradle', 'build.gradle.kts', 'CMakeLists.txt', 'mix.exs', 'deno.json', 'deno.jsonc']);
async function discover(root) {
  const found = []; let count = 0;
  async function visit(directory, depth) {
    if (++count > 3000 || depth > 10) return;
    let entries; try { entries = await fs.readdir(directory, { withFileTypes: true }); } catch { return; }
    if (entries.some(e => markers.has(e.name) || /\.(xcodeproj|sln|csproj)$/.test(e.name))) { found.push(directory); return; }
    for (const e of entries) if (e.isDirectory() && !e.name.startsWith('.') && !excluded.has(e.name)) await visit(path.join(directory, e.name), depth + 1);
  }
  await visit(root, 0); return found.length ? found : [root];
}
function relative(value) {
  if (typeof value !== 'string' || !value || value.includes('\0') || path.isAbsolute(value) || value.split(/[\\/]/).includes('..')) throw new Error('Invalid repository path.');
  return value;
}
async function preview(root, name) {
  relative(name);
  const base = await fs.realpath(root), resolved = await fs.realpath(path.join(base, name));
  if (!resolved.startsWith(base + path.sep)) throw new Error('File points outside this folder.');
  const file = await fs.open(resolved, 'r');
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size > 1024 * 1024) return 'Preview unavailable: file is not regular or exceeds 1 MB.';
    const buffer = Buffer.alloc(Math.min(stat.size + 1, 1024 * 1024 + 1));
    const { bytesRead } = await file.read(buffer, 0, buffer.length, 0);
    if (bytesRead > 1024 * 1024 || buffer.subarray(0, bytesRead).includes(0)) return 'Binary or oversized file — preview unavailable.';
    return buffer.subarray(0, bytesRead).toString('utf8');
  } finally { await file.close(); }
}
async function fileContent(root, name) {
  relative(name);
  const types = { '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp' };
  const type = types[path.extname(name).toLowerCase()];
  if (!type) return { text: await preview(root, name), markdown: /\.(md|markdown)$/i.test(name) };
  const base = await fs.realpath(root), resolved = await fs.realpath(path.join(base, name));
  if (!resolved.startsWith(base + path.sep)) throw new Error('File points outside this folder.');
  const file = await fs.open(resolved, 'r');
  try {
    const stat = await file.stat(); if (!stat.isFile() || stat.size > 1024 * 1024) return { text: 'Image preview exceeds 1 MB.' };
    const data = Buffer.alloc(stat.size + 1), { bytesRead } = await file.read(data, 0, data.length, 0);
    if (bytesRead > 1024 * 1024) return { text: 'Image preview exceeds 1 MB.' };
    return { image: `data:${type};base64,${data.subarray(0, bytesRead).toString('base64')}` };
  } finally { await file.close(); }
}
async function folderFiles(root) {
  const result = []; let visited = 0;
  async function walk(relativePath, depth) {
    if (++visited > 3000 || depth > 10 || result.length > 10000) return;
    const entries = await fs.readdir(path.join(root, relativePath), { withFileTypes: true }).catch(() => []);
    for (const entry of entries) {
      if (entry.name.startsWith('.') || excluded.has(entry.name)) continue;
      const name = path.join(relativePath, entry.name);
      if (entry.isDirectory()) await walk(name, depth + 1); else if (entry.isFile()) result.push(name);
    }
  }
  await walk('', 0); return result.sort();
}
function parseStatus(raw) {
  const records = raw.split('\0'), result = [];
  for (let i = 0; i < records.length; i++) {
    const record = records[i]; if (!record) continue;
    const x = record[0], y = record[1], name = record.slice(3);
    const original = /[RC]/.test(x + y) ? records[++i] : undefined;
    result.push({ path: name, original, index: x, worktree: y, untracked: x === '?', conflict: ['DD','AU','UD','UA','DU','AA','UU'].includes(x + y) });
  }
  return result;
}
async function snapshot(root) {
  if (!await git(root, ['rev-parse', '--is-inside-work-tree']).then(() => true, () => false)) return { branch: 'Folder', changes: [], files: await folderFiles(root), log: [], branches: [] };
  const [status, branch, files, log, branches] = await Promise.all([
    git(root, ['status', '--porcelain=v1', '-z']),
    git(root, ['symbolic-ref', '--short', '-q', 'HEAD']).catch(() => git(root, ['rev-parse', '--short', 'HEAD'])),
    git(root, ['ls-files', '-z', '--cached', '--others', '--exclude-standard']),
    git(root, ['log', '-100', '--format=%H%x00%h%x00%s%x00%an%x00%aI']).catch(() => ''),
    git(root, ['for-each-ref', '--format=%(refname:short)', 'refs/heads', 'refs/remotes'])
  ]);
  return { branch: branch.trim(), changes: parseStatus(status), files: [...new Set(files.split('\0').filter(Boolean))].sort(),
    log: log.trim().split('\n').filter(Boolean).map(line => { const [id, short, subject, author, date] = line.split('\0'); return { id, short, subject, author, date }; }), branches: branches.trim().split('\n').filter(Boolean) };
}
function revision(value) { if (typeof value !== 'string' || !/^[0-9a-f]{40,64}$/.test(value)) throw new Error('Invalid commit.'); return value; }
async function read(root, request) {
  if (request.kind === 'file') return preview(root, request.path);
  if (request.kind === 'commit') return git(root, ['show', '--format=fuller', '--no-ext-diff', '--no-textconv', revision(request.commit), '--']);
  const name = relative(request.path);
  if (request.kind === 'untracked') return preview(root, name);
  if (!['staged', 'unstaged'].includes(request.kind)) throw new Error('Invalid preview.');
  return git(root, ['diff', '--no-ext-diff', '--no-textconv', ...(request.kind === 'staged' ? ['--cached'] : []), '--', name]);
}
function hunks(text) {
  const lines = text.split('\n'), start = lines.findIndex(line => line.startsWith('@@ '));
  if (start < 0) return { header: '', patches: [] };
  const header = lines.slice(0, start).join('\n') + '\n';
  const patches = []; let current = [];
  for (const line of lines.slice(start)) {
    if (line.startsWith('@@ ') && current.length) { patches.push(current.join('\n') + '\n'); current = []; }
    current.push(line);
  }
  if (current.length) patches.push(current.join('\n').replace(/\n$/, '') + '\n');
  return { header, patches };
}
const digest = text => createHash('sha256').update(text).digest('hex');
async function mutate(root, action, value = {}) {
  const paths = () => [relative(value.path), ...(value.original ? [relative(value.original)] : [])];
  switch (action) {
    case 'discardHunk': {
      const text = await read(root, { kind: 'unstaged', path: value.path });
      if (digest(text) !== value.hash) throw new Error('The diff changed. Refresh before discarding a hunk.');
      const parsed = hunks(text);
      if (!Number.isInteger(value.hunk) || !parsed.patches[value.hunk] || /^(new file|deleted file|rename from|rename to)/m.test(text)) throw new Error('This hunk cannot be discarded separately.');
      const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'convoy-patch-'));
      try {
        const patch = path.join(temp, 'patch.diff'); await fs.writeFile(patch, parsed.header + parsed.patches[value.hunk]);
        await git(root, ['apply', '--check', '--reverse', '--', patch]);
        return await git(root, ['apply', '--reverse', '--', patch]);
      } finally { await fs.rm(temp, { recursive: true, force: true }); }
    }
    case 'stage': return git(root, ['add', '--', ...paths()]);
    case 'unstage': {
      const unborn = await git(root, ['rev-parse', '--verify', 'HEAD']).then(() => false, () => true);
      return git(root, unborn ? ['rm', '--cached', '--', ...paths()] : ['restore', '--staged', '--', ...paths()]);
    }
    case 'stageAll': return git(root, ['add', '--all']);
    case 'discard': return git(root, ['restore', '--worktree', '--', relative(value.path)]);
    case 'commit': {
      if (typeof value.message !== 'string' || !value.message.trim() || value.message.length > 10000) throw new Error('Enter a commit message.');
      return git(root, ['commit', ...(value.amend === true ? ['--amend'] : []), '-m', value.message]);
    }
    case 'fetch': return git(root, ['fetch', '--all', '--prune']);
    case 'pull': return git(root, ['pull', '--ff-only']);
    case 'push': return git(root, ['push']);
    case 'switch': case 'branch': {
      if (typeof value.branch !== 'string' || value.branch.startsWith('-')) throw new Error('Invalid branch.');
      await git(root, ['check-ref-format', '--branch', value.branch]);
      return git(root, ['switch', ...(action === 'branch' ? ['-c'] : []), value.branch]);
    }
    case 'revert': return git(root, ['revert', '--no-edit', revision(value.commit)]);
    case 'resetSoft': case 'resetMixed': return git(root, ['reset', action === 'resetSoft' ? '--soft' : '--mixed', revision(value.commit)]);
    default: throw new Error('Unknown Git action.');
  }
}
module.exports = { discover, preview, parseStatus, snapshot, read, mutate, relative, fileContent, hunks, digest };
