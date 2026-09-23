const path = require('node:path');
const os = require('node:os');
const { createHash } = require('node:crypto');
const { agentEnvironment } = require('./launch.cjs');
function accountEnvironment(session, profiles, root, source = process.env) {
  const key = session.agent === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
  const profile = session.profileID && profiles.find(p => p.id === session.profileID && p.agent === session.agent);
  if (session.profileID && !profile) throw new Error('Session account profile is missing.');
  const home = session.agentHome || (profile
    ? path.join(root, createHash('sha256').update(profile.id).digest('hex'))
    : path.resolve(source[key] || path.join(os.homedir(), session.agent === 'claude' ? '.claude' : '.codex')));
  const env = agentEnvironment(source);
  env[key] = home;
  if (profile) for (const name of ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'OPENAI_API_KEY', 'CODEX_API_KEY']) delete env[name];
  return { home, env };
}
module.exports = { accountEnvironment };
