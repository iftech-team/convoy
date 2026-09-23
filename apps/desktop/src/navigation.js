export function installNavigation({ $, api, perform, update, state, openSession, project, selectProject }) {
  const defaults = { palette: 'mod+shift+p', newSession: 'mod+n', files: 'mod+shift+g', next: 'mod+alt+arrowright', previous: 'mod+alt+arrowleft', settings: 'mod+,', search: 'mod+k' };
  const actions = { palette: () => { $('palette-dialog').showModal(); $('palette-search').value = ''; draw(); $('palette-search').focus(); }, newSession: () => $('new-session').click(), files: () => $('files-open').click(), settings: () => $('settings').click(), search: () => $('search').focus(), next: () => navigate(1), previous: () => navigate(-1) };
  function navigate(direction) {
    const sessions = state().sessions.filter(s => s.projectID === project().projectID && !s.archived);
    if (!sessions.length) return;
    const index = sessions.findIndex(s => s.id === project().sessionID);
    openSession(sessions[(index + direction + sessions.length) % sessions.length].id);
  }
  function draw() {
    const query = $('palette-search').value.toLowerCase();
    const entries = [
      ...Object.keys(actions).filter(key => key !== 'palette').map(key => ({ label: key, run: actions[key] })),
      ...state().projects.map(p => ({ label: `Project: ${p.title}`, run: () => selectProject(p.id) })),
      ...state().sessions.filter(s => !s.archived).map(s => ({ label: `Session: ${s.title}`, run: () => openSession(s.id) }))
    ];
    $('palette-list').replaceChildren(...entries.filter(e => e.label.toLowerCase().includes(query)).map(entry => {
      const node = document.createElement('button'); node.textContent = entry.label; node.onclick = () => { $('palette-dialog').close(); entry.run(); }; return node;
    }));
  }
  $('palette-search').oninput = draw;
  $('palette-search').onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); document.querySelector('#palette-list button')?.click(); } };
  $('palette-open').onclick = actions.palette;
  document.addEventListener('keydown', event => {
    if (document.querySelector('dialog[open]')) return;
    const chord = [event.ctrlKey || event.metaKey ? 'mod' : '', event.altKey ? 'alt' : '', event.shiftKey ? 'shift' : '', event.key.toLowerCase()].filter(Boolean).join('+');
    const bindings = { ...defaults, ...state().settings.shortcuts };
    for (const [key, binding] of Object.entries(bindings)) if (chord === binding) { event.preventDefault(); actions[key]?.(); break; }
  });
  $('shortcuts-open').onclick = () => {
    $('shortcut-fields').replaceChildren(...Object.entries({ ...defaults, ...state().settings.shortcuts }).map(([key, value]) => {
      const label = document.createElement('label'); label.textContent = key; const input = document.createElement('input'); input.name = key; input.value = value; input.required = true; label.append(input); return label;
    })); $('shortcuts-dialog').showModal();
  };
  $('shortcuts-form').onsubmit = event => { event.preventDefault(); perform(async () => { update(await api.saveSettings({ ...state().settings, shortcuts: Object.fromEntries(new FormData(event.target)) })); $('shortcuts-dialog').close(); }); };
}
