const fs = require('node:fs/promises');
const path = require('node:path');
const uuid = /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i;
async function head(file) {
  const handle = await fs.open(file, 'r');
  try { const data = Buffer.alloc(96000), { bytesRead } = await handle.read(data, 0, data.length, 0); return data.subarray(0, bytesRead).toString('utf8').split('\n').flatMap(line => { try { return [JSON.parse(line)]; } catch { return []; } }); } finally { await handle.close(); }
}
async function scan(agent, home, directory) {
  const root = path.join(home, agent === 'claude' ? 'projects' : 'sessions'), files = []; let visited = 0;
  async function walk(folder, depth) {
    if (++visited > 3000 || depth > 5 || files.length > 3000) return;
    for (const entry of await fs.readdir(folder, { withFileTypes: true }).catch(() => [])) {
      const file = path.join(folder, entry.name);
      if (entry.isDirectory()) await walk(file, depth + 1);
      else if (entry.isFile() && entry.name.endsWith('.jsonl')) files.push(file);
    }
  }
  if (agent === 'claude') await walk(path.join(root, directory.replace(/[^A-Za-z0-9]/g, '-')), 5); else await walk(root, 0);
  const result = [];
  for (const file of files) {
    try {
      const records = await head(file);
      const meta = records.find(r => r.type === 'session_meta')?.payload;
      const id = agent === 'claude' ? path.basename(file, '.jsonl') : meta?.id;
      if (!uuid.test(id || '') || (agent === 'codex' && (path.resolve(meta.cwd || '') !== path.resolve(directory) || meta.source?.subagent || meta.parent_thread_id))) continue;
      const user = agent === 'claude' ? records.find(r => r.type === 'user')?.message : records.find(r => r.payload?.type === 'user_message' || r.payload?.role === 'user')?.payload;
      const content = user?.message || user?.content;
      const title = (typeof content === 'string' ? content : Array.isArray(content) ? content.map(p => p.text || '').join(' ') : `${agent} ${id.slice(0, 8)}`).replace(/\s+/g, ' ').slice(0, 100);
      result.push({ agent, providerID: id, title, at: (await fs.stat(file)).mtimeMs });
    } catch { /* Skip incomplete or inaccessible provider files. */ }
  }
  return result.sort((a, b) => b.at - a.at).slice(0, 100);
}
module.exports = { scan };
