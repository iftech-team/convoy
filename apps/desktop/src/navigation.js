import { icon, agentIcon } from './icons.js';

export function installNavigation({ $, api, perform, update, state, openSession, project, selectProject, extra = {} }) {
  const defaults = { palette: 'mod+shift+p', newSession: 'mod+n', files: 'mod+shift+g', next: 'mod+alt+arrowright', previous: 'mod+alt+arrowleft', settings: 'mod+,', search: 'mod+k', sidebar: 'mod+b', home: 'mod+shift+h', newTask: 'mod+shift+n' };
  const labels = { newSession: 'New session', files: 'Files & Changes', settings: 'Settings…', search: 'Find project or session', next: 'Next session', previous: 'Previous session', sidebar: 'Toggle sidebar', home: 'Home', newTask: 'New task' };
  const glyphs = { newSession: 'plus', files: 'sidebarRight', settings: 'gear', search: 'search', next: 'chevronRight', previous: 'chevronRight', sidebar: 'sidebar', home: 'house', newTask: 'checklist' };
  const actions = {
    palette: () => { $('palette-dialog').showModal(); $('palette-search').value = ''; draw(); $('palette-search').focus(); },
    newSession: () => $('new-session').click(), files: () => $('files-open').click(), settings: () => $('settings').click(), search: () => $('search').focus(),
    next: () => navigate(1), previous: () => navigate(-1), sidebar: () => extra.sidebar?.(), home: () => extra.home?.(),
    newTask: () => { if (!$('planning').disabled) { $('planning').click(); $('new-task').click(); } }
  };
  function navigate(direction) {
    const sessions = state().sessions.filter(s => s.projectID === project().projectID && !s.archived);
    if (!sessions.length) return;
    const index = sessions.findIndex(s => s.id === project().sessionID);
    openSession(sessions[(index + direction + sessions.length) % sessions.length].id);
  }
  const keyLabel = binding => binding.replace('mod', navigator.platform.startsWith('Mac') ? '⌘' : 'Ctrl').replace('shift', '⇧').replace('alt', '⌥').replace(/arrow(\w+)/, '$1').replace(/\+/g, '').toUpperCase();
  let highlighted = 0;
  function draw() {
    const query = $('palette-search').value.toLowerCase();
    const bindings = { ...defaults, ...state().settings.shortcuts };
    const running = new Set(state().running);
    const entries = [
      ...state().sessions.filter(s => !s.archived).map(s => ({ label: s.title, subtitle: `${state().projects.find(p => p.id === s.projectID)?.title || 'Project'} · ${s.agent === 'claude' ? 'Claude Code' : 'Codex'}`, kind: 'Session', agent: s.agent, running: running.has(s.id), run: () => openSession(s.id) })),
      ...state().projects.map(p => ({ label: p.title, subtitle: p.path, kind: 'Project', glyph: 'folder', run: () => selectProject(p.id) })),
      ...Object.keys(labels).map(key => ({ label: labels[key], kind: 'Action', glyph: glyphs[key], shortcut: keyLabel(bindings[key] || ''), run: actions[key] }))
    ];
    const matches = entries.filter(e => !query || `${e.kind}: ${e.label} ${e.subtitle || ''}`.toLowerCase().includes(query));
    const ordered = query ? matches : [...matches.filter(e => e.running), ...matches.filter(e => !e.running && e.kind === 'Session').slice(0, 6), ...matches.filter(e => e.kind !== 'Session')];
    highlighted = 0;
    $('palette-list').replaceChildren(...ordered.map((entry, index) => {
      const node = document.createElement('button'); node.type = 'button';
      node.append(entry.agent ? agentIcon(entry.agent, 13) : icon(entry.glyph || 'terminal', 13));
      const body = document.createElement('span'); body.className = 'tab-text';
      const title = document.createElement('span'); title.className = 'tab-title'; title.textContent = entry.label;
      body.append(title);
      if (entry.subtitle) { const sub = document.createElement('span'); sub.className = 'tab-project'; sub.textContent = entry.subtitle; body.append(sub); }
      node.append(body);
      if (entry.running) { const dot = document.createElement('span'); dot.className = 'state-dot'; dot.dataset.state = 'running'; node.append(dot); }
      const kind = document.createElement('span'); kind.className = 'kind'; kind.textContent = entry.shortcut || entry.kind; node.append(kind);
      node.classList.toggle('highlighted', index === 0);
      node.onclick = () => { $('palette-dialog').close(); entry.run(); }; return node;
    }));
    $('palette-count').textContent = `${ordered.length} result${ordered.length === 1 ? '' : 's'}`;
    if (!ordered.length) { const empty = document.createElement('p'); empty.textContent = 'No matches'; empty.style.padding = '12px'; $('palette-list').append(empty); }
  }
  function move(step) {
    const nodes = [...$('palette-list').querySelectorAll('button')]; if (!nodes.length) return;
    highlighted = (highlighted + step + nodes.length) % nodes.length;
    nodes.forEach((node, index) => node.classList.toggle('highlighted', index === highlighted));
    nodes[highlighted].scrollIntoView({ block: 'nearest' });
  }
  $('palette-search').oninput = draw;
  $('palette-search').onkeydown = event => {
    if (event.key === 'Enter') { event.preventDefault(); $('palette-list').querySelector('button.highlighted')?.click() || $('palette-list').querySelector('button')?.click(); }
    else if (event.key === 'ArrowDown') { event.preventDefault(); move(1); }
    else if (event.key === 'ArrowUp') { event.preventDefault(); move(-1); }
  };
  $('palette-open').onclick = actions.palette;
  document.addEventListener('keydown', event => {
    if (document.querySelector('dialog[open]')) return;
    const mod = event.ctrlKey || event.metaKey;
    if (mod && !event.altKey && !event.shiftKey && /^[1-9]$/.test(event.key)) { event.preventDefault(); extra.tab?.(Number(event.key) - 1); return; }
    const chord = [mod ? 'mod' : '', event.altKey ? 'alt' : '', event.shiftKey ? 'shift' : '', event.key.toLowerCase()].filter(Boolean).join('+');
    const bindings = { ...defaults, ...state().settings.shortcuts };
    for (const [key, binding] of Object.entries(bindings)) if (chord === binding) { event.preventDefault(); actions[key]?.(); break; }
  });
  $('shortcuts-open').onclick = () => {
    $('shortcut-fields').replaceChildren(...Object.entries({ ...defaults, ...state().settings.shortcuts }).map(([key, value]) => {
      const label = document.createElement('label'); label.textContent = labels[key] || key; const input = document.createElement('input'); input.name = key; input.value = value; input.required = true; label.append(input); return label;
    })); $('shortcuts-dialog').showModal();
  };
  $('shortcuts-form').onsubmit = event => { event.preventDefault(); perform(async () => { update(await api.saveSettings({ ...state().settings, shortcuts: Object.fromEntries(new FormData(event.target)) })); $('shortcuts-dialog').close(); }); };
}
