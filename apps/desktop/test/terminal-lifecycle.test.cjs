const { test } = require('node:test');
const assert = require('node:assert/strict');
const { resizeTerminal, exitDetails } = require('../src/terminal-lifecycle.cjs');

test('late resize ignores an exited Windows PTY before its exit callback arrives', () => {
  let calls = 0;
  const entry = { child: { resize() { calls++; throw new Error('Cannot resize a pty that has already exited'); } } };
  resizeTerminal(entry, 100, 30);
  resizeTerminal(entry, 120, 40);
  assert.equal(calls, 1);
  assert.equal(entry.exited, true);
  resizeTerminal(undefined, 100, 30);
  resizeTerminal({ stopping: true, child: { resize() { assert.fail('stopping PTY resized'); } } }, 100, 30);
});
test('resize still propagates unexpected errors and forwards live dimensions', () => {
  const sizes = [];
  resizeTerminal({ child: { resize: (...size) => sizes.push(size) } }, 120, 35);
  assert.deepEqual(sizes, [[120, 35]]);
  assert.throws(() => resizeTerminal({ child: { resize() { throw new Error('unexpected failure'); } } }, 120, 35), /unexpected failure/);
});
test('missing-conversation guidance requires the current Claude ID and a failed exit', () => {
  const session = { agent: 'claude', providerID: '8a0751fc-0589-4e8b-bce6-0e23d21214c0' };
  const message = `\x1b[31mNo conversation found with session ID: ${session.providerID}\x1b[0m\r\n`;
  assert.equal(exitDetails(session, 1, message).resumeMissing, true);
  assert.equal(exitDetails(session, 0, message).resumeMissing, false);
  assert.equal(exitDetails(session, 1, message, true).resumeMissing, false);
  assert.equal(exitDetails({ ...session, agent: 'codex' }, 1, message).resumeMissing, false);
  assert.equal(exitDetails(session, 1, message.replace(session.providerID, 'another-id')).resumeMissing, false);
});
