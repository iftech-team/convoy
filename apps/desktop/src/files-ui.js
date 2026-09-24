export function installFiles({ api, $, button, perform, context }) {
  let target, data, lifetime = 0, snapshotGeneration = 0, previewGeneration = 0;
  let tab = 'changes', selectedCommit, lastPreview, lastRequest, loading = false, reading = false;
  const drafts = new Map(), operations = new Map();
  const dialog = $('files-dialog'), list = $('files-list'), preview = $('file-preview');
  const current = epoch => dialog.open && lifetime === epoch;
  function controls() {
    const busy = loading || operations.has(target), unavailable = !data || data.isRepo === false;
    for (const node of dialog.querySelectorAll('[data-git-action], [data-history-action], [data-file-action], #diff-hunks button, #git-commit-form button, #git-branch-form button, #git-pr')) {
      node.disabled = busy || unavailable || (node.hasAttribute('data-history-action') && !selectedCommit);
    }
    $('files-refresh').disabled = busy;
    $('files-progress').textContent = operations.get(target) || (loading ? 'Refreshing repository…' : reading ? 'Loading preview…' :
      data?.isRepo === false ? 'This folder is not a Git repository. You can browse its files.' : data ? `${data.changes.length} changed · ${data.changes.filter(f => f.index !== ' ' && !f.untracked).length} staged · Refresh after external edits` : 'Repository unavailable. Try Refresh.');
  }
  function clearPreview(message = 'Select a file or commit to preview.') {
    previewGeneration++; reading = false; lastPreview = undefined; lastRequest = undefined;
    preview.textContent = message; $('diff-hunks').replaceChildren();
  }
  function reconciled(request) {
    if (!request || !['staged', 'unstaged', 'untracked'].includes(request.kind)) return request;
    const file = data.changes.find(f => f.path === request.path);
    if (!file) return undefined;
    if (request.kind === 'staged' && file.index !== ' ' && !file.untracked) return request;
    return { path: file.path, kind: file.untracked ? 'untracked' : file.worktree !== ' ' ? 'unstaged' : 'staged' };
  }
  async function load() {
    const token = ++snapshotGeneration, epoch = lifetime, selection = previewGeneration;
    loading = true; controls();
    try {
      const result = await api.files(target);
      if (token !== snapshotGeneration || !current(epoch)) return;
      data = result;
      if (data.isRepo === false) tab = 'files';
      $('files-branch').textContent = data.branch; draw();
      // A refresh must not cancel a newer file selection, or leave an old diff after staging.
      if (selection === previewGeneration && lastRequest) {
        const request = reconciled(lastRequest);
        if (request) await read(request); else clearPreview('No remaining changes for this file.');
      }
    } finally { if (token === snapshotGeneration && current(epoch)) { loading = false; controls(); } }
  }
  async function read(request) {
    const token = ++previewGeneration, epoch = lifetime;
    lastRequest = request; lastPreview = undefined; reading = true;
    preview.textContent = 'Loading…'; $('diff-hunks').replaceChildren(); controls();
    try {
      const content = await api.fileRead(target, { ...request, structured: true });
      if (token === previewGeneration && current(epoch)) { lastPreview = content; showPreview(); }
    } catch (error) {
      if (token === previewGeneration && current(epoch)) { preview.textContent = 'Could not load the preview. Select the file again or refresh.'; throw error; }
    } finally { if (token === previewGeneration && current(epoch)) { reading = false; controls(); } }
  }
  async function operation(label, action) {
    const repository = target, epoch = lifetime;
    if (operations.has(repository)) return;
    operations.set(repository, label); controls();
    try { return await action(repository, () => current(epoch)); }
    finally { operations.delete(repository); if (dialog.open) controls(); }
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
      const columns = document.createElement('colgroup');
      for (const width of ['40px', '', '40px', '']) { const column = document.createElement('col'); if (width) column.style.width = width; columns.append(column); }
      table.append(columns);
      const header = table.createTHead().insertRow();
      for (const text of ['Before', 'After']) { const cell = document.createElement('th'); cell.colSpan = 2; cell.scope = 'colgroup'; cell.textContent = text; header.append(cell); }
      let oldLine = 0, newLine = 0, inHunk = false;
      for (const line of text.split('\n').slice(0, 5000)) {
        const row = document.createElement('tr');
        const match = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)/);
        if (match) { oldLine = Number(match[1]); newLine = Number(match[2]); inHunk = true; }
        if (!inHunk || match || line.startsWith('\\')) {
          if (!line) continue;
          const cell = document.createElement('td'); cell.colSpan = 4; cell.textContent = line; row.className = 'diff-meta'; row.append(cell); table.append(row); continue;
        }
        const removed = line.startsWith('-'), added = line.startsWith('+'), context = line.startsWith(' ');
        if (!removed && !added && !context) continue;
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
        await act('discardHunk', { path: lastRequest.path, hash: lastPreview.hash, hunk });
      }))));
    }
  }
  $('file-render').onchange = () => { showPreview(); controls(); };
  $('diff-split').onchange = () => { showPreview(); controls(); };
  async function act(action, value) {
    return operation('Applying Git operation…', async (repository, isCurrent) => {
      const changed = await api.fileAction(repository, action, value);
      if (isCurrent()) await load();
      return changed;
    });
  }
  function actionButton(label, action) { const node = button(label, false, action); node.dataset.fileAction = ''; return node; }
  function draw() {
    const query = $('files-filter').value.toLowerCase();
    list.replaceChildren();
    for (const node of document.querySelectorAll('[data-git-tab]')) { node.setAttribute('aria-pressed', String(node.dataset.gitTab === tab)); node.classList.toggle('selected', node.dataset.gitTab === tab); }
    if (tab === 'changes') for (const file of data.changes.filter(f => f.path.toLowerCase().includes(query))) {
      const row = document.createElement('div'); row.className = 'file-row';
      row.append(button(`${file.index}${file.worktree} ${file.path}`, false, () => perform(() => read({ kind: file.untracked ? 'untracked' : file.worktree !== ' ' ? 'unstaged' : 'staged', path: file.path }))));
      if (file.index !== ' ' && !file.untracked) row.append(button('Staged diff', false, () => perform(() => read({ kind: 'staged', path: file.path }))), actionButton('Unstage', () => perform(() => act('unstage', file))));
      if (file.worktree !== ' ' || file.untracked) row.append(actionButton('Stage', () => perform(() => act('stage', file))));
      if (!file.untracked && file.worktree !== ' ') row.append(actionButton('Discard', () => perform(() => act('discard', file))));
      if (file.untracked) row.append(actionButton('Trash', () => perform(() => act('trash', file))));
      list.append(row);
    }
    if (tab === 'files') for (const name of data.files.filter(f => f.toLowerCase().includes(query))) list.append(button(name, false, () => perform(() => read({ kind: 'file', path: name }))));
    if (tab === 'log') for (const commit of data.log.filter(c => `${c.subject} ${c.author}`.toLowerCase().includes(query))) list.append(button(`${commit.short} ${commit.subject} · ${commit.author}`, false, () => { selectedCommit = commit.id; controls(); perform(() => read({ kind: 'commit', commit: commit.id })); }));
    if (tab === 'branches') for (const branch of data.branches.filter(b => b.toLowerCase().includes(query))) list.append(actionButton(branch === data.branch ? `${branch} · current` : branch, () => perform(() => act('switch', { branch }))));
    $('git-history-actions').hidden = tab !== 'log';
    $('git-commit-form').hidden = tab !== 'changes';
    $('git-branch-form').hidden = tab !== 'branches';
    controls();
    if (!list.childElementCount) { const empty = document.createElement('p'); empty.textContent = query ? 'No matching entries. Try another filter.' : tab === 'changes' ? 'Working tree clean. No changes to review.' : tab === 'log' ? 'No commits yet.' : 'No entries to display.'; list.append(empty); }
  }
  $('files-open').onclick = () => perform(async () => {
    target = context(); if (!target) throw new Error('Open a project first.');
    lifetime++; snapshotGeneration++; clearPreview();
    selectedCommit = undefined; data = undefined; list.replaceChildren(); $('files-filter').value = ''; $('files-branch').textContent = '';
    $('git-message').value = drafts.get(target) || ''; $('git-amend').checked = false; dialog.showModal(); await load();
  });
  dialog.addEventListener('close', () => { lifetime++; snapshotGeneration++; clearPreview(); loading = false; drafts.set(target, $('git-message').value); });
  $('files-refresh').onclick = () => perform(load);
  $('files-filter').oninput = () => { if (data) draw(); };
  for (const node of document.querySelectorAll('[data-git-tab]')) node.onclick = () => { tab = node.dataset.gitTab; selectedCommit = undefined; clearPreview(); if (data) draw(); };
  for (const node of document.querySelectorAll('[data-git-action]')) node.onclick = () => perform(() => act(node.dataset.gitAction));
  for (const node of document.querySelectorAll('[data-history-action]')) node.onclick = () => perform(() => {
    if (!selectedCommit) throw new Error('Select a commit first.');
    return act(node.dataset.historyAction, { commit: selectedCommit });
  });
  $('git-generate').onclick = () => perform(() => operation('Generating commit message…', async (repository, isCurrent) => {
    const original = $('git-message').value, result = await api.fileAction(repository, 'generate');
    if (isCurrent() && $('git-message').value === original) $('git-message').value = result;
  }));
  $('git-pr').onclick = () => perform(() => operation('Creating pull request…', async (repository, isCurrent) => {
    const result = await api.fileAction(repository, 'pr');
    if (isCurrent()) { clearPreview(result); await load(); }
  }));
  $('git-commit-form').onsubmit = event => { event.preventDefault(); perform(async () => {
    const epoch = lifetime, message = $('git-message').value, amend = $('git-amend').checked;
    const committed = await act('commit', { message, amend });
    if (committed && current(epoch) && $('git-message').value === message) { $('git-message').value = ''; $('git-amend').checked = false; }
  }); };
  $('git-branch-form').onsubmit = event => { event.preventDefault(); perform(() => act('branch', { branch: $('git-new-branch').value })); };
}
