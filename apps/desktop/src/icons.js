// Inline SVG glyphs that mirror the SF Symbols used by the native macOS app.
const stroke = (body, extra = '') => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"${extra}>${body}</svg>`;
const fill = body => `<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">${body}</svg>`;

export const icons = {
  brand: fill('<path d="M12 2 2 7l10 5 10-5-10-5z"/><path d="m2 12 10 5 10-5-2-1-8 4-8-4-2 1z" opacity=".7"/><path d="m2 17 10 5 10-5-2-1-8 4-8-4-2 1z" opacity=".45"/>'),
  sidebar: stroke('<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M9 4v16"/>'),
  sidebarRight: stroke('<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M15 4v16"/>'),
  house: stroke('<path d="M3 11 12 4l9 7"/><path d="M5 10v10h14V10"/><path d="M10 20v-6h4v6"/>'),
  plus: stroke('<path d="M12 5v14M5 12h14"/>', ' stroke-width="2.4"'),
  x: stroke('<path d="M6 6l12 12M18 6 6 18"/>', ' stroke-width="2.4"'),
  gear: stroke('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),
  sliders: stroke('<path d="M4 6h10M18 6h2M4 12h2M10 12h10M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="8" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>'),
  search: stroke('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
  folder: stroke('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/>'),
  folderFill: fill('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/>'),
  folderPlus: stroke('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/><path d="M12 10v6M9 13h6"/>'),
  chevronRight: stroke('<path d="m9 6 6 6-6 6"/>', ' stroke-width="2.6"'),
  chevronDown: stroke('<path d="m6 9 6 6 6-6"/>', ' stroke-width="2.6"'),
  chevronUpDown: stroke('<path d="m8 9 4-4 4 4M8 15l4 4 4-4"/>'),
  terminal: stroke('<rect x="3" y="4" width="18" height="16" rx="3"/><path d="m7 9 3 3-3 3M12 15h5"/>'),
  bolt: fill('<path d="M13 2 4 14h6l-1 8 9-12h-6l1-8z"/>'),
  ellipsis: fill('<circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/>'),
  branch: stroke('<circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="8" r="2.2"/><path d="M6 7.2v9.6M18 10.2c0 3.5-3 4.5-6 5-2.5.5-5 1.2-6 1.6"/>'),
  bell: stroke('<path d="M6 16V11a6 6 0 1 1 12 0v5l2 2H4l2-2z"/><path d="M10 20a2 2 0 0 0 4 0"/>'),
  layoutOne: stroke('<rect x="3" y="5" width="18" height="14" rx="3"/>'),
  layoutTwo: stroke('<rect x="3" y="5" width="18" height="14" rx="3"/><path d="M12 5v14"/>'),
  book: stroke('<path d="M4 4h6a3 3 0 0 1 3 3v13a2 2 0 0 0-2-2H4V4z"/><path d="M20 4h-6a3 3 0 0 0-3 3v13a2 2 0 0 1 2-2h7V4z"/>'),
  doc: stroke('<path d="M6 3h8l4 4v14H6V3z"/><path d="M14 3v4h4M9 12h6M9 16h6"/>'),
  checklist: stroke('<path d="m4 6 1.5 1.5L8 5M4 12l1.5 1.5L8 11M4 18l1.5 1.5L8 17M11 6h9M11 12h9M11 18h9"/>'),
  clock: stroke('<circle cx="12" cy="12" r="8"/><path d="M12 8v4l3 2"/>'),
  clockBack: stroke('<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>'),
  check: stroke('<path d="m5 12 4 4L19 6"/>', ' stroke-width="2.6"'),
  checkCircle: fill('<path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm-1.5 14.5-4-4 1.4-1.4 2.6 2.6 5.6-5.6 1.4 1.4-7 7z"/>'),
  play: fill('<path d="M7 5v14l12-7z"/>'),
  stop: fill('<rect x="6" y="6" width="12" height="12" rx="2"/>'),
  person: stroke('<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>'),
  keyboard: stroke('<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6"/>'),
  command: stroke('<path d="M9 9V6a3 3 0 1 0-3 3h3zm0 0v6m0-6h6m-6 6H6a3 3 0 1 0 3 3v-3zm6-6V6a3 3 0 1 1 3 3h-3zm0 0v6m0 0h3a3 3 0 1 1-3 3v-3z"/>'),
  chart: stroke('<path d="M4 20V10M10 20V4M16 20v-8M22 20H2"/>'),
  sparkles: fill('<path d="m12 2 2 6 6 2-6 2-2 6-2-6-6-2 6-2 2-6zM19 14l1 3 3 1-3 1-1 3-1-3-3-1 3-1 1-3zM5 15l.8 2.2L8 18l-2.2.8L5 21l-.8-2.2L2 18l2.2-.8L5 15z"/>'),
  pin: fill('<path d="M15 3h-6l1 6-3 3v2h4v6l1 1 1-1v-6h4v-2l-3-3 1-6z"/>'),
  refresh: stroke('<path d="M20 12a8 8 0 1 1-2.3-5.7L20 8"/><path d="M20 3v5h-5"/>'),
  square: stroke('<rect x="4" y="4" width="16" height="16" rx="3"/>'),
  squareCheck: fill('<path d="M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zm5.5 13.5 7-7-1.4-1.4-5.6 5.6-2.6-2.6-1.4 1.4 4 4z"/>'),
  diffSplit: stroke('<rect x="3" y="5" width="18" height="14" rx="3"/><path d="M12 5v14"/>'),
  wrap: stroke('<path d="M4 6h16M4 12h11a3 3 0 0 1 0 6h-3"/><path d="m14 15-2 3 2 3M4 18h4"/>'),
  info: stroke('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'),
  warning: fill('<path d="M12 2 1 21h22L12 2zm1 15h-2v-2h2v2zm0-4h-2V9h2v4z"/>'),
  cup: stroke('<path d="M4 8h13v6a5 5 0 0 1-5 5H9a5 5 0 0 1-5-5V8z"/><path d="M17 10h2a2 2 0 0 1 0 4h-2M3 21h16"/>'),
  moon: fill('<path d="M21 13.5A8.5 8.5 0 0 1 10.5 3a7 7 0 1 0 10.5 10.5z"/>'),
  arrowLeft: stroke('<path d="M19 12H5m6-6-6 6 6 6"/>'),
  externalLink: stroke('<path d="M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"/>'),
  copy: stroke('<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3"/>'),
  claude: '<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="#D97757" fill-rule="nonzero" d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z"/></svg>',
  codex: '<svg viewBox="0 0 24 24" aria-hidden="true"><path fill="currentColor" d="M9.205 8.658v-2.26c0-.19.072-.333.238-.428l4.543-2.616c.619-.357 1.356-.523 2.117-.523 2.854 0 4.662 2.212 4.662 4.566 0 .167 0 .357-.024.547l-4.71-2.759a.797.797 0 00-.856 0l-5.97 3.473zm10.609 8.8V12.06c0-.333-.143-.57-.429-.737l-5.97-3.473 1.95-1.118a.433.433 0 01.476 0l4.543 2.617c1.309.76 2.189 2.378 2.189 3.948 0 1.808-1.07 3.473-2.76 4.163zM7.802 12.703l-1.95-1.142c-.167-.095-.239-.238-.239-.428V5.899c0-2.545 1.95-4.472 4.591-4.472 1 0 1.927.333 2.712.928L8.23 5.067c-.285.166-.428.404-.428.737v6.898zM12 15.128l-2.795-1.57v-3.33L12 8.658l2.795 1.57v3.33L12 15.128zm1.796 7.23c-1 0-1.927-.332-2.712-.927l4.686-2.712c.285-.166.428-.404.428-.737v-6.898l1.974 1.142c.167.095.238.238.238.428v5.233c0 2.545-1.974 4.472-4.614 4.472zm-5.637-5.303l-4.544-2.617c-1.308-.761-2.188-2.378-2.188-3.948A4.482 4.482 0 014.21 6.327v5.423c0 .333.143.571.428.738l5.947 3.449-1.95 1.118a.432.432 0 01-.476 0zm-.262 3.9c-2.688 0-4.662-2.021-4.662-4.519 0-.19.024-.38.047-.57l4.686 2.71c.286.167.571.167.856 0l5.97-3.448v2.26c0 .19-.07.333-.237.428l-4.543 2.616c-.619.357-1.356.523-2.117.523zm5.899 2.83a5.947 5.947 0 005.827-4.756C22.287 18.339 24 15.84 24 13.296c0-1.665-.713-3.282-1.998-4.448.119-.5.19-.999.19-1.498 0-3.401-2.759-5.947-5.946-5.947-.642 0-1.26.095-1.88.31A5.962 5.962 0 0010.205 0a5.947 5.947 0 00-5.827 4.757C1.713 5.447 0 7.945 0 10.49c0 1.666.713 3.283 1.998 4.448-.119.5-.19 1-.19 1.499 0 3.401 2.759 5.946 5.946 5.946.642 0 1.26-.095 1.88-.309a5.96 5.96 0 004.162 1.713z"/></svg>'
};

/** Returns a span carrying the named glyph. `size` is the box in px. */
export function icon(name, size = 12, className = '') {
  const node = document.createElement('span');
  node.className = `icon ${className}`.trim();
  node.style.setProperty('--icon-size', `${size}px`);
  node.innerHTML = icons[name] || '';
  return node;
}

export function agentIcon(agent, size = 12, className = '') {
  const node = icon(agent === 'claude' ? 'claude' : 'codex', size, `agent-icon ${className}`);
  node.title = agent === 'claude' ? 'Claude Code' : 'Codex';
  return node;
}

/** Deterministic project tint, matching the native djb2 palette. */
const palette = ['#5E6AD2', '#4CAF83', '#E5A54B', '#D65C5C', '#3FA7D6', '#B067C9', '#8A8F98', '#E07C4C'];
export function projectTint(name = '') {
  let hash = 5381;
  for (const byte of new TextEncoder().encode(name)) hash = (Math.imul(hash, 33) + byte) >>> 0;
  return palette[hash % palette.length];
}

/** Replaces `data-icon` placeholders in static markup with glyphs. */
export function hydrateIcons(root = document) {
  for (const node of root.querySelectorAll('[data-icon]')) {
    const size = Number(node.dataset.iconSize || 12);
    node.replaceChildren(icon(node.dataset.icon, size));
    if (node.dataset.label) { const text = document.createElement('span'); text.textContent = node.dataset.label; node.append(text); }
  }
  for (const node of root.querySelectorAll('.agent-tile[data-agent]')) node.replaceChildren(agentIcon(node.dataset.agent, 22));
}
