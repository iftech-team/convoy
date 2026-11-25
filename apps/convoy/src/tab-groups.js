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
/// are drawn once grouped. Returns the same array when nothing moves.
export function normalizeOrder(tabs, projectOf) {
  const order = groupTabs(tabs, projectOf).flatMap((group) => group.tabs);
  return order.every((id, index) => id === tabs[index]) ? tabs : order;
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
