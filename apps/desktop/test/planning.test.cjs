const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Workspace } = require('../src/workspace.cjs');
const { markdown } = require('../src/planning.cjs');
const { accountEnvironment } = require('../src/accounts.cjs');
const { launchSpec } = require('../src/launch.cjs');
function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-planning-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, 'workspace.json');
  const workspace = new Workspace(file); workspace.addProject(directory);
  const projectID = workspace.state.projects[0].id;
  workspace.saveSpec({ projectID, title: 'Login', acceptance: 'Keyboard navigation works' });
  const specID = workspace.state.specs[0].id;
  workspace.saveTask({ projectID, specID, title: 'Keyboard support', agent: 'codex' });
  return { directory, file, workspace, projectID, specID, taskID: workspace.state.tasks[0].id };
}
test('spec approval gates task preparation and edits invalidate completed work', t => {
  const { workspace, specID, taskID } = fixture(t);
  assert.throws(() => workspace.prepareTask(taskID), /Approve/);
  assert.throws(() => workspace.approveSpec(specID, 99), /changed/);
  workspace.approveSpec(specID, 1); workspace.prepareTask(taskID);
  const originalID = workspace.state.tasks[0].sessionID;
  workspace.prepareTask(taskID); // Repeated preparation does not fork a fresh conversation.
  assert.equal(workspace.state.tasks[0].sessionID, originalID);
  workspace.setTaskStatus(taskID, 'done');
  workspace.saveSpec({ ...workspace.state.specs[0], acceptance: 'Keyboard and screen reader navigation work' });
  assert.equal(workspace.state.specs[0].revision, 2);
  assert.equal(workspace.state.specs[0].approvedRevision, undefined);
  assert.equal(workspace.state.tasks[0].status, 'changes');
  assert.throws(() => workspace.setTaskStatus(taskID, 'done'), /Approve/);
  workspace.approveSpec(specID, 2); workspace.prepareTask(taskID);
  assert.notEqual(workspace.state.tasks[0].sessionID, originalID);
  assert.equal(workspace.state.sessions.length, 2);
  assert.match(workspace.state.sessions[1].prompt, /screen reader/);
});
test('unchanged specs retain approval and task edits preserve previous session history', t => {
  const { workspace, specID, taskID } = fixture(t);
  workspace.approveSpec(specID, 1);
  workspace.saveSpec(workspace.state.specs[0]);
  assert.equal(workspace.state.specs[0].approvedRevision, 1);
  workspace.prepareTask(taskID);
  const oldSessionID = workspace.state.tasks[0].sessionID;
  workspace.saveTask({ ...workspace.state.tasks[0], details: 'Support Tab and Escape' });
  assert.equal(workspace.state.tasks[0].sessionID, undefined);
  assert.ok(workspace.session(oldSessionID));
  workspace.prepareTask(taskID);
  assert.notEqual(workspace.state.tasks[0].sessionID, oldSessionID);
});
test('running tasks reject edits, and relaunch marks them interrupted without restarting', t => {
  const { workspace, file, specID, taskID } = fixture(t);
  workspace.approveSpec(specID, 1); workspace.prepareTask(taskID);
  workspace.update(next => { next.tasks[0].status = 'building'; });
  assert.throws(() => workspace.saveTask({ ...workspace.state.tasks[0], title: 'Changed' }), /Stop/);
  assert.throws(() => workspace.setTaskStatus(taskID, 'done'), /Stop/);
  const restored = new Workspace(file);
  assert.equal(restored.state.tasks[0].status, 'failed');
  assert.match(restored.state.tasks[0].lastError, /closed/);
  assert.equal(restored.state.tasks[0].sessionID, workspace.state.tasks[0].sessionID);
});
test('markdown export includes acceptance, revision, tasks and findings', t => {
  const { workspace, specID } = fixture(t);
  workspace.approveSpec(specID, 1);
  workspace.saveTask({ ...workspace.state.tasks[0], findings: 'Checked with keyboard only.' });
  const output = markdown(workspace.state.specs[0], workspace.state.tasks);
  assert.match(output, /Revision: 1 · Approved/);
  assert.match(output, /Keyboard navigation works/);
  assert.match(output, /Checked with keyboard only/);
});
test('account profiles use separate homes, reject provider mismatches, and keep resumed homes stable', t => {
  const { workspace, directory, projectID } = fixture(t);
  workspace.addProfile('Work / ../ account', 'codex');
  const profileID = workspace.state.profiles[0].id;
  assert.throws(() => workspace.addSession({ projectID, agent: 'claude', title: 'Wrong account', profileID }), /account/);
  workspace.addSession({ projectID, agent: 'codex', title: 'Work', profileID });
  const session = workspace.state.sessions.at(-1);
  const bound = accountEnvironment(session, workspace.state.profiles, directory, { OPENAI_API_KEY: 'not-forwarded', PATH: '/bin' });
  assert.equal(path.dirname(bound.home), directory);
  assert.equal(bound.env.CODEX_HOME, bound.home);
  assert.equal(bound.env.OPENAI_API_KEY, undefined);
  const resumed = accountEnvironment({ ...session, agentHome: bound.home }, workspace.state.profiles, path.join(directory, 'changed'), { CODEX_HOME: '/other' });
  assert.equal(resumed.home, bound.home);
  assert.throws(() => accountEnvironment(session, [], directory), /missing/);
  const launch = launchSpec({ ...session, agentHome: bound.home }, true, 'linux', {}, { CODEX_HOME: bound.home });
  assert.ok(launch.args[1].startsWith('exec env '));
  assert.ok(launch.args[1].includes(`CODEX_HOME=${bound.home}`));
});
test('activity stays bounded and persisted without storing credentials', t => {
  const { workspace, file, projectID } = fixture(t);
  workspace.addSession({ projectID, title: 'Activity', agent: 'codex' });
  for (let i = 0; i < 205; i++) workspace.record('exited', workspace.state.sessions[0], `Exit ${i}`);
  const restored = new Workspace(file);
  assert.equal(restored.state.activity.length, 200);
  assert.equal(restored.state.activity[0].detail, 'Exit 204');
});

test('publication and review choices are explicit and changing them invalidates the old brief', t => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'convoy-modes-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const workspace = new Workspace(path.join(directory, 'state.json')); workspace.addProject(directory);
  const projectID = workspace.state.projects[0].id;
  workspace.saveTask({ projectID, title: 'Publish safely', agent: 'claude', mode: 'pr', autoReview: true });
  const task = workspace.state.tasks[0]; workspace.prepareTask(task.id);
  assert.match(workspace.state.sessions[0].prompt, /pull request/);
  workspace.saveTask({ ...task, mode: 'none' }); assert.equal(workspace.state.tasks[0].sessionID, undefined);
  workspace.prepareTask(task.id); assert.match(workspace.state.sessions.at(-1).prompt, /Do not publish/);
});
