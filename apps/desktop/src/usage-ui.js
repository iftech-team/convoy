export function installUsage({ $, api, state, selected }) {
  let generation = 0;
  const names = { five_hour: '5-hour limit', seven_day: 'Weekly limit', primary: 'Primary limit', secondary: 'Secondary limit' };
  async function refresh() {
    const token = ++generation;
    const session = state().sessions.find(s => s.id === $('usage-session').value);
    $('usage-value').replaceChildren(); $('usage-updated').textContent = '';
    $('usage-refresh').disabled = !session;
    $('usage-account').textContent = '';
    if (!session) { $('usage-value').textContent = 'Create or select a session to view its account usage.'; return; }
    const account = state().profiles.find(p => p.id === session.profileID)?.label || 'System account';
    $('usage-account').textContent = `${session.agent === 'claude' ? 'Claude Code' : 'Codex'} · ${account}`;
    $('usage-value').textContent = 'Loading usage…'; $('usage-refresh').disabled = true;
    try {
      const result = await api.usage(session.id);
      if (token !== generation || !$('usage-dialog').open) return;
      const windows = result.windows.filter(w => Number.isFinite(w.percent) && w.percent >= 0 && w.percent <= 100);
      $('usage-value').replaceChildren();
      if (!windows.length) $('usage-value').textContent = session.agent === 'claude'
        ? 'No usage reported yet. Enable Claude usage status line in Settings, then start or resume this session. Usage appears when Claude sends a supported update.'
        : 'No usage reported by Codex. Check that this session’s account is signed in and try Refresh.';
      for (const w of windows) {
        const row = document.createElement('section'); row.className = 'usage-window';
        const label = document.createElement('strong'); label.textContent = `${names[w.name] || w.name}: ${Math.round(w.percent)}% used`;
        const meter = document.createElement('progress'); meter.max = 100; meter.value = w.percent; meter.setAttribute('aria-label', label.textContent);
        const reset = document.createElement('small'); reset.textContent = w.resetsAt ? `Resets ${new Date(w.resetsAt * 1000).toLocaleString()}` : 'Reset time unavailable';
        row.append(label, meter, reset); $('usage-value').append(row);
      }
      $('usage-updated').textContent = result.at ? `Last reported ${new Date(result.at).toLocaleString()}${Date.now() - result.at > 300000 ? ' · cached' : ''}` : '';
      $('limits-summary').textContent = windows.length ? `${session.agent === 'claude' ? 'Claude' : 'Codex'} · ${windows.map(w => `${names[w.name] || w.name} ${Math.round(w.percent)}%`).join(' · ')} · last checked ${new Date().toLocaleTimeString()}` : 'Usage unavailable';
    } catch (error) {
      if (token === generation && $('usage-dialog').open) $('usage-value').textContent = `Could not read usage: ${error.message}`;
    } finally { if (token === generation) $('usage-refresh').disabled = false; }
  }
  function open(id = selected()) {
    $('usage-session').replaceChildren(...state().sessions.filter(s => !s.archived || s.id === id).map(s => {
      const option = document.createElement('option'); option.value = s.id;
      option.textContent = `${state().projects.find(p => p.id === s.projectID)?.title || 'Project'} / ${s.title}`;
      return option;
    }));
    if (id) $('usage-session').value = id;
    $('usage-dialog').showModal(); refresh();
  }
  $('limits-open').onclick = () => open();
  $('usage-refresh').onclick = refresh; $('usage-session').onchange = refresh;
  $('usage-dialog').addEventListener('close', () => { if (!$('usage-dialog').open) generation++; });
  return open;
}
