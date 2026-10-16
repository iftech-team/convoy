// The Usage page: tokens and an estimated cost, read from every Claude Code
// and Codex transcript on this computer, and the price table it uses.
//
// Charts follow the data-viz rules: one hue per agent that never moves
// (Claude slot 1, Codex slot 2, validated against both surfaces), thin
// marks with a 2px surface gap, hairline grid, a legend whenever two series
// show, text in text colours, and a table for every value a tooltip shows.

import { agentIcon, icons } from "./icons.js";
import { button, escape, plural } from "./ui.js";
import { state } from "./state.js";
import { language, t } from "./i18n.js";

export const RANGES = [7, 30, 90];
const AGENTS = [
  ["claude", "Claude"],
  ["codex", "Codex"],
];

// ---------------------------------------------------------------- formats --

export const compact = (value) =>
  new Intl.NumberFormat(language(), { notation: "compact", maximumFractionDigits: value >= 1000 && value < 10000 ? 1 : value < 1000 ? 0 : 1 }).format(value ?? 0);
export const whole = (value) => new Intl.NumberFormat(language()).format(value ?? 0);
export const money = (value) =>
  new Intl.NumberFormat(language(), { style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: value > 0 && value < 0.01 ? 4 : 2 }).format(value ?? 0);
const dateOf = (text) => {
  const [y, m, d] = text.split("-").map(Number);
  return new Date(y, m - 1, d);
};
export const dayLabel = (text, long = false) =>
  dateOf(text).toLocaleDateString(language(), long ? { weekday: "short", month: "short", day: "numeric" } : { month: "short", day: "numeric" });
const projectName = (id) => (id ? state.projects.find((item) => item.id === id)?.title ?? t("Removed project") : t("No project"));

/// A tooltip's lines: the value leads, the name follows. Read with
/// textContent when shown, so nothing in it is markup.
const tip = (title, rows) => escape(JSON.stringify({ title, rows }));

// ------------------------------------------------------------------ chart --

/// A clean axis: steps of 1, 2 or 5 times a power of ten, about four.
function axis(value) {
  if (value <= 0) return { max: 4, step: 1 };
  const rough = value / 4;
  const power = 10 ** Math.floor(Math.log10(rough));
  const step = [1, 2, 5, 10].map((n) => n * power).find((n) => n >= rough);
  return { max: step * Math.ceil(value / step - 1e-9), step };
}

/// Tokens per day, stacked by agent: Claude at the baseline, Codex above.
function dayChart(days, agents) {
  const width = Math.max(320, state.usageView.width || 880);
  const height = 200;
  const left = 44;
  const bottom = 22;
  const plot = height - bottom - 8;
  const scale = axis(Math.max(...days.map((day) => agents.reduce((sum, agent) => sum + day[agent], 0)), 0));
  const max = scale.max;
  const band = (width - left) / days.length;
  const bar = Math.max(2, Math.min(24, band - 2));
  const y = (value) => 8 + plot - (value / max) * plot;
  const ticks = Array.from({ length: Math.round(max / scale.step) + 1 }, (_, n) => n * scale.step);
  const every = Math.ceil(days.length / Math.max(2, Math.floor((width - left) / 72)));
  const gap = 2;
  const parts = days.map((day, index) => {
    const x = left + index * band + (band - bar) / 2;
    const segments = [];
    const present = agents.filter((agent) => day[agent] > 0);
    let sum = 0;
    present.forEach((agent, at) => {
      const low = y(sum) - (at > 0 ? gap : 0);
      sum += day[agent];
      const high = Math.min(y(sum), low - 1);
      const h = low - high;
      const r = at === present.length - 1 ? Math.min(4, h, bar / 2) : 0;
      // Rounded at the data end only, square at the baseline; a 2px gap
      // of surface between the agents.
      segments.push(
        `<path class="usage-bar usage-bar--${agent}" d="M${x} ${low}V${high + r}q0 ${-r} ${r} ${-r}h${bar - 2 * r}q${r} 0 ${r} ${r}V${low}z"/>`,
      );
    });
    const total = agents.reduce((sum, agent) => sum + day[agent], 0);
    const rows = [
      ...agents.map((agent) => ({ value: compact(day[agent]), label: AGENTS.find(([key]) => key === agent)[1], key: agent })),
      ...(agents.length > 1 ? [{ value: compact(total), label: t("Total") }] : []),
      { value: money(day.cost), label: t("Estimated cost") },
    ];
    const label = `${dayLabel(day.day, true)}: ${whole(total)} ${t("tokens")}`;
    return `
      <g class="usage-col" tabindex="0" role="img" aria-label="${escape(label)}" data-tip="${tip(dayLabel(day.day, true), rows)}">
        <rect class="usage-col__hit" x="${left + index * band}" y="0" width="${band}" height="${height - bottom}"/>
        ${segments.join("")}
      </g>
      ${index % every === 0 ? `<text class="usage-axis" x="${left + index * band + band / 2}" y="${height - 6}" text-anchor="middle">${escape(dayLabel(day.day))}</text>` : ""}`;
  });
  return `
    <svg class="usage-chart__svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="group" aria-label="${escape(t("Tokens per day"))}">
      ${ticks
        .map(
          (value) => `<line class="${value ? "usage-grid" : "usage-baseline"}" x1="${left}" x2="${width}" y1="${y(value)}" y2="${y(value)}"/>
             <text class="usage-axis usage-axis--y" x="${left - 8}" y="${y(value) + 4}" text-anchor="end">${escape(compact(value))}</text>`,
        )
        .join("")}
      ${parts.join("")}
    </svg>`;
}

function dayTable(days, agents) {
  return `
    <div class="usage-table-wrap" data-scroll="usage-days">
      <table class="usage-table">
        <thead><tr><th>${escape(t("Day"))}</th>${agents.map((agent) => `<th class="num">${escape(AGENTS.find(([key]) => key === agent)[1])}</th>`).join("")}<th class="num">${escape(t("Estimated cost"))}</th></tr></thead>
        <tbody>${[...days]
          .reverse()
          .map((day) => `<tr><td>${escape(dayLabel(day.day, true))}</td>${agents.map((agent) => `<td class="num">${whole(day[agent])}</td>`).join("")}<td class="num">${money(day.cost)}</td></tr>`)
          .join("")}</tbody>
      </table>
    </div>`;
}

/// Twelve weeks, a column per week from Monday, a cell per day; one hue,
/// light to dark, in five steps.
function heatmap(days) {
  if (!days.length) return "";
  const first = dateOf(days[0].day);
  const lead = (first.getDay() + 6) % 7;
  const values = days.map((day) => day.total).filter((value) => value > 0).sort((a, b) => a - b);
  const cut = (share) => values[Math.min(values.length - 1, Math.floor(share * values.length))] ?? 0;
  const edges = [cut(0.25), cut(0.5), cut(0.75)];
  const level = (value) => (value <= 0 ? 0 : value <= edges[0] ? 1 : value <= edges[1] ? 2 : value <= edges[2] ? 3 : 4);
  const cell = 13;
  const step = cell + 3;
  const top = 18;
  const left = 30;
  const weeks = Math.ceil((lead + days.length) / 7);
  let months = "";
  let lastMonth = -1;
  const cells = days
    .map((day, index) => {
      const at = lead + index;
      const week = Math.floor(at / 7);
      const row = at % 7;
      const date = dateOf(day.day);
      if (row === 0 || index === 0) {
        if (date.getMonth() !== lastMonth && (row === 0 ? date.getDate() <= 7 || index === 0 : true)) {
          lastMonth = date.getMonth();
          months += `<text class="usage-axis" x="${left + week * step}" y="11">${escape(date.toLocaleDateString(language(), { month: "short" }))}</text>`;
        }
      }
      return `<rect class="usage-heat usage-heat--${level(day.total)}" x="${left + week * step}" y="${top + row * step}" width="${cell}" height="${cell}" rx="3"
                tabindex="0" role="img" aria-label="${escape(`${dayLabel(day.day, true)}: ${whole(day.total)} ${t("tokens")}`)}"
                data-tip="${tip(dayLabel(day.day, true), [{ value: compact(day.total), label: t("tokens") }])}"/>`;
    })
    .join("");
  const weekday = (offset) => new Date(2024, 0, 1 + offset).toLocaleDateString(language(), { weekday: "short" });
  const labels = [0, 2, 4].map((row) => `<text class="usage-axis" x="0" y="${top + row * step + 10}">${escape(weekday(row))}</text>`).join("");
  return `
    <svg class="usage-heatmap" width="${left + weeks * step}" height="${top + 7 * step}" role="group" aria-label="${escape(t("Daily activity, last 12 weeks"))}">
      ${months}${labels}${cells}
    </svg>
    <div class="usage-scale" aria-hidden="true">
      <span>${escape(t("Less"))}</span>${[0, 1, 2, 3, 4].map((n) => `<i class="usage-heat--${n}"></i>`).join("")}<span>${escape(t("More"))}</span>
    </div>`;
}

// ------------------------------------------------------------------ pieces --

const tile = (label, value, detail = "", extra = "") => `
  <div class="usage-tile">
    <div class="usage-tile__label">${escape(label)}${extra}</div>
    <div class="usage-tile__value">${escape(value)}</div>
    ${detail ? `<div class="usage-tile__detail">${escape(detail)}</div>` : ""}
  </div>`;

const legend = (agents) =>
  agents.length < 2
    ? ""
    : `<div class="usage-legend">${agents
        .map((agent) => `<span class="usage-legend__item"><i class="usage-swatch usage-swatch--${agent}"></i>${escape(AGENTS.find(([key]) => key === agent)[1])}</span>`)
        .join("")}</div>`;

const card = (title, body, aside = "", note = "") => `
  <section class="usage-card">
    <div class="usage-card__head"><h3 class="usage-card__title">${escape(title)}</h3>${aside}</div>
    ${body}
    ${note ? `<p class="usage-card__note">${escape(note)}</p>` : ""}
  </section>`;

const costCell = (value, partial = false) =>
  value === null || value === undefined
    ? `<span class="usage-none" title="${escape(t("No price for this model"))}">—</span>`
    : `${partial ? `<span title="${escape(t("Some of its models have no price"))}">≥ </span>` : ""}${money(value)}`;

function modelTable(models) {
  if (!models.length) return `<p class="home-none">${escape(t("No models in this range."))}</p>`;
  const head = ["Model", "Input", "Output", "Cache read", "Cache write", "Reasoning", "Total", "Estimated cost"];
  return `
    <div class="usage-table-wrap" data-scroll="usage-models">
      <table class="usage-table">
        <thead><tr>${head.map((name, at) => `<th${at ? ' class="num"' : ""}>${escape(t(name))}</th>`).join("")}</tr></thead>
        <tbody>${models
          .map(
            (row) => `
          <tr>
            <td class="usage-table__name">${agentIcon(row.agent, 13)}<span>${escape(row.model)}</span></td>
            <td class="num">${compact(row.tokens.input)}</td>
            <td class="num">${compact(row.tokens.output)}</td>
            <td class="num">${compact(row.tokens.cache_read)}</td>
            <td class="num">${compact(row.tokens.cache_write)}</td>
            <td class="num">${row.tokens.reasoning ? compact(row.tokens.reasoning) : "—"}</td>
            <td class="num usage-table__strong" title="${escape(whole(row.total))}">${compact(row.total)}</td>
            <td class="num">${costCell(row.cost)}</td>
          </tr>`,
          )
          .join("")}</tbody>
      </table>
    </div>`;
}

function projectTable(projects, total) {
  if (!projects.length) return `<p class="home-none">${escape(t("No projects in this range."))}</p>`;
  return `
    <div class="usage-table-wrap" data-scroll="usage-projects">
      <table class="usage-table">
        <thead><tr><th>${escape(t("Project"))}</th><th class="num">Claude</th><th class="num">Codex</th><th class="num">${escape(t("Total"))}</th><th class="usage-table__share">${escape(t("Share"))}</th><th class="num">${escape(t("Estimated cost"))}</th></tr></thead>
        <tbody>${projects
          .map((row) => {
            const share = total ? row.total / total : 0;
            return `
          <tr>
            <td class="usage-table__name">${row.project_id ? "" : `<span class="usage-other">${icons.folder}</span>`}<span>${escape(projectName(row.project_id))}</span></td>
            <td class="num">${row.claude ? compact(row.claude) : "—"}</td>
            <td class="num">${row.codex ? compact(row.codex) : "—"}</td>
            <td class="num usage-table__strong" title="${escape(whole(row.total))}">${compact(row.total)}</td>
            <td class="usage-table__share"><span class="usage-meter"><i style="width:${(share * 100).toFixed(1)}%"></i></span><span class="usage-meter__text">${Math.round(share * 100)}%</span></td>
            <td class="num">${costCell(row.cost, row.partial)}</td>
          </tr>`;
          })
          .join("")}</tbody>
      </table>
    </div>`;
}

function filters(view) {
  const projects = state.projects;
  return `
    <div class="usage-filters" role="toolbar" aria-label="${escape(t("Filters"))}">
      <div class="segmented">
        ${RANGES.map((days) => `<button data-usage-days="${days}" aria-pressed="${view.days === days}">${escape(t("{count} days", { count: days }))}</button>`).join("")}
      </div>
      <select id="usage-project" class="pref-select" aria-label="${escape(t("Project"))}">
        <option value="">${escape(t("All projects"))}</option>
        ${projects.map((item) => `<option value="${escape(item.id)}"${item.id === view.project ? " selected" : ""}>${escape(item.title)}</option>`).join("")}
      </select>
      <div class="segmented">
        ${[["", t("All agents")], ...AGENTS]
          .map(([value, name]) => `<button data-usage-agent="${value}" aria-pressed="${view.agent === value}">${value ? agentIcon(value, 12) : ""}${escape(name)}</button>`)
          .join("")}
      </div>
      ${view.data?.scanning ? `<span class="usage-status">${icons.refresh}<span>${escape(t("Reading new transcript lines…"))}</span></span>` : ""}
    </div>`;
}

// -------------------------------------------------------------------- page --

export function usageView() {
  const view = state.usageView;
  const data = view.data;
  const head = `
    <div class="home__head">
      <h1 class="home__greeting">${escape(t("Usage"))}</h1>
      <span class="home__summary">${escape(t("From the transcripts Claude Code and Codex keep on this computer"))}</span>
      <span class="section__spacer"></span>
      ${button({ label: t("Usage prices…"), icon: "gear", data: { "settings-open": "Usage prices" } })}
      ${button({ label: t("Home"), icon: "home", action: "home" })}
    </div>`;
  if (!data) {
    return `<div class="home usage" data-scroll="usage">${head}${filters(view)}
      <div class="usage-empty">${view.error ? escape(view.error) : `<span class="usage-spinner"></span>${escape(t("Reading transcripts… The first time can take a moment."))}`}</div></div>`;
  }
  const agents = view.agent ? [view.agent] : ["claude", "codex"];
  const empty = data.total === 0;
  const unpriced = data.unpriced.length;
  const priceNote = unpriced
    ? t("{models} without a price, left out of the cost.", { models: plural(unpriced, "model") })
    : t("At API list prices. Subscription plans are not billed per token.");
  const top = data.models[0];
  const body = empty
    ? `<div class="usage-empty">
         <strong>${escape(data.scanning || data.stale ? t("Reading transcripts…") : t("No usage in this range"))}</strong>
         <span>${escape(data.scanning || data.stale ? t("Numbers appear as soon as the first files are read.") : t("Nothing in the transcripts matches these filters. Try a longer range or all projects."))}</span>
       </div>
       ${card(t("Daily activity, last 12 weeks"), heatmap(data.heatmap))}`
    : `
      <div class="usage-tiles">
        ${tile(t("Total tokens"), compact(data.total), t("{input} in · {output} out · {cache} cache", { input: compact(data.tokens.input), output: compact(data.tokens.output), cache: compact(data.tokens.cache_read + data.tokens.cache_write) }))}
        ${tile(t("Estimated cost"), money(data.cost), priceNote, `<span class="usage-badge">${escape(t("estimate"))}</span>`)}
        ${tile(t("Active days"), t("{count} of {days}", { count: data.active_days, days: data.days.length }), plural(data.tokens.requests, "request"))}
        ${tile(t("Top model"), top?.model ?? "—", top ? t("{share}% of tokens", { share: Math.round((top.total / data.total) * 100) }) : "")}
      </div>
      ${unpriced ? `<div class="usage-notice">${icons.warn}<span>${escape(t("No price for {names}.", { names: data.unpriced.join(", ") }))}</span>${button({ label: t("Set prices…"), kind: "quiet", data: { "settings-open": "Usage prices" } })}</div>` : ""}
      ${card(
        t("Tokens per day"),
        view.table ? dayTable(data.days, agents) : `<div class="usage-chart">${dayChart(data.days, agents)}</div>`,
        `${legend(agents)}<button class="button button--quiet usage-toggle" data-usage-table="1" aria-pressed="${!!view.table}">${icons.list}${escape(view.table ? t("Chart") : t("Table"))}</button>`,
      )}
      <div class="usage-columns">
        ${card(t("By project"), projectTable(data.projects, data.total))}
        ${card(t("Daily activity, last 12 weeks"), heatmap(data.heatmap))}
      </div>
      ${card(t("By model"), modelTable(data.models), "", t("Costs are estimates from the price table in Settings. Input is what was neither read from nor written to the cache; reasoning is part of output."))}`;
  return `
    <div class="home usage${view.loading ? " usage--refreshing" : ""}" data-scroll="usage">
      ${head}
      ${filters(view)}
      ${data.prices_error ? `<div class="usage-notice">${icons.warn}<span>${escape(data.prices_error)} ${escape(t("Using the default prices."))}</span></div>` : ""}
      ${body}
    </div>`;
}

// ---------------------------------------------------------------- settings --

const rate = (row, index, key) => `
  <input class="pref-number usage-price__rate" id="price-${index}-${key}" type="number" min="0" step="0.01"
         inputmode="decimal" data-price-row="${index}" data-price-key="${key}" aria-label="${escape(t(PRICE_KEYS.find(([name]) => name === key)[1]))}"
         value="${row[key] === null || row[key] === undefined ? "" : escape(row[key])}" placeholder="—" />`;

const PRICE_KEYS = [
  ["input", "Input"],
  ["output", "Output"],
  ["cache_read", "Cache read"],
  ["cache_write", "Cache write 5m"],
  ["cache_write_1h", "Cache write 1h"],
];

export function pricesSection() {
  const editor = state.pricesEditor;
  if (!editor) return `<div class="pref-empty">${escape(t("Loading…"))}</div>`;
  const seen = (state.usageView.data?.unpriced ?? []).filter((model) => !editor.rows.some((row) => row.model.trim().toLowerCase() === model.toLowerCase()));
  return `
    <section class="pref-group">
      <h3 class="pref-group__title">${escape(t("Price per million tokens, USD"))}</h3>
      <div class="pref-group__box usage-prices" data-scroll="usage-prices">
        <table class="usage-table usage-prices__table">
          <thead><tr><th>${escape(t("Model"))}</th>${PRICE_KEYS.map(([, name]) => `<th class="num">${escape(t(name))}</th>`).join("")}<th></th></tr></thead>
          <tbody>${editor.rows
            .map(
              (row, index) => `
            <tr>
              <td><input class="pref-text usage-price__model" id="price-${index}-model" data-price-row="${index}" data-price-key="model"
                         value="${escape(row.model)}" placeholder="claude-opus-4-*" spellcheck="false" aria-label="${escape(t("Model"))}" /></td>
              ${PRICE_KEYS.map(([key]) => `<td>${rate(row, index, key)}</td>`).join("")}
              <td>${button({ icon: "trash", kind: "quiet", title: t("Remove"), data: { "price-remove": index } })}</td>
            </tr>`,
            )
            .join("")}</tbody>
        </table>
      </div>
      <p class="pref-group__footer">${escape(t("A model id also matches its dated releases; end it with * to match a whole family. An empty rate is unknown, and tokens that need it are left out of the cost. Anthropic's API prices are filled in; OpenAI's are left for you to add."))}</p>
    </section>
    ${seen.length ? `<div class="usage-prices__seen"><span>${escape(t("Seen without a price:"))}</span>${seen.map((model) => `<button class="usage-chip" data-price-add="${escape(model)}">${icons.plus}${escape(model)}</button>`).join("")}</div>` : ""}
    ${editor.error ? `<p class="usage-prices__error" role="alert">${escape(editor.error)}</p>` : ""}
    <div class="pref-actions">
      ${button({ label: t("Add model"), icon: "plus", data: { "price-add": "" } })}
      ${button({ label: t("Reset to defaults"), icon: "refresh", data: { "price-reset": "1" } })}
      <span class="section__spacer"></span>
      ${editor.dirty ? `<span class="usage-prices__dirty">${escape(t("Unsaved changes"))}</span>` : ""}
      ${button({ label: t("Save prices"), kind: "primary", data: { "price-save": "1" }, disabled: !editor.dirty })}
    </div>
    <p class="pref-group__footer">${escape(t("Every cost shown is an estimate. Subscription plans are not billed per token."))}</p>`;
}
