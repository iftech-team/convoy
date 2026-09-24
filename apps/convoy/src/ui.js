// The pieces every screen is built from. Markup only — no state, no calls.

import { icons } from "./icons.js";

export const escape = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (character) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        character
      ],
  );

/// Shortens a path for a place that cannot hold it, keeping the end, which is
/// the part that identifies it.
/// "1 task", "2 tasks". Small, but a count that reads wrong is the kind of
/// detail that makes an app feel unfinished.
export const plural = (count, one, many = `${one}s`) =>
  `${count} ${count === 1 ? one : many}`;

export const shorten = (value, max = 48) => {
  const text = String(value ?? "");
  return text.length <= max ? text : `…${text.slice(-(max - 1))}`;
};

export const button = ({
  label = "",
  icon = "",
  action = "",
  data = {},
  kind = "",
  title = "",
  disabled = false,
}) => {
  const attributes = Object.entries(data)
    .map(([key, value]) => `data-${key}="${escape(value)}"`)
    .join(" ");
  return `<button class="button${kind ? ` button--${kind}` : ""}"
    ${action ? `data-action="${escape(action)}"` : ""} ${attributes}
    ${title ? `title="${escape(title)}"` : ""} ${disabled ? "disabled" : ""}>
    ${icons[icon] ?? ""}${label ? escape(label) : ""}
  </button>`;
};

export const empty = (icon, title, text) => `
  <div class="empty">
    <span class="empty__mark">${icons[icon] ?? icons.folder}</span>
    <h2 class="empty__title">${escape(title)}</h2>
    <p class="empty__text">${escape(text)}</p>
  </div>`;

export const field = (label, control, note = "") => `
  <label class="field">
    <span class="field__label">${escape(label)}</span>
    ${control}
    ${note ? `<span class="field__note">${escape(note)}</span>` : ""}
  </label>`;

export const textInput = (id, value, placeholder = "") =>
  `<input id="${id}" value="${escape(value)}" placeholder="${escape(placeholder)}"
          spellcheck="false" />`;

export const textArea = (id, value, placeholder = "", rows = 4) =>
  `<textarea id="${id}" spellcheck="false" rows="${rows}"
             placeholder="${escape(placeholder)}">${escape(value)}</textarea>`;

export const choice = (key, options, selected) => `
  <div class="choice">
    ${options
      .map(
        ([value, label]) =>
          `<button data-set="${key}" data-value="${escape(value)}"
                   aria-pressed="${value === selected}">${escape(label)}</button>`,
      )
      .join("")}
  </div>`;

export const segmented = (key, options, selected) => `
  <div class="segmented">
    ${options
      .map(
        ([value, label]) =>
          `<button data-set="${key}" data-value="${escape(value)}"
                   aria-pressed="${value === selected}">${escape(label)}</button>`,
      )
      .join("")}
  </div>`;

export const toggle = (key, on, label) =>
  `<button class="switch" data-toggle="${escape(key)}" aria-pressed="${!!on}"
           aria-label="${escape(label)}"></button>`;

export const setting = (title, hint, control) => `
  <div class="setting">
    <div class="setting__text">
      <div class="setting__title">${escape(title)}</div>
      ${hint ? `<div class="setting__hint">${escape(hint)}</div>` : ""}
    </div>
    <div class="setting__control">${control}</div>
  </div>`;

export const group = (label, rows) => `
  <div class="group">
    ${label ? `<div class="group__label">${escape(label)}</div>` : ""}
    ${rows}
  </div>`;

/// A modal. `foot` holds its actions; the scrim behind it dismisses.
export const modal = ({ title, hint = "", body, foot, note = "", wide = false }) => `
  <div class="scrim" data-dismiss="1">
    <div class="modal${wide ? " modal--wide" : ""}" role="dialog" aria-modal="true"
         aria-label="${escape(title)}">
      <div class="modal__head">
        <div class="modal__title">${escape(title)}</div>
        ${hint ? `<div class="modal__hint">${escape(hint)}</div>` : ""}
      </div>
      <div class="modal__body" data-scroll="modal">${body}</div>
      ${note ? `<p class="modal__note">${note}</p>` : ""}
      <div class="modal__foot">${foot}</div>
    </div>
  </div>`;

export const confirm = ({ title, body, confirmLabel, action, danger = true }) =>
  modal({
    title,
    body: `<p class="modal__hint">${escape(body)}</p>`,
    foot: `
      ${button({ label: "Cancel", data: { dismiss: "1" } })}
      ${button({ label: confirmLabel, action, kind: danger ? "danger-solid" : "primary" })}`,
  });

/// Reads the value of a field the current dialog rendered.
export const value = (id) => document.querySelector(`#${id}`)?.value ?? "";
export const numberValue = (id, fallback) => {
  const parsed = Number(document.querySelector(`#${id}`)?.value);
  return Number.isFinite(parsed) ? Math.round(parsed) : fallback;
};
