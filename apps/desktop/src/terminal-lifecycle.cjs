const { plain } = require('./history.cjs');

function resizeTerminal(entry, cols, rows) {
  if (!entry || entry.stopping || entry.exited) return;
  try { entry.child.resize(cols, rows); }
  catch (error) {
    // ConPTY can exit before node-pty finishes draining output and emits onExit.
    // Only suppress this known lifecycle race; other resize failures still matter.
    if (error.message !== 'Cannot resize a pty that has already exited') throw error;
    entry.exited = true;
  }
}

function exitDetails(session, code, output, stopped = false) {
  const missing = `No conversation found with session ID: ${session.providerID}`;
  return {
    code, stopped,
    resumeMissing: !stopped && code !== 0 && session.agent === 'claude' && !!session.providerID &&
      plain(output).split(/\r?\n/).some(line => line.trim() === missing)
  };
}
module.exports = { resizeTerminal, exitDetails };
