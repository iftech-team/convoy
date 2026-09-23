export function installFiles({ api, $, button, perform, context }) {
  let target, data, generation = 0, tab = 'changes', selectedCommit, lastPreview, lastRequest;
  const drafts = new Map();
  const dialog = $('files-dialog'), list = $('files-list'), preview = $('file-preview');
  async function load() {
    const token = ++generation;
    const result = await api.files(target);
    if (token !== generation || !dialog.open) return;
    data = result; $('files-branch').textContent = data.branch; draw();
  }
  async function read(request) {
    const token = ++generation; preview.textContent = 'Loading…'; lastPreview = undefined; $('diff-hunks').replaceChildren();
    const content = await api.fileRead(target, { ...request, structured: true });
    if (token === generation && dialog.open) { lastPreview = content; lastRequest = request; showPreview(); }
  }
  function showPreview() {
    if (!lastPreview) return;
    preview.replaceChildren();
    const text = lastPreview.text || 'No differences.';
    if (lastPreview.image) { const image = document.createElement('img'); image.src = lastPreview.image; image.alt = lastRequest.path; image.style.maxWidth = '100%'; preview.append(image); $('diff-hunks').replaceChildren(); return; }
    if (lastPreview.markdown && $('file-render').checked) {
      let code = false;
      for (const line of text.split('\n').slice(0, 5000)) {
        if (line.startsWith('```')) { code = !code; continue; }
        const heading = !code && line.match(/^(#{1,6}) (.*)$/);
        const node = document.createElement(code ? 'code' : heading ? `h${heading[1].length}` : 'p'); node.textContent = heading ? heading[2] : line; preview.append(node);
      }
      $('diff-hunks').replaceChildren(); return;
    }
    if (!$('diff-split').checked || !['staged', 'unstaged'].includes(lastRequest.kind)) { preview.textContent = text; }
    else {
      const table = document.createElement('table'); table.className = 'split-diff';
      let oldLine = 0, newLine = 0;
      for (const line of text.split('\n').slice(0, 5000)) {
        const row = document.createElement('tr');
        const match = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)/);
        if (match) { oldLine = Number(match[1]); newLine = Number(match[2]); }
        const removed = line.startsWith('-') && !line.startsWith('---'), added = line.startsWith('+') && !line.startsWith('+++'), context = line.startsWith(' ');
        row.className = removed ? 'removed' : added ? 'added' : '';
        const values = removed ? [oldLine++, line.slice(1), '', ''] : added ? ['', '', newLine++, line.slice(1)] : context ? [oldLine++, line.slice(1), newLine++, line.slice(1)] : ['', line, '', line];
        for (const value of values) { const cell = document.createElement('td'); cell.textContent = value; row.append(cell); } table.append(row);
      }
      preview.append(table);
    }
    $('diff-hunks').replaceChildren();
    if (lastRequest.kind === 'unstaged' && !/^(new file|deleted file|rename from|rename to)/m.test(text)) {
      const matches = text.match(/^@@ .*$/gm) || [];
      matches.slice(0, 50).forEach((label, hunk) => $('diff-hunks').append(button(`Discard hunk ${hunk + 1}`, false, () => perform(async () => {
        const request = lastRequest;
        await act('discardHunk', { path: request.path, hash: lastPreview.hash, hunk }); await read(request);
      }))));
    }
  }
  $('file-render').onchange = showPreview;
  $('diff-split').onchange = showPreview;
  async function act(action, value) { const changed = await api.fileAction(target, action, value); await load(); return changed; }
  function draw() {
    const query = $('files-filter').value.toLowerCase();
    list.replaceChildren();
    for (const node of document.querySelectorAll('[data-git-tab]')) node.setAttribute('aria-pressed', String(node.dataset.gitTab === tab));
    if (tab === 'changes') for (const file of data.changes.filter(f => f.path.toLowerCase().includes(query))) {
      const row = document.createElement('div'); row.className = 'file-row';
      row.append(button(`${file.index}${file.worktree} ${file.path}`, false, () => perform(() => read({ kind: file.untracked ? 'untracked' : 'unstaged', path: file.path }))));
      if (file.index !== ' ' && !file.untracked) row.append(button('Staged diff', false, () => perform(() => read({ kind: 'staged', path: file.path }))), button('Unstage', false, () => perform(() => act('unstage', file))));
      if (file.worktree !== ' ' || file.untracked) row.append(button('Stage', false, () => perform(() => act('stage', file))));
      if (!file.untracked && file.worktree !== ' ') row.append(button('Discard', false, () => perform(() => act('discard', file))));
      if (file.untracked) row.append(button('Trash', false, () => perform(() => act('trash', file))));
      list.append(row);
    }
    if (tab === 'files') for (const name of data.files.filter(f => f.toLowerCase().includes(query))) list.append(button(name, false, () => perform(() => read({ kind: 'file', path: name }))));
    if (tab === 'log') for (const commit of data.log.filter(c => `${c.subject} ${c.author}`.toLowerCase().includes(query))) list.append(button(`${commit.short} ${commit.subject} · ${commit.author}`, false, () => { selectedCommit = commit.id; perform(() => read({ kind: 'commit', commit: commit.id })); }));
    if (tab === 'branches') for (const branch of data.branches.filter(b => b.toLowerCase().includes(query))) list.append(button(branch, branch === data.branch, () => perform(() => act('switch', { branch }))));
    $('git-history-actions').hidden = tab !== 'log';
    if (!list.childElementCount) { const empty = document.createElement('p'); empty.textContent = 'No matching entries.'; list.append(empty); }
  }
  $('files-open').onclick = () => perform(async () => {
    target = context(); if (!target) throw new Error('Open a project first.');
    preview.textContent = ''; selectedCommit = undefined; lastPreview = undefined; data = undefined; list.replaceChildren(); $('diff-hunks').replaceChildren(); $('git-message').value = drafts.get(target) || ''; $('git-amend').checked = false; dialog.showModal(); await load();
  });
  dialog.addEventListener('close', () => { generation++; drafts.set(target, $('git-message').value); lastPreview = undefined; });
  $('files-refresh').onclick = () => perform(load);
  $('files-filter').oninput = () => { if (data) draw(); };
  for (const node of document.querySelectorAll('[data-git-tab]')) node.onclick = () => { generation++; tab = node.dataset.gitTab; preview.textContent = ''; lastPreview = undefined; $('diff-hunks').replaceChildren(); if (data) draw(); };
  for (const node of document.querySelectorAll('[data-git-action]')) node.onclick = () => perform(() => act(node.dataset.gitAction));
  for (const node of document.querySelectorAll('[data-history-action]')) node.onclick = () => perform(() => {
    if (!selectedCommit) throw new Error('Select a commit first.');
    return act(node.dataset.historyAction, { commit: selectedCommit });
  });
  $('git-generate').onclick = () => perform(async () => {
    const repository = target, original = $('git-message').value, result = await api.fileAction(target, 'generate');
    if (dialog.open && target === repository && $('git-message').value === original) $('git-message').value = result;
  });
  $('git-pr').onclick = () => perform(async () => { const result = await api.fileAction(target, 'pr'); preview.textContent = result; await load(); });
  $('git-commit-form').onsubmit = event => { event.preventDefault(); perform(async () => {
    const message = $('git-message').value, amend = $('git-amend').checked;
    const committed = await act('commit', { message, amend });
    if (committed && $('git-message').value === message) { $('git-message').value = ''; $('git-amend').checked = false; }
  }); };
  $('git-branch-form').onsubmit = event => { event.preventDefault(); perform(() => act('branch', { branch: $('git-new-branch').value })); };
}
