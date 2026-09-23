const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Workspace, defaults } = require('../src/workspace.cjs');
const { History, paste, reviewBrief } = require('../src/history.cjs');
const { git, createWorktree, gitStatus } = require('../src/git.cjs');
function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-features-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, 'workspace.json');
  const workspace = new Workspace(file);
  workspace.addProject(directory);
  workspace.addSession({ projectID: workspace.state.projects[0].id, agent: 'claude', title: 'Builder', prompt: 'Fix bug' });
  return { directory, file, workspace, builder: workspace.state.sessions[0] };
}
test('schema 1 migrates without losing provider IDs or unknown data', t => {
  const { file, workspace, builder } = fixture(t);
  const legacy = { ...workspace.state, schemaVersion: 1, customField: { preserved: true } };
  delete legacy.settings; delete legacy.quickCommands;
  fs.writeFileSync(file, JSON.stringify(legacy));
  const migrated = new Workspace(file);
  assert.equal(migrated.state.schemaVersion, 3);
  assert.deepEqual(migrated.state.settings, defaults);
  assert.equal(migrated.session(builder.id).providerID, builder.providerID);
  migrated.editSession(builder.id, { notes: 'Updated' });
  assert.deepEqual(new Workspace(file).state.customField, { preserved: true });
});
test('review handoff preserves folder, branch, source link and uses a new provider identity', t => {
  const { directory, workspace, builder, file } = fixture(t);
  workspace.update(next => Object.assign(next.sessions[0], { workingDirectory: path.join(directory, 'checkout'), branch: 'task/fix', ownsWorktree: true }));
  workspace.addSession({ projectID: builder.projectID, agent: 'codex', title: 'Review', prompt: reviewBrief(builder, '\x1b[32mDone\x1b[0m'), reviewOf: builder.id });
  const review = new Workspace(file).state.sessions[1];
  assert.equal(review.reviewOf, builder.id);
  assert.equal(review.workingDirectory, path.join(directory, 'checkout'));
  assert.equal(review.branch, 'task/fix');
  assert.equal(review.ownsWorktree, undefined);
  assert.equal(review.providerID, '');
  assert.match(review.prompt, /Do not edit files/);
  assert.ok(!review.prompt.includes('\x1b'));
});
test('running sessions cannot be archived or rebound, but notes and pinning can change', t => {
  const { workspace, builder } = fixture(t);
  assert.throws(() => workspace.editSession(builder.id, { archived: true }, true), /Stop/);
  assert.throws(() => workspace.editSession(builder.id, { providerID: builder.providerID }, true), /Stop/);
  workspace.editSession(builder.id, { pinned: true, notes: 'Important', title: 'Updated' }, true);
  assert.equal(workspace.session(builder.id).pinned, true);
  assert.throws(() => workspace.editSession(builder.id, { workingDirectory: '/elsewhere' }), /Invalid/);
  workspace.editSession(builder.id, { archived: true });
  workspace.editSession(builder.id, { archived: false });
  assert.equal(workspace.session(builder.id).providerID, builder.providerID);
});
test('settings and scoped commands persist; invalid changes are atomic', t => {
  const { workspace, builder, file } = fixture(t);
  workspace.saveSettings({ ...defaults, theme: 'light', fontSize: 18 });
  workspace.saveCommand({ title: 'Check', text: 'Run the test suite', submit: false, projectID: builder.projectID });
  const saved = new Workspace(file).state;
  assert.equal(saved.settings.fontSize, 18);
  assert.equal(saved.quickCommands[0].projectID, builder.projectID);
  assert.throws(() => workspace.saveSettings({ ...defaults, scrollback: 999999 }));
  assert.throws(() => workspace.saveCommand({ title: 'Oops', text: 'text', submit: false, projectID: 'missing' }));
  assert.deepEqual(new Workspace(file).state, saved);
});
test('snapshots remain bounded plain text, and IDs cannot escape the history directory', t => {
  const { directory } = fixture(t);
  const history = new History(path.join(directory, 'history'));
  const id = '../outside';
  history.save(id, 'x'.repeat(60000) + '\x1b[31mred\x1b[0m');
  assert.equal(history.read(id).length, 48000);
  assert.ok(history.read(id).endsWith('red'));
  assert.equal(fs.existsSync(path.join(directory, 'outside')), false);
});
test('feedback paste strips control sequences and does not press Enter', () => {
  const text = 'hello\x1b[201~\r\x03\x00\nworld';
  assert.equal(paste(text), '\x1b[200~hello\nworld\x1b[201~');
  assert.equal(paste('test', true), '\x1b[200~test\x1b[201~\r');
});
test('real worktrees isolate changes, reject invalid branches, and refuse dirty removal', async t => {
  const { directory } = fixture(t);
  const repo = path.join(directory, 'repo'); fs.mkdirSync(repo);
  await git(repo, ['init']);
  await git(repo, ['-c', 'user.name=Convoy Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgSign=false', 'commit', '--allow-empty', '-m', 'initial']);
  const original = (await git(repo, ['rev-parse', '--abbrev-ref', 'HEAD'])).trim();
  const root = path.join(directory, 'worktrees');
  await assert.rejects(createWorktree(repo, root, '--detach'), /valid/);
  await assert.rejects(createWorktree(repo, root, 'bad branch'));
  const checkout = await createWorktree(repo, root, 'convoy/test');
  assert.equal((await gitStatus(checkout)).branch, 'convoy/test');
  assert.equal((await git(repo, ['rev-parse', '--abbrev-ref', 'HEAD'])).trim(), original);
  fs.writeFileSync(path.join(checkout, 'untracked.txt'), 'do not delete');
  assert.equal((await gitStatus(checkout)).changedFiles, 1);
  await assert.rejects(git(repo, ['worktree', 'remove', checkout]));
  assert.equal(fs.readFileSync(path.join(checkout, 'untracked.txt'), 'utf8'), 'do not delete');
  fs.unlinkSync(path.join(checkout, 'untracked.txt'));
  await git(repo, ['worktree', 'remove', checkout]);
  assert.equal(fs.existsSync(checkout), false);
  assert.ok((await git(repo, ['branch', '--list', 'convoy/test'])).includes('convoy/test'));
});
