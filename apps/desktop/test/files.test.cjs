const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { git } = require('../src/git.cjs');
const { discover, preview, parseStatus, snapshot, mutate, read } = require('../src/files.cjs');
function fixture(t) { const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-files-')); t.after(() => fs.rmSync(dir, { recursive: true, force: true })); return dir; }
test('discovery stops at project boundaries and skips dependencies and symlinks', async t => {
  const dir = fixture(t);
  for (const name of ['group/app', 'node_modules/ignored']) { fs.mkdirSync(path.join(dir, name), { recursive: true }); fs.writeFileSync(path.join(dir, name, 'package.json'), '{}'); }
  assert.deepEqual(await discover(dir), [path.join(dir, 'group/app')]);
});
test('preview rejects traversal, external symlinks, binary and oversized files', async t => {
  const dir = fixture(t); fs.writeFileSync(path.join(dir, 'text'), 'hello'); fs.writeFileSync(path.join(dir, 'binary'), Buffer.from([0, 1])); fs.writeFileSync(path.join(dir, 'large'), Buffer.alloc(1024 * 1024 + 1, 65));
  assert.equal(await preview(dir, 'text'), 'hello');
  await assert.rejects(preview(dir, '../outside'));
  assert.match(await preview(dir, 'binary'), /Binary/); assert.match(await preview(dir, 'large'), /exceeds/);
  if (process.platform !== 'win32') { fs.symlinkSync(os.tmpdir(), path.join(dir, 'link')); await assert.rejects(preview(dir, 'link')); }
});
test('Git panel handles unborn repositories, spaced paths, stage, commit, diffs and rename records', async t => {
  const dir = fixture(t); await git(dir, ['init']); await git(dir, ['config', 'user.name', 'Test']); await git(dir, ['config', 'user.email', 'test@example.invalid']);
  const name = 'space name.txt'; fs.writeFileSync(path.join(dir, name), 'one\n');
  assert.equal((await snapshot(dir)).changes[0].untracked, true);
  await mutate(dir, 'stage', { path: name }); await mutate(dir, 'unstage', { path: name });
  assert.equal((await snapshot(dir)).changes[0].untracked, true);
  await mutate(dir, 'stageAll'); await mutate(dir, 'commit', { message: 'Initial commit' });
  fs.writeFileSync(path.join(dir, name), 'two\n'); assert.match(await read(dir, { kind: 'unstaged', path: name }), /\+two/);
  await mutate(dir, 'discard', { path: name }); assert.equal(fs.readFileSync(path.join(dir, name), 'utf8'), 'one\n');
  assert.equal((await snapshot(dir)).log.length, 1);
  const parsed = parseStatus('R  new name\0old name\0?? other\0'); assert.equal(parsed[0].original, 'old name'); assert.equal(parsed.length, 2);
  await assert.rejects(mutate(dir, 'resetMixed', { commit: '--hard' }));
});
test('discard hunk rejects stale diffs and preserves other hunks', async t => {
  const { digest } = require('../src/files.cjs');
  const dir = fixture(t); await git(dir, ['init']); await git(dir, ['config', 'user.name', 'Test']); await git(dir, ['config', 'user.email', 'test@example.invalid']);
  const before = Array.from({ length: 30 }, (_, i) => `line ${i}`).join('\n') + '\n';
  fs.writeFileSync(path.join(dir, 'text'), before); await mutate(dir, 'stageAll'); await mutate(dir, 'commit', { message: 'Initial' });
  fs.writeFileSync(path.join(dir, 'text'), before.replace('line 1\n', 'first edit\n').replace('line 25\n', 'second edit\n'));
  const diff = await read(dir, { kind: 'unstaged', path: 'text' });
  await assert.rejects(mutate(dir, 'discardHunk', { path: 'text', hunk: 0, hash: 'stale' }), /changed/);
  await mutate(dir, 'discardHunk', { path: 'text', hunk: 0, hash: digest(diff) });
  assert.equal(fs.readFileSync(path.join(dir, 'text'), 'utf8'), before.replace('line 25\n', 'second edit\n'));
});

test('staging a path treats Git wildcard characters literally', async t => {
  const dir = fixture(t); await git(dir, ['init']);
  fs.writeFileSync(path.join(dir, '[ab].txt'), 'literal'); fs.writeFileSync(path.join(dir, 'a.txt'), 'other');
  await mutate(dir, 'stage', { path: '[ab].txt' });
  const status = await snapshot(dir);
  assert.equal(status.changes.find(f => f.path === '[ab].txt').index, 'A');
  assert.equal(status.changes.find(f => f.path === 'a.txt').untracked, true);
});
test('shared-file setup never follows destination symlinks or overwrites existing files', async t => {
  const { setup } = require('../src/worktree-setup.cjs');
  const dir = fixture(t), repo = path.join(dir, 'repo'), target = path.join(dir, 'target'), outside = path.join(dir, 'outside');
  for (const folder of [repo, target, outside]) fs.mkdirSync(folder);
  fs.writeFileSync(path.join(repo, 'settings'), 'source'); fs.writeFileSync(path.join(target, 'settings'), 'keep');
  await assert.rejects(setup(repo, target, ['settings'], '')); assert.equal(fs.readFileSync(path.join(target, 'settings'), 'utf8'), 'keep');
  if (process.platform !== 'win32') {
    fs.mkdirSync(path.join(repo, 'linked', 'nested'), { recursive: true }); fs.writeFileSync(path.join(repo, 'linked', 'nested', 'file'), 'source');
    fs.symlinkSync(outside, path.join(target, 'linked'));
    await assert.rejects(setup(repo, target, ['linked/nested/file'], ''));
    assert.deepEqual(fs.readdirSync(outside), []);
  }
});
