// Tabs grouped by project, for the tab strip and the vertical tab column.
// Pure: no DOM, no state, so the rules can be tested on their own.

/// The tabs as one group per project, in the order each project first
/// appears, keeping the order of tabs within a project. Tabs with no project
/// share one group whose projectId is null.
export function groupTabs(tabs, projectOf) {
  const groups = [];
  const byProject = new Map();
  for (const id of tabs) {
    const projectId = projectOf(id) ?? null;
    let group = byProject.get(projectId);
    if (!group) {
      group = { projectId, tabs: [] };
      byProject.set(projectId, group);
      groups.push(group);
    }
    group.tabs.push(id);
  }
  return groups;
}

/// The tabs reordered so each project's tabs sit together, which is how they
/// are drawn once grouped, with the group keyed `first` (pinned tabs) ahead
/// of the rest. Returns the same array when nothing moves.
export function normalizeOrder(tabs, projectOf, first) {
  const groups = groupTabs(tabs, projectOf);
  const lead = first === undefined ? [] : groups.filter((group) => group.projectId === first);
  const order = [...lead, ...groups.filter((group) => !lead.includes(group))].flatMap((group) => group.tabs);
  return order.every((id, index) => id === tabs[index]) ? tabs : order;
}

/// A whole project's tabs moved to just before another project's, or to the
/// end when `before` is null. Each project's own order is kept.
export function moveGroup(tabs, projectOf, key, before) {
  const groups = groupTabs(tabs, projectOf);
  const moving = groups.find((group) => group.projectId === key);
  if (!moving || key === before) return tabs;
  const rest = groups.filter((group) => group !== moving);
  const at = before === null ? -1 : rest.findIndex((group) => group.projectId === before);
  rest.splice(at < 0 ? rest.length : at, 0, moving);
  return rest.flatMap((group) => group.tabs);
}

/// The tabs that are on screen, in the order shown: a folded group's tabs are
/// left out, except the tab being looked at, so it never disappears.
export function visibleTabs(groups, collapsed, currentId) {
  return groups.flatMap((group) =>
    group.projectId !== null && collapsed.has(group.projectId)
      ? group.tabs.filter((id) => id === currentId)
      : group.tabs,
  );
}

/// Where a dragged tab may land when tabs are grouped: the first and last
/// index (among all tabs) of its own group.
export function groupBounds(tabs, projectOf, id) {
  const projectId = projectOf(id) ?? null;
  let first = -1;
  let last = -1;
  tabs.forEach((tab, index) => {
    if ((projectOf(tab) ?? null) !== projectId) return;
    if (first < 0) first = index;
    last = index;
  });
  return [first, last];
}
