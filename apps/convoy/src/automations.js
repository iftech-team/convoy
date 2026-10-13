// The Automations tab's actions: list, edit, switch on and off, run now,
// delete. main.js passes clicks, typing and changes here first; anything this
// does not handle goes on to main.js as before.

import { attempt, done, ensureModels, activeAccount, lastChoice, state, toast } from "./state.js";
import { t } from "./i18n.js";
import { DAYS, draftSchedule, previewBlock } from "./views-automations.js";
import { fitSkills } from "./skills.js";
import { autoSkills } from "./views-skills.js";

const FIELDS = {
  "auto-title": "title",
  "auto-prompt": "prompt",
  "auto-at": "at",
  "auto-from": "from",
};

export function installAutomations({ render, openModal, closeDialog, selectProject, loadWorkspace }) {
  state.automationsLoaded = false;
  state.allAutomations = [];
  const editing = () => state.dialog?.kind === "automation";

  /// The selected project's automations, and every project's for Home.
  async function load() {
    const all = await attempt("automations_list", { projectId: null });
    state.automationsLoaded = true;
    if (all.ok) state.allAutomations = all.value ?? [];
  }

  async function reload() {
    await load();
    render();
  }

  // ------------------------------------------------------------ editor --

  let previewSeq = 0;
  let previewTimer = null;

  /// The next three runs, asked of the backend as the schedule changes, and
  /// drawn in place so typing elsewhere in the sheet is never disturbed.
  function refreshPreview() {
    clearTimeout(previewTimer);
    previewTimer = setTimeout(async () => {
      const dialog = state.dialog;
      if (!editing()) return;
      const seq = ++previewSeq;
      const result = await attempt("automation_preview", { schedule: draftSchedule(dialog) });
      if (seq !== previewSeq || state.dialog !== dialog) return;
      dialog.preview = result.ok ? result.value : [];
      dialog.previewError = result.ok ? null : result.error;
      const node = document.querySelector(".auto-preview");
      if (node) node.outerHTML = previewBlock(dialog);
    }, 120);
  }

  function open(item) {
    const agent = item?.agent ?? state.settings.default_agent ?? "claude";
    const schedule = item?.schedule ?? { kind: "weekdays", at: "09:00", days: ["mon", "tue", "wed", "thu", "fri"] };
    const fresh = item ? {} : lastChoice(agent);
    openModal({
      kind: "automation",
      id: item?.id ?? null,
      project_id: item?.project_id ?? state.projectId,
      title: item?.title ?? "",
      prompt: item?.prompt ?? "",
      agent,
      account: item ? (item.profile_id ?? "") : activeAccount(agent),
      model: item?.model ?? fresh.model ?? "",
      effort: item?.effort ?? fresh.effort ?? "",
      view: item?.view ?? "terminal",
      worktree: item?.worktree ?? false,
      catch_up: item?.catch_up ?? true,
      enabled: item?.enabled ?? true,
      schedule_kind: schedule.kind,
      at: schedule.at ?? "09:00",
      days: schedule.days ?? ["mon", "tue", "wed", "thu", "fri"],
      hours: schedule.hours ?? 4,
      from: schedule.from ?? "09:00",
      preview: null,
      previewError: null,
      // A new one starts with the automatic skills.
      skills: item ? (item.skills ?? []) : autoSkills(agent, item?.project_id ?? state.projectId),
      skillsTouched: !!item,
    });
    ensureModels(agent, state.dialog.account);
    refreshPreview();
  }

  async function save() {
    const dialog = state.dialog;
    if (!editing() || dialog.busy) return;
    const input = {
      id: dialog.id,
      project_id: dialog.project_id,
      title: dialog.title,
      prompt: dialog.prompt,
      agent: dialog.agent,
      model: dialog.model === "__loading" ? "" : dialog.model ?? "",
      effort: dialog.effort ?? "",
      profile_id: dialog.account || null,
      view: dialog.view,
      worktree: !!dialog.worktree,
      schedule: draftSchedule(dialog),
      enabled: dialog.enabled !== false,
      catch_up: !!dialog.catch_up,
      skills: dialog.skills ?? [],
    };
    dialog.busy = true;
    render();
    const saved = await attempt("automation_save", { input });
    if (state.dialog !== dialog) return;
    dialog.busy = false;
    if (!saved.ok) {
      toast(t(saved.error), "error");
      return render();
    }
    closeDialog();
    toast(dialog.id ? t("Automation saved") : t("Automation created"));
    await reload();
  }

  // ------------------------------------------------------------ actions --

  const find = (id) => (state.allAutomations ?? []).find((item) => item.id === id);

  async function runNow(id) {
    const item = find(id);
    if (item) item.running = true;
    render();
    const result = await attempt("automation_run_now", { id });
    if (!result.ok) toast(t(result.error), "error");
    else toast(t("Started {title}", { title: item?.title ?? "" }));
    await loadWorkspace();
    await reload();
  }

  async function setEnabled(id, enabled) {
    const item = find(id);
    if (item) item.enabled = enabled;
    render();
    await done("automation_enable", { id, enabled });
    await reload();
  }

  function remove(id) {
    const item = find(id);
    openModal({
      kind: "confirm",
      title: t("Delete “{title}”?", { title: item?.title ?? "" }),
      body: t("It stops running. Sessions it already started stay."),
      confirmLabel: t("Delete"),
      danger: true,
      run: async () => {
        if (await done("automation_delete", { id })) toast(t("Automation deleted"));
        await reload();
      },
    });
  }

  /// A click anywhere. True when it was ours.
  async function onClick(target) {
    const data = target.dataset;
    if (data.auto) {
      const id = data.autoId;
      if (data.auto === "new") {
        if (!state.projectId) return true;
        open(null);
        return true;
      }
      if (data.auto === "edit") return open(find(id)), true;
      if (data.auto === "run") return runNow(id), true;
      if (data.auto === "delete") return remove(id), true;
      if (data.auto === "tab") {
        await openTab(data.autoProject);
        return true;
      }
    }
    if (data.toggle?.startsWith("auto-enabled:")) {
      const id = data.toggle.slice("auto-enabled:".length);
      await setEnabled(id, target.getAttribute("aria-pressed") !== "true");
      return true;
    }
    if (!editing()) return false;
    if (data.action === "automation-save") {
      await save();
      return true;
    }
    const dialog = state.dialog;
    if (data.set === "auto-agent") {
      if (dialog.agent !== data.value) {
        dialog.agent = data.value;
        dialog.account = activeAccount(data.value);
        Object.assign(dialog, lastChoice(data.value));
        ensureModels(dialog.agent, dialog.account);
        fitSkills(dialog, data.value, dialog.project_id);
      }
      render();
      return true;
    }
    if (data.set === "auto-view") {
      dialog.view = data.value;
      render();
      return true;
    }
    if (data.set === "auto-kind") {
      dialog.schedule_kind = data.value;
      render();
      refreshPreview();
      return true;
    }
    if (data.autoDay) {
      const days = new Set(dialog.days);
      if (!days.delete(data.autoDay)) days.add(data.autoDay);
      dialog.days = DAYS.map(([key]) => key).filter((key) => days.has(key));
      render();
      refreshPreview();
      return true;
    }
    if (data.toggle === "auto-worktree" || data.toggle === "auto-catchup") {
      const key = data.toggle === "auto-worktree" ? "worktree" : "catch_up";
      dialog[key] = !dialog[key];
      render();
      return true;
    }
    return false;
  }

  /// Typing in the editor goes into its draft as it is typed.
  function onInput(field) {
    if (!editing() || !field.closest?.(".modal")) return false;
    const key = FIELDS[field.id];
    if (!key) return false;
    state.dialog[key] = field.value;
    if (key === "at" || key === "from") refreshPreview();
    return true;
  }

  function onChange(field) {
    if (!editing()) return false;
    const dialog = state.dialog;
    if (field.id === "auto-hours") {
      dialog.hours = Number(field.value);
      refreshPreview();
      return true;
    }
    if (field.id === "auto-account") {
      dialog.account = field.value;
      ensureModels(dialog.agent, dialog.account);
      render();
      return true;
    }
    if (field.id === "auto-model") {
      dialog.model = field.value;
      render();
      return true;
    }
    if (field.id === "auto-effort") {
      dialog.effort = field.value;
      return true;
    }
    if (field.id in FIELDS) {
      onInput(field);
      return true;
    }
    return false;
  }

  /// The Automations tab of a project.
  async function openTab(projectId) {
    if (projectId && projectId !== state.projectId) await selectProject(projectId);
    state.page = null;
    state.tab = "automations";
    state.sessionId = null;
    state.files = null;
    render();
    await reload();
  }

  return { load, reload, open, onClick, onInput, onChange, openTab, find };
}
