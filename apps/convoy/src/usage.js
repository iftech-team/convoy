// The Usage page's actions: range and filters, the chart's tooltips and its
// measured width, and the price table editor in Settings. main.js passes
// clicks, typing and changes here first.

import { attempt, state, toast } from "./state.js";
import { t } from "./i18n.js";
import { RANGES } from "./views-usage.js";

const KEY = "convoy.usage";

export function installUsage({ render }) {
  let remembered = {};
  try {
    remembered = JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {};
  } catch {
    /* defaults */
  }
  state.usageView = {
    days: RANGES.includes(remembered.days) ? remembered.days : 30,
    project: "",
    agent: ["claude", "codex"].includes(remembered.agent) ? remembered.agent : "",
    table: false,
    data: null,
    loading: false,
    error: null,
    width: 0,
  };
  state.pricesEditor = null;

  const remember = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify({ days: state.usageView.days, agent: state.usageView.agent }));
    } catch {
      /* not remembered */
    }
  };

  let seq = 0;
  /// Asks for the report; the previous one stays on screen, dimmed, until
  /// the new one arrives.
  async function load() {
    const view = state.usageView;
    const mine = ++seq;
    view.loading = true;
    if (state.page === "usage") render();
    const result = await attempt("usage_stats", { days: view.days, projectId: view.project || null, agent: view.agent || null });
    if (mine !== seq) return;
    view.loading = false;
    if (result.ok) {
      view.data = result.value;
      view.error = null;
    } else {
      view.error = String(result.error);
    }
    if (state.page === "usage" || state.page === "settings") render();
  }

  async function loadPrices() {
    const result = await attempt("usage_prices");
    if (!result.ok) {
      state.pricesEditor = { rows: [], defaults: [], dirty: false, error: String(result.error) };
    } else {
      state.pricesEditor = { rows: result.value.prices.map((row) => ({ ...row })), defaults: result.value.defaults, dirty: false, error: null };
    }
    render();
  }

  const blank = (model = "") => ({ model, input: null, output: null, cache_read: null, cache_write: null, cache_write_1h: null });

  async function savePrices(command, args) {
    const editor = state.pricesEditor;
    const result = await attempt(command, args);
    if (!result.ok) {
      editor.error = String(result.error);
      return render();
    }
    state.pricesEditor = { ...editor, rows: result.value.map((row) => ({ ...row })), dirty: false, error: null };
    toast(t("Prices saved"));
    render();
    if (state.usageView.data) load();
  }

  function onClick(target) {
    const data = target.dataset;
    const view = state.usageView;
    if (data.usageDays) {
      view.days = Number(data.usageDays);
      remember();
      load();
      return true;
    }
    if (data.usageAgent !== undefined) {
      view.agent = data.usageAgent;
      remember();
      load();
      return true;
    }
    if (data.usageTable) {
      view.table = !view.table;
      render();
      return true;
    }
    const editor = state.pricesEditor;
    if (!editor) return false;
    if (data.priceAdd !== undefined) {
      editor.rows.push(blank(data.priceAdd));
      editor.dirty = true;
      render();
      document.querySelector(`#price-${editor.rows.length - 1}-${data.priceAdd ? "input" : "model"}`)?.focus();
      return true;
    }
    if (data.priceRemove !== undefined) {
      editor.rows.splice(Number(data.priceRemove), 1);
      editor.dirty = true;
      render();
      return true;
    }
    if (data.priceReset) {
      savePrices("usage_prices_reset");
      return true;
    }
    if (data.priceSave) {
      savePrices("usage_prices_save", { prices: editor.rows });
      return true;
    }
    return false;
  }

  function onInput(field) {
    const editor = state.pricesEditor;
    if (!editor || field.dataset?.priceRow === undefined) return false;
    const row = editor.rows[Number(field.dataset.priceRow)];
    const key = field.dataset.priceKey;
    if (!row) return true;
    if (key === "model") row.model = field.value;
    else row[key] = field.value.trim() === "" || !Number.isFinite(Number(field.value)) ? null : Number(field.value);
    if (!editor.dirty) {
      editor.dirty = true;
      render();
    }
    return true;
  }

  function onChange(field) {
    if (field.id !== "usage-project") return false;
    state.usageView.project = field.value;
    load();
    return true;
  }

  /// The chart is drawn to the pixel width it has, so its text is never
  /// scaled; a new width redraws it once.
  function afterRender() {
    if (current && !current.isConnected) hideTip();
    if (state.page !== "usage") return;
    const host = document.querySelector(".usage-chart");
    const width = host ? Math.floor(host.clientWidth) : 0;
    if (width && Math.abs(width - state.usageView.width) > 4) {
      state.usageView.width = width;
      requestAnimationFrame(render);
    }
  }

  // --------------------------------------------------------- tooltip --

  const box = document.createElement("div");
  box.className = "usage-tip";
  box.setAttribute("role", "tooltip");
  box.hidden = true;
  document.body.append(box);
  let current = null;

  function showTip(node) {
    let parsed;
    try {
      parsed = JSON.parse(node.dataset.tip);
    } catch {
      return;
    }
    current?.classList.remove("is-hover");
    current = node;
    box.replaceChildren();
    const title = document.createElement("div");
    title.className = "usage-tip__title";
    title.textContent = parsed.title;
    box.append(title);
    for (const row of parsed.rows) {
      const line = document.createElement("div");
      line.className = "usage-tip__row";
      if (row.key) {
        const key = document.createElement("i");
        key.className = `usage-tip__key usage-tip__key--${row.key}`;
        line.append(key);
      }
      const value = document.createElement("strong");
      value.textContent = row.value;
      const label = document.createElement("span");
      label.textContent = row.label;
      line.append(value, label);
      box.append(line);
    }
    box.hidden = false;
    // A day's column is tall; the tip sits over its bars, not its hit area.
    const bars = [...node.querySelectorAll(".usage-bar")].map((bar) => bar.getBoundingClientRect());
    const rect = bars.length
      ? { left: bars[0].left, width: bars[0].width, top: Math.min(...bars.map((bar) => bar.top)), bottom: Math.max(...bars.map((bar) => bar.bottom)) }
      : node.getBoundingClientRect();
    const tipRect = box.getBoundingClientRect();
    let left = rect.left + rect.width / 2 - tipRect.width / 2;
    left = Math.max(8, Math.min(window.innerWidth - tipRect.width - 8, left));
    let top = rect.top - tipRect.height - 8;
    if (top < 8) top = rect.bottom + 8;
    box.style.left = `${left}px`;
    box.style.top = `${top}px`;
    node.classList.add("is-hover");
  }

  function hideTip() {
    current?.classList.remove("is-hover");
    current = null;
    box.hidden = true;
  }

  const tipOf = (event) => event.target.closest?.("[data-tip]");
  document.addEventListener("pointerover", (event) => {
    const node = tipOf(event);
    if (node && node !== current) showTip(node);
    else if (!node && current) hideTip();
  });
  document.addEventListener("focusin", (event) => {
    const node = tipOf(event);
    if (node) showTip(node);
  });
  document.addEventListener("focusout", (event) => tipOf(event) && hideTip());
  document.addEventListener("pointerleave", hideTip);

  return { load, loadPrices, onClick, onInput, onChange, afterRender, hideTip };
}
