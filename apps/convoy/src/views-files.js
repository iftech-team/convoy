import { anchorStyle } from "./views.js";
// Files & Changes.
//
// Four views over one folder: the working tree, the files themselves, the
// commit log and the branches. Every destructive action is narrow, and each
// one asks before it runs.

import { icons } from "./icons.js";
import { button, escape, empty } from "./ui.js";
import { project, session, state } from "./state.js";

const VIEWS = [
  ["changes", "Changes", "diff"],
  ["files", "Files", "folder"],
  ["log", "Log", "clock"],
  ["branches", "Branches", "git"],
];

/// What Files & Changes can show from here: the open session's folder, its
/// project, and the other projects of the same group — the macOS picker for a
/// folder of repositories, where each repository is a project.
function fileTargets() {
  const current = project();
  const targets = [];
  const open = session();
  if (open?.working_directory) targets.push([open.id, `${open.title} (worktree)`]);
  if (current) targets.push([current.id, current.title]);
  if (current?.group) {
    for (const item of state.projects) {
      if (item.group === current.group && item.id !== current.id) targets.push([item.id, item.title]);
    }
  }
  return targets;
}

export function filesView() {
  const files = state.files;
  if (!files) return "";
  const snapshot = files.snapshot;

  return `
    <div class="files">
      <div class="files__bar">
        ${button({ icon: "back", action: "files-close", kind: "quiet", title: "Back" })}
        <div class="segmented">
          ${VIEWS.map(
            ([key, label]) =>
              `<button data-files-view="${key}" aria-pressed="${files.view === key}">${label}</button>`,
          ).join("")}
        </div>
        ${
          fileTargets().length > 1
            ? `<select id="files-repo" class="files__repo" title="Repository">${fileTargets()
                .map(([id, name]) => `<option value="${escape(id)}"${id === files.id ? " selected" : ""}>${escape(name)}</option>`)
                .join("")}</select>`
            : ""
        }
        <span class="crumb__muted">
          ${snapshot ? escape(snapshot.branch) : "…"}
          ${snapshot ? ` · ${snapshot.changes.length} changed` : ""}
        </span>
        ${
          snapshot?.upstream
            ? `<span class="files__sync" title="Compared with ${escape(snapshot.upstream.name)}">↑${snapshot.upstream.ahead} ↓${snapshot.upstream.behind}</span>`
            : ""
        }
        <span class="section__spacer"></span>
        ${button({ icon: "refresh", action: "files-refresh", kind: "icon", title: "Refresh" })}
        ${button({
          label: "Side by side",
          icon: "split",
          action: "files-split",
          kind: files.sideBySide ? "primary" : "",
        })}
        ${button({ label: "Remote", icon: "git", action: "files-remote" })}
      </div>
      <div class="files__body">
        <div class="files__column">
          <div class="files__scroll" data-scroll="files">${list(files)}</div>
          ${actions(files)}
        </div>
        <div class="files__preview" data-scroll="preview">${preview(files)}</div>
      </div>
      ${commitBar(files)}
    </div>`;
}

function list(files) {
  const snapshot = files.snapshot;
  if (!snapshot) return '<div class="list__none">Reading…</div>';

  if (files.view === "changes") {
    if (!snapshot.changes.length) {
      return '<div class="list__none">Nothing has changed.</div>';
    }
    return `
      ${snapshot.changes
        .map(
          (change) => `
        <button class="change${selected(files, change.path) ? " change--on" : ""}"
                data-change="${escape(change.path)}">
          <span class="change__status${change.conflict ? " change__status--bad" : ""}">${escape(change.status)}</span>
          <span class="change__path" title="${escape(change.path)}">${escape(tail(change.path))}</span>
        </button>`,
        )
        .join("")}
`;
  }

  if (files.view === "files") {
    if (!snapshot.files.length) {
      return '<div class="list__none">No files.</div>';
    }
    return snapshot.files
      .map(
        (name) => `
      <button class="change${selected(files, name) ? " change--on" : ""}"
              data-file="${escape(name)}">
        <span class="change__path" title="${escape(name)}">${escape(tail(name))}</span>
      </button>`,
      )
      .join("");
  }

  if (files.view === "log") {
    if (!snapshot.log.length) {
      return '<div class="list__none">No commits yet.</div>';
    }
    return `
      ${snapshot.log
        .map(
          (commit) => `
        <button class="commit${files.selection?.commit === commit.id ? " change--on" : ""}"
                data-commit="${escape(commit.id)}">
          <span class="commit__subject">${escape(commit.subject)}</span>
          <span class="commit__meta">
            ${escape(commit.short)} · ${escape(commit.author)} · ${escape(commit.date.split("T")[0] ?? "")}
          </span>
        </button>`,
        )
        .join("")}
`;
  }

  return `
    ${snapshot.branches
      .map(
        (branch) => `
      <button class="change${files.branch === branch ? " change--on" : ""}"
              data-branch="${escape(branch)}">
        <span class="change__path">${escape(branch)}</span>
        ${branch === snapshot.branch ? '<span class="badge--muted badge">current</span>' : ""}
      </button>`,
      )
      .join("")}
`;
}

/// The actions belong to the list, not inside it: a bar that scrolls away with
/// the rows is a bar the user cannot find when the list is long.
function actions(files) {
  if (!files.snapshot) return "";
  if (files.view === "changes") {
    return `
      <div class="files__actions">
        ${button({ label: "Stage", action: "stage" })}
        ${button({ label: "Unstage", action: "unstage" })}
        ${button({ label: "Stage all", action: "stage-all" })}
        ${button({ label: "Unstage all", action: "unstage-all" })}
        ${
          files.selection && files.snapshot.changes.some((change) => change.path === files.selection.path && change.conflict)
            ? button({ label: "Mark resolved", action: "resolve", kind: "primary" })
            : ""
        }
        ${button({ icon: "open", action: "file-open", kind: "quiet", title: "Open in its app" })}
        ${button({ icon: "folder", action: "file-reveal", kind: "quiet", title: "Show in folder" })}
        ${button({ icon: "copy", action: "file-copy", kind: "quiet", title: "Copy path" })}
        ${button({ label: "Discard", action: "discard", kind: "danger" })}
        ${button({ label: "Trash", action: "trash", kind: "danger" })}
        ${button({ label: "Discard hunk", action: "discard-hunk", kind: "danger" })}
      </div>`;
  }
  if (files.view === "log") {
    return `
      <div class="files__actions">
        ${button({ label: "Revert", action: "revert", kind: "danger" })}
        ${button({ label: "Reset soft", action: "reset-soft", kind: "danger" })}
        ${button({ label: "Reset mixed", action: "reset-mixed", kind: "danger" })}
      </div>`;
  }
  if (files.view === "branches") {
    return `
      <div class="files__actions">
        ${button({ label: "Switch", action: "switch-branch" })}
        ${button({ label: "New branch…", action: "new-branch" })}
      </div>`;
  }
  return "";
}

/// A path is identified by its end, so that is the end kept. `direction: rtl`
/// does this in CSS but moves a trailing slash to the front, which turns
/// `src/commands/` into `/src/commands`.
const tail = (path, max = 42) =>
  path.length <= max ? path : `…${path.slice(path.length - max + 1)}`;

const selected = (files, path) =>
  files.selection && "path" in files.selection && files.selection.path === path;

function preview(files) {
  const shown = files.preview;
  if (!shown) {
    return empty("diff", "Nothing selected", "Choose a file or a commit.");
  }
  if (shown.kind === "image") {
    return `<img class="preview__image" alt=""
                 src="data:${escape(shown.mime)};base64,${escape(shown.data)}" />`;
  }
  if (shown.kind === "markdown") {
    return `<div class="preview__markdown">${markdown(shown.text)}</div>`;
  }
  if (shown.kind === "split") {
    return `
      <div class="split">
        ${shown.rows
          .map((row) =>
            row.separator
              ? `<div class="split__hunk">${escape(row.separator)}</div>`
              : `<div class="split__row">
                   ${side(row.left, "left")}
                   ${side(row.right, "right")}
                 </div>`,
          )
          .join("")}
      </div>`;
  }
  if (!shown.text.trim()) {
    return empty("check", "Nothing to show", "This selection has no differences.");
  }
  return `<pre class="preview__text">${highlight(shown.text, shown.language)}</pre>`;
}

function side(cell, which) {
  if (!cell) return `<span class="split__cell split__cell--gap"></span>`;
  const [number, body] = cell;
  return `
    <span class="split__cell split__cell--${which}">
      <span class="split__number">${number}</span>
      <span class="split__text">${escape(body)}</span>
    </span>`;
}

/// Diff colouring only. Full syntax highlighting would mean shipping a
/// grammar set for a preview pane; the useful signal here is what changed.
function highlight(text, language) {
  if (language !== "diff") return escape(text);
  return text
    .split("\n")
    .map((line) => {
      const kind = line.startsWith("+++") || line.startsWith("---")
        ? "head"
        : line.startsWith("@@")
          ? "hunk"
          : line.startsWith("+")
            ? "add"
            : line.startsWith("-")
              ? "del"
              : line.startsWith("diff ") || line.startsWith("index ")
                ? "head"
                : "";
      return `<span class="diff-line${kind ? ` diff-line--${kind}` : ""}">${escape(line)}</span>`;
    })
    .join("\n");
}

/// Basic Markdown, as in the other builds: headings, emphasis, code and list
/// structure, nothing more.
export function markdown(text) {
  const inline = (value) =>
    escape(value)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/(^|[^*])\*([^*]+)\*/g, "$1<em>$2</em>");

  const out = [];
  let inCode = false;
  for (const line of String(text).split("\n")) {
    if (line.startsWith("```")) {
      out.push(inCode ? "</code></pre>" : '<pre class="md-code"><code>');
      inCode = !inCode;
      continue;
    }
    if (inCode) {
      out.push(escape(line));
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      out.push(`<li>${inline(line.replace(/^\s*[-*]\s+/, ""))}</li>`);
      continue;
    }
    if (!line.trim()) {
      out.push("<br />");
      continue;
    }
    out.push(`<p>${inline(line)}</p>`);
  }
  if (inCode) out.push("</code></pre>");
  return out.join("");
}

function commitBar(files) {
  if (files.view !== "changes") return "";
  return `
    <div class="commit-bar">
      <textarea id="commit-message" placeholder="Commit message"
                spellcheck="false">${escape(files.message)}</textarea>
      <div class="commit-bar__actions">
        ${button({ label: "Commit", action: "commit", kind: "primary" })}
        ${button({ label: "Commit & Push", action: "commit-push" })}
        ${button({ label: "Amend", action: "amend" })}
        ${button({ label: "Generate with Claude", action: "generate", icon: "bolt" })}
      </div>
    </div>`;
}

export function remoteMenu() {
  return `
    <div class="scrim scrim--clear" data-dismiss="1">
      <div class="menu menu--anchored" role="menu" style="${anchorStyle()}">
        <button class="menu__item" data-action="fetch"><span>Fetch</span></button>
        <button class="menu__item" data-action="pull"><span>Pull (fast-forward only)</span></button>
        <button class="menu__item" data-action="push"><span>Push</span></button>
        <div class="menu__divider"></div>
        <button class="menu__item" data-action="pr"><span>Create pull request</span></button>
      </div>
    </div>`;
}

export { icons };
