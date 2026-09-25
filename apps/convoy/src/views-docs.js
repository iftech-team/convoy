// The Docs tab, as the macOS app has it: the project doc and specs kept in
// the repository under `.specdesk`, beside the repository's own Markdown.
// A list on the left, the rendered file (or its editor) on the right.

import { icons } from "./icons.js";
import { button, escape } from "./ui.js";
import { markdown } from "./views-files.js";
import { state } from "./state.js";

export const PROJECT_DOC = ".specdesk/PROJECT.md";
const SPECS = ".specdesk/specs/";

const nameOf = (file) => file.replace(SPECS, "").replace(".specdesk/", "");

function fileRow(file) {
  const docs = state.docs;
  const glyph = file === PROJECT_DOC ? icons.book : file.startsWith(SPECS) ? icons.doc : icons.page;
  return `
    <button class="doc-file${file.startsWith(".specdesk") ? " doc-file--ours" : ""}" data-doc="${escape(file)}"
            aria-current="${docs.selected === file}" title="${escape(file)}">
      ${glyph}<span>${escape(nameOf(file))}</span>
    </button>`;
}

function group(title, files) {
  if (!files.length) return "";
  return `<div class="doc-group">${escape(title)}</div>${files.map(fileRow).join("")}`;
}

function viewer() {
  const docs = state.docs;
  const file = docs.selected;
  if (!file) {
    const hasDoc = docs.files.includes(PROJECT_DOC);
    return `
      <div class="docs__welcome">
        <span class="docs__badge">${icons.book}</span>
        <h2>Project doc and specs</h2>
        <p>The project doc gives every agent the context it needs. Specs describe one piece of work each; tasks link to them.</p>
        <div class="docs__actions">
          ${
            hasDoc
              ? button({ label: "Open project doc", icon: "book", kind: "primary", data: { doc: PROJECT_DOC } })
              : button({ label: "Write project doc with AI", icon: "sparkles", kind: "primary", action: "doc-write-project" })
          }
          ${button({ label: "New spec", icon: "plus", action: "doc-new-spec" })}
        </div>
      </div>`;
  }
  const isSpec = file.startsWith(SPECS);
  return `
    <div class="docs__bar">
      <span class="docs__path" title="${escape(file)}">${escape(file)}</span>
      <span class="section__spacer"></span>
      ${file === PROJECT_DOC ? button({ label: "Refresh with AI", icon: "sparkles", action: "doc-write-project" }) : ""}
      ${isSpec ? button({ label: "Draft with AI", icon: "sparkles", action: "doc-draft-spec" }) : ""}
      ${isSpec ? button({ label: "Task from spec", icon: "plus", action: "doc-task" }) : ""}
      ${
        docs.editing
          ? button({ label: docs.dirty ? "Save" : "Done", action: docs.dirty ? "doc-save" : "doc-done", kind: docs.dirty ? "primary" : "" })
          : button({ label: "Edit", icon: "edit", action: "doc-edit" })
      }
      ${button({ icon: "copy", action: "doc-copy-path", kind: "quiet", title: "Copy path" })}
    </div>
    ${
      docs.editing
        ? `<textarea id="doc-editor" class="docs__editor" spellcheck="false">${escape(docs.text)}</textarea>`
        : `<div class="docs__render" data-scroll="doc">${docs.loading ? '<p class="pref-muted">Loading…</p>' : markdown(docs.text)}</div>`
    }`;
}

export function docsView() {
  const docs = state.docs;
  if (!docs) return "";
  const hasDoc = docs.files.includes(PROJECT_DOC);
  const specs = docs.files.filter((file) => file.startsWith(SPECS));
  const others = docs.files.filter((file) => file !== PROJECT_DOC && !file.startsWith(SPECS));
  return `
    <div class="docs">
      <aside class="docs__list">
        <div class="docs__head">
          <span>Docs & specs</span>
          ${button({ icon: "refresh", action: "doc-reload", kind: "quiet", title: "Reload from disk" })}
        </div>
        <div class="docs__buttons">
          ${hasDoc ? "" : button({ label: "Write project doc with AI", icon: "sparkles", action: "doc-write-project", kind: "primary" })}
          ${button({ label: "New spec…", icon: "plus", action: "doc-new-spec", kind: hasDoc ? "primary" : "" })}
        </div>
        <div class="docs__files" data-scroll="doc-list">
          ${group("Project doc", hasDoc ? [PROJECT_DOC] : [])}
          ${group("Specs", specs)}
          ${group("Other docs", others)}
          ${docs.files.length ? "" : '<p class="docs__none">Nothing here yet.</p>'}
        </div>
        <p class="docs__note">Stored in the repo under .specdesk so agents read them as files.</p>
      </aside>
      <section class="docs__view">${viewer()}</section>
    </div>`;
}
