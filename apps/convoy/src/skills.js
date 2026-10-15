// The skills library's actions: list, edit, delete, add a template, pick
// skills for a new session, task or automation, and switch a session's
// skills from its chip. main.js passes clicks, typing and changes here first.

import { attempt, done, state, toast } from "./state.js";
import { plural } from "./ui.js";
import { t } from "./i18n.js";
import { TEMPLATES, autoSkills, findSkill, importMatches, skillsFor } from "./views-skills.js";

const FIELDS = {
  "skill-name": "name",
  "skill-description": "description",
  "skill-body": "body",
};

/// A draft's skills for its agent and project: the automatic ones until the
/// person picks, then what they picked that still applies.
export function fitSkills(dialog, agent, projectId) {
  if (!dialog) return;
  if (!dialog.skillsTouched) {
    dialog.skills = autoSkills(agent, projectId);
    return;
  }
  const allowed = new Set(skillsFor(agent, projectId).map((item) => item.id));
  dialog.skills = (dialog.skills ?? []).filter((id) => allowed.has(id));
}

export function installSkills({ render, openModal, closeDialog, openSettings, loadWorkspace }) {
  state.skills = [];
  state.skillsLoaded = false;
  const editing = () => state.dialog?.kind === "skill";

  async function load() {
    const list = await attempt("skills_list");
    state.skillsLoaded = true;
    if (list.ok) state.skills = list.value ?? [];
  }

  /// The agents' skills on this machine, to pick from: those of the
  /// project it was opened from are picked to begin with.
  async function openImport(projectId = null) {
    const found = await attempt("skills_discover");
    if (!found.ok) return toast(t(found.error), "error");
    openModal({
      kind: "skill-import",
      found: found.value ?? [],
      picked: (found.value ?? []).filter((item) => projectId && item.project_id === projectId).map((item) => item.path),
      everywhere: !projectId,
      projects: projectId ? [projectId] : [],
      roles: ["build"],
      auto: false,
    });
  }

  async function runImport() {
    const dialog = state.dialog;
    if (!dialog?.picked.length || dialog.busy) return;
    if (!dialog.everywhere && !dialog.projects.length) return toast(t("Pick the projects these skills are for."));
    dialog.busy = true;
    render();
    const count = await attempt("skills_import", {
      paths: dialog.picked,
      projects: dialog.everywhere ? [] : dialog.projects,
      roles: dialog.roles.some((role) => role !== "build") ? dialog.roles : null,
      auto: !!dialog.auto,
    });
    if (state.dialog !== dialog) return;
    dialog.busy = false;
    if (!count.ok) {
      toast(t(count.error), "error");
      return render();
    }
    closeDialog();
    await reload();
    toast(t("Imported {skills}.", { skills: plural(count.value, "skill") }));
  }

  /// Clicks inside the import dialog.
  function onImportClick(data) {
    const dialog = state.dialog;
    if (data.importPick) {
      dialog.picked = dialog.picked.includes(data.importPick)
        ? dialog.picked.filter((path) => path !== data.importPick)
        : [...dialog.picked, data.importPick];
    } else if (data.importAll) {
      // All or none of what the search shows; picks hidden by it stay.
      const shown = importMatches(dialog).map((item) => item.path);
      const every = shown.every((path) => dialog.picked.includes(path));
      dialog.picked = every
        ? dialog.picked.filter((path) => !shown.includes(path))
        : [...new Set([...dialog.picked, ...shown])];
    } else if (data.importScope) {
      dialog.everywhere = data.importScope === "everywhere";
    } else if (data.importProject) {
      // A folder of projects picks or drops its children with it.
      const id = data.importProject;
      const family = [id, ...state.projects.filter((item) => item.parent_id === id).map((item) => item.id)];
      const on = !dialog.projects.includes(id);
      dialog.projects = on
        ? [...new Set([...dialog.projects, ...family])]
        : dialog.projects.filter((other) => !family.includes(other));
      dialog.everywhere = false;
    } else if (data.importRole) {
      const roles = new Set(dialog.roles);
      if (roles.has(data.importRole)) roles.delete(data.importRole);
      else roles.add(data.importRole);
      dialog.roles = roles.size ? [...roles] : ["build"];
    } else if (data.importAuto) {
      dialog.auto = !dialog.auto;
    } else {
      return false;
    }
    render();
    return true;
  }

  async function reload() {
    await load();
    render();
  }

  function open(item, projectId = null) {
    openModal({
      kind: "skill",
      id: item?.id ?? null,
      name: item?.name ?? "",
      description: item?.description ?? "",
      body: item?.body ?? "",
      project_id: item ? (item.project_id ?? "") : (projectId ?? ""),
      agents: item?.agents ?? "both",
      auto: item?.auto ?? false,
      roles: item?.roles ?? ["build"],
      projects: item?.projects ?? null,
      preview: false,
    });
  }

  async function save() {
    const dialog = state.dialog;
    if (!editing() || dialog.busy) return;
    const input = {
      id: dialog.id,
      name: dialog.name,
      description: dialog.description,
      body: dialog.body,
      project_id: dialog.project_id || null,
      agents: dialog.agents,
      auto: !!dialog.auto,
      // The builder alone is what a skill always was: kept as none.
      roles: dialog.roles?.some((role) => role !== "build") ? dialog.roles : null,
      projects: dialog.projects?.length ? dialog.projects : null,
    };
    dialog.busy = true;
    render();
    const saved = await attempt("skill_save", { input });
    if (state.dialog !== dialog) return;
    dialog.busy = false;
    if (!saved.ok) {
      toast(t(saved.error), "error");
      return render();
    }
    closeDialog();
    toast(dialog.id ? t("Skill saved") : t("Skill created"));
    await reload();
  }

  async function addTemplate(key) {
    const template = TEMPLATES.find((item) => item.key === key);
    if (!template) return;
    const input = { id: null, name: t(template.name), description: t(template.description), body: t(template.body), project_id: null, agents: "both", auto: false };
    const saved = await attempt("skill_save", { input });
    if (!saved.ok) toast(t(saved.error), "error");
    else toast(t("Added “{name}”", { name: input.name }));
    await reload();
  }

  function remove(id) {
    const item = findSkill(id);
    openModal({
      kind: "confirm",
      title: t("Delete “{title}”?", { title: item?.name ?? "" }),
      body: t("It comes off every session, task and automation it is attached to. Running sessions keep it until they stop."),
      confirmLabel: t("Delete"),
      danger: true,
      run: async () => {
        if (await done("skill_delete", { id })) toast(t("Skill deleted"));
        await loadWorkspace();
        await reload();
      },
    });
  }

  /// A session's chip menu: switches one skill on or off, from its next start.
  async function toggleOnSession(id, skillId) {
    const lists = [state.sessions, state.allSessions].flatMap((list) => (list ?? []).filter((entry) => entry.id === id));
    const current = lists[0]?.skills ?? [];
    const next = current.includes(skillId) ? current.filter((other) => other !== skillId) : [...current, skillId];
    for (const entry of lists) entry.skills = next;
    render();
    if (!(await done("session_skills", { id, skills: next }))) {
      for (const entry of lists) entry.skills = current;
      render();
    }
  }

  /// A click anywhere. True when it was ours.
  async function onClick(target) {
    const data = target.dataset;
    if (data.skillToggle) {
      await toggleOnSession(data.skillSession, data.skillToggle);
      return true;
    }
    if (data.skillPick && state.dialog && Array.isArray(state.dialog.skills)) {
      const dialog = state.dialog;
      dialog.skillsTouched = true;
      dialog.skills = dialog.skills.includes(data.skillPick)
        ? dialog.skills.filter((id) => id !== data.skillPick)
        : [...dialog.skills, data.skillPick];
      render();
      return true;
    }
    if (state.dialog?.kind === "skill-import" && onImportClick(data)) return true;
    if (data.skill) {
      const act = data.skill;
      if (act === "import") return openImport(data.skillProject || null), true;
      if (act === "import-run") return runImport(), true;
      if (act === "new") return open(null, data.skillProject || null), true;
      if (act === "edit") return open(findSkill(data.skillId)), true;
      if (act === "delete") return remove(data.skillId), true;
      if (act === "template") return addTemplate(data.skillTemplate), true;
      if (act === "manage") {
        state.menu = null;
        state.dialog = null;
        await openSettings("Skills");
        return true;
      }
      if (!editing()) return false;
      if (act === "save") return save(), true;
      if (act === "write" || act === "preview") {
        state.dialog.preview = act === "preview";
        render();
        if (act === "write") document.querySelector("#skill-body")?.focus();
        return true;
      }
    }
    if (!editing()) return false;
    if (data.set === "skill-agents") {
      state.dialog.agents = data.value;
      render();
      return true;
    }
    if (data.toggle === "skill-auto") {
      state.dialog.auto = !state.dialog.auto;
      render();
      return true;
    }
    if (data.toggle?.startsWith("skill-role:")) {
      const role = data.toggle.slice("skill-role:".length);
      const roles = new Set(state.dialog.roles ?? ["build"]);
      if (roles.has(role)) roles.delete(role);
      else roles.add(role);
      // A skill is for at least one role; none would be the builder anyway.
      state.dialog.roles = roles.size ? [...roles] : ["build"];
      render();
      return true;
    }
    return false;
  }

  function onInput(field) {
    if (state.dialog?.kind === "skill-import" && field.id === "skill-import-search") {
      state.dialog.query = field.value;
      render();
      return true;
    }
    if (!editing() || !field.closest?.(".modal")) return false;
    const key = FIELDS[field.id];
    if (!key) return false;
    state.dialog[key] = field.value;
    if (key === "body") {
      const note = field.parentElement?.querySelector(".field__note");
      if (note) note.textContent = t("{count} of 16,000 characters", { count: field.value.length.toLocaleString() });
    }
    return true;
  }

  function onChange(field) {
    if (!editing()) return false;
    if (field.id === "skill-project") {
      // "Several projects" keeps the list it was imported with.
      if (field.value === "__several") return true;
      state.dialog.project_id = field.value;
      state.dialog.projects = null;
      return true;
    }
    return field.id in FIELDS ? onInput(field) : false;
  }

  return { load, reload, open, onClick, onInput, onChange };
}
