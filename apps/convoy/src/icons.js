// Inline SVG rather than an icon font: a few dozen glyphs do not justify a
// download, and `currentColor` makes them follow the theme for free.
export const icon = (paths, size = 16) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 16 16" fill="none"
        stroke="currentColor" stroke-width="1.5" stroke-linecap="round"
        stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

export const icons = {
  search: icon('<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/>', 14),
  plus: icon('<path d="M8 3.5v9M3.5 8h9"/>'),
  minus: icon('<path d="M3.5 8h9"/>', 14),
  gear: icon(
    '<circle cx="8" cy="8" r="2"/>' +
      '<path d="M12.9 9.6a1.1 1.1 0 0 0 .2 1.2l.1.1a1.3 1.3 0 1 1-1.9 1.9l-.1-.1a1.1 1.1 0 0 0-1.2-.2 1.1 1.1 0 0 0-.7 1v.2a1.3 1.3 0 0 1-2.6 0v-.1a1.1 1.1 0 0 0-.7-1 1.1 1.1 0 0 0-1.2.2l-.1.1a1.3 1.3 0 1 1-1.9-1.9l.1-.1a1.1 1.1 0 0 0 .2-1.2 1.1 1.1 0 0 0-1-.7h-.2a1.3 1.3 0 0 1 0-2.6h.1a1.1 1.1 0 0 0 1-.7 1.1 1.1 0 0 0-.2-1.2l-.1-.1a1.3 1.3 0 1 1 1.9-1.9l.1.1a1.1 1.1 0 0 0 1.2.2h.1a1.1 1.1 0 0 0 .7-1v-.2a1.3 1.3 0 0 1 2.6 0v.1a1.1 1.1 0 0 0 .7 1 1.1 1.1 0 0 0 1.2-.2l.1-.1a1.3 1.3 0 1 1 1.9 1.9l-.1.1a1.1 1.1 0 0 0-.2 1.2v.1a1.1 1.1 0 0 0 1 .7h.2a1.3 1.3 0 0 1 0 2.6h-.1a1.1 1.1 0 0 0-1 .7z"/>'
  ),
  refresh: icon('<path d="M13.5 8a5.5 5.5 0 1 1-1.7-4"/><path d="M13.8 1.8V4h-2.2"/>', 14),
  open: icon('<path d="M9.5 2.5H13V6"/><path d="M13 2.5 7.5 8"/><path d="M12 9.5V13H3V4h3.5"/>', 14),
  play: icon('<path d="M5 3.2 12 8l-7 4.8z" fill="currentColor" stroke="none"/>', 14),
  stop: icon('<rect x="4.5" y="4.5" width="7" height="7" rx="1.5" fill="currentColor" stroke="none"/>', 14),
  terminal: icon('<path d="M3 4.5 6 8l-3 3.5M8 11.5h5"/>', 14),
  copy: icon('<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.5h-8v8"/>', 14),
  folder: icon('<path d="M1.8 4.2h4.1l1.2 1.5h7.1v6.6a.8.8 0 0 1-.8.8H2.6a.8.8 0 0 1-.8-.8z"/>'),
  review: icon('<path d="M2.5 3.2h11v7.3H8.8L6 13.2v-2.7H2.5z"/><path d="m5.6 6.8 1.4 1.4 2.9-2.9"/>', 22),
  awake: icon('<circle cx="8" cy="8" r="3.4"/><path d="M8 1.6v1.4M8 13v1.4M14.4 8H13M3 8H1.6"/>', 14),
  limits: icon('<path d="M2.5 13V9.5M6.2 13V4M9.8 13V7M13.5 13V2.5"/>', 14),
  chevron: icon('<path d="m6 4 4 4-4 4"/>', 14),
  back: icon('<path d="m10 4-4 4 4 4"/>', 14),
  more: icon('<circle cx="3.5" cy="8" r="1.2" fill="currentColor" stroke="none"/><circle cx="8" cy="8" r="1.2" fill="currentColor" stroke="none"/><circle cx="12.5" cy="8" r="1.2" fill="currentColor" stroke="none"/>', 16),
  bolt: icon('<path d="M9 1.5 3.5 9H7.5L7 14.5 12.5 7H8.5z"/>', 14),
  git: icon('<circle cx="4" cy="4" r="1.8"/><circle cx="4" cy="12" r="1.8"/><circle cx="12" cy="8" r="1.8"/><path d="M4 5.8v4.4M5.8 4h2.4a2 2 0 0 1 2 2v.3"/>', 14),
  diff: icon('<path d="M4.5 2.5v11M2.5 6h4M9.5 10h4M11.5 8v4"/>', 22),
  list: icon('<path d="M3 4.5h10M3 8h10M3 11.5h6"/>', 22),
  board: icon('<rect x="2.5" y="2.5" width="4" height="11" rx="1"/><rect x="9.5" y="2.5" width="4" height="7" rx="1"/>', 22),
  trash: icon('<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.2a.8.8 0 0 0 .8.8h4.2a.8.8 0 0 0 .8-.8l.6-8.2"/>', 14),
  split: icon('<rect x="2.5" y="3" width="11" height="10" rx="1.2"/><path d="M8 3v10"/>', 14),
  inbox: icon('<path d="M2.5 9.5h3l1 2h3l1-2h3"/><path d="M3.8 3h8.4l1.3 6.5v3H2.5v-3z"/>', 22),
  clock: icon('<circle cx="8" cy="8" r="5.5"/><path d="M8 5v3.2l2 1.2"/>', 14),
  person: icon('<circle cx="8" cy="5.5" r="2.5"/><path d="M3 13.2a5 5 0 0 1 10 0"/>', 14),
  download: icon('<path d="M8 2.5v7.5M5 7.5 8 10.5l3-3M3 13h10"/>', 14),
  close: icon('<path d="M4 4l8 8M12 4l-8 8"/>', 14),
  check: icon('<path d="m3.5 8.5 3 3 6-7"/>', 14),
  chevronDown: icon('<path d="m4 6 4 4 4-4"/>', 12),
  chevronRight: icon('<path d="m6 4 4 4-4 4"/>', 12),
  pin: icon('<path d="M9.8 2.5 13.5 6.2l-2 .6-2.6 2.6.3 2.6-1 1-2-2.6-3 3M6.4 7.8l2.6-2.6.6-2"/>', 12),
  moon: icon('<path d="M13 9.5A5.5 5.5 0 0 1 6.5 3a5.5 5.5 0 1 0 6.5 6.5z"/>', 14),
  link: icon('<path d="M7 9a2.5 2.5 0 0 0 3.5 0l2-2A2.5 2.5 0 0 0 9 3.5l-.7.7M9 7a2.5 2.5 0 0 0-3.5 0l-2 2A2.5 2.5 0 0 0 7 12.5l.7-.7"/>', 14),
  history: icon('<path d="M2.5 8a5.5 5.5 0 1 0 1.6-3.9"/><path d="M2.3 2.5v2.2h2.2M8 5.2v3l2 1.2"/>', 14),
  edit: icon('<path d="M10.5 3 13 5.5 6 12.5H3.5V10z"/>', 14),
  archive: icon('<rect x="2.5" y="3" width="11" height="3" rx=".8"/><path d="M3.5 6v6.5h9V6M6.5 8.5h3"/>', 14),
  branch: icon('<circle cx="4.5" cy="3.5" r="1.4"/><circle cx="4.5" cy="12.5" r="1.4"/><circle cx="11.5" cy="5" r="1.4"/><path d="M4.5 4.9v6.2M11.5 6.4c0 2.4-2 3-4.5 3.4-1.3.2-2.5.6-2.5 1.3"/>', 10),
  sliders: icon('<path d="M3 4.5h6M12 4.5h1M3 11.5h1M7 11.5h6"/><circle cx="10.5" cy="4.5" r="1.5"/><circle cx="5.5" cy="11.5" r="1.5"/>', 14),
  paint: icon('<path d="M8 2.5a5.5 5.5 0 1 0 0 11c.9 0 1.2-.7.9-1.4-.4-.8.1-1.6 1-1.6h1.3a2.3 2.3 0 0 0 2.3-2.3c0-3.2-2.5-5.7-5.5-5.7z"/><circle cx="5.3" cy="7" r=".7" fill="currentColor"/><circle cx="8" cy="5.2" r=".7" fill="currentColor"/><circle cx="10.7" cy="7" r=".7" fill="currentColor"/>', 14),
  sparkles: icon('<path d="M6.5 2.5 7.6 6 11 7l-3.4 1.1L6.5 11.5 5.4 8.1 2 7l3.4-1z"/><path d="M12 10.5l.5 1.5 1.5.5-1.5.5-.5 1.5-.5-1.5-1.5-.5 1.5-.5z"/>', 14),
  keyboard: icon('<rect x="1.8" y="4" width="12.4" height="8" rx="1.3"/><path d="M4.5 6.5h.1M7 6.5h.1M9.5 6.5h.1M12 6.5h-.1M5 9.5h6"/>', 14),
  bell: icon('<path d="M4 11V7.5a4 4 0 0 1 8 0V11l1 1.2H3z"/><path d="M6.8 13.8a1.3 1.3 0 0 0 2.4 0"/>', 14),
  sidebar: icon('<rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M6 3v10"/>', 14),
  checkCircle: icon('<circle cx="8" cy="8" r="6" fill="currentColor" stroke="none"/><path d="m5.5 8.2 1.7 1.7 3.3-3.6" stroke="#fff"/>', 13),
  paneOne: icon('<rect x="2.5" y="3" width="11" height="10" rx="1.2"/>', 14),
  home: icon('<path d="M2.5 7.5 8 3l5.5 4.5V13H9.8V9.8H6.2V13H2.5z"/>', 14),
  book: icon('<path d="M3 2.8h4a1.5 1.5 0 0 1 1 .5 1.5 1.5 0 0 1 1-.5h4v10H9a1 1 0 0 0-1 .9 1 1 0 0 0-1-.9H3z"/><path d="M8 3.3v10.4"/>', 14),
  doc: icon('<path d="M3.5 1.8h6l3 3v9.4h-9z"/><path d="M9.5 1.8v3h3M5.5 8h5M5.5 10.5h5"/>', 13),
  page: icon('<path d="M3.5 1.8h6l3 3v9.4h-9z"/><path d="M9.5 1.8v3h3"/>', 13),
  bell: icon('<path d="M4 11V7.5a4 4 0 0 1 8 0V11l1 1.2H3z"/><path d="M6.8 13.8a1.3 1.3 0 0 0 2.4 0"/>', 14),
  warn: icon('<path d="M8 2.8 14 13H2z"/><path d="M8 6.8v2.6M8 11.3v.1"/>', 14),
};

// The agents' own marks — the same artwork the macOS app uses. Claude keeps
// its colour; Codex follows the text colour, as a template image does there.
const CLAUDE_PATH = "M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z";
const CODEX_PATH = "M9.205 8.658v-2.26c0-.19.072-.333.238-.428l4.543-2.616c.619-.357 1.356-.523 2.117-.523 2.854 0 4.662 2.212 4.662 4.566 0 .167 0 .357-.024.547l-4.71-2.759a.797.797 0 00-.856 0l-5.97 3.473zm10.609 8.8V12.06c0-.333-.143-.57-.429-.737l-5.97-3.473 1.95-1.118a.433.433 0 01.476 0l4.543 2.617c1.309.76 2.189 2.378 2.189 3.948 0 1.808-1.07 3.473-2.76 4.163zM7.802 12.703l-1.95-1.142c-.167-.095-.239-.238-.239-.428V5.899c0-2.545 1.95-4.472 4.591-4.472 1 0 1.927.333 2.712.928L8.23 5.067c-.285.166-.428.404-.428.737v6.898zM12 15.128l-2.795-1.57v-3.33L12 8.658l2.795 1.57v3.33L12 15.128zm1.796 7.23c-1 0-1.927-.332-2.712-.927l4.686-2.712c.285-.166.428-.404.428-.737v-6.898l1.974 1.142c.167.095.238.238.238.428v5.233c0 2.545-1.974 4.472-4.614 4.472zm-5.637-5.303l-4.544-2.617c-1.308-.761-2.188-2.378-2.188-3.948A4.482 4.482 0 014.21 6.327v5.423c0 .333.143.571.428.738l5.947 3.449-1.95 1.118a.432.432 0 01-.476 0zm-.262 3.9c-2.688 0-4.662-2.021-4.662-4.519 0-.19.024-.38.047-.57l4.686 2.71c.286.167.571.167.856 0l5.97-3.448v2.26c0 .19-.07.333-.237.428l-4.543 2.616c-.619.357-1.356.523-2.117.523zm5.899 2.83a5.947 5.947 0 005.827-4.756C22.287 18.339 24 15.84 24 13.296c0-1.665-.713-3.282-1.998-4.448.119-.5.19-.999.19-1.498 0-3.401-2.759-5.947-5.946-5.947-.642 0-1.26.095-1.88.31A5.962 5.962 0 0010.205 0a5.947 5.947 0 00-5.827 4.757C1.713 5.447 0 7.945 0 10.49c0 1.666.713 3.283 1.998 4.448-.119.5-.19 1-.19 1.499 0 3.401 2.759 5.946 5.946 5.946.642 0 1.26-.095 1.88-.309a5.96 5.96 0 004.162 1.713z";

export const agentIcon = (agent, size = 14) =>
  agent === "claude"
    ? `<svg class="agent-icon" width="${size}" height="${size}" viewBox="0 0 24 24" aria-label="Claude Code"><path fill="#D97757" d="${CLAUDE_PATH}"/></svg>`
    : `<svg class="agent-icon" width="${size}" height="${size}" viewBox="0 0 24 24" aria-label="Codex"><path fill="currentColor" d="${CODEX_PATH}"/></svg>`;

// The macOS palette, and the same name hash, so a project keeps its colour
// on every platform.
const PALETTE = ["#5E6AD2", "#4CAF83", "#E5A54B", "#D65C5C", "#3FA7D6", "#B067C9", "#8A8F98", "#E07C4C"];

export function projectTint(project) {
  if (project.color) return project.color;
  let hash = 5381n;
  for (const byte of new TextEncoder().encode(project.title ?? "")) {
    hash = (hash * 33n + BigInt(byte)) & 0xffffffffffffffffn;
  }
  return PALETTE[Number(hash % BigInt(PALETTE.length))];
}

/// The macOS app's symbol choices, keyed by their SF Symbols names so a
/// project's `sf:` icon means the same thing in either app.
export const SYMBOLS = {
  folder: '<path d="M1.8 4.2h4.1l1.2 1.5h7.1v6.6a.8.8 0 0 1-.8.8H2.6a.8.8 0 0 1-.8-.8z"/>',
  shippingbox: '<path d="M2.5 5 8 2.5 13.5 5v6L8 13.5 2.5 11z"/><path d="M2.5 5 8 7.5 13.5 5M8 7.5v6M5.2 3.8l5.5 2.5"/>',
  cart: '<path d="M1.8 2.5h2l1.5 7.5h7l1.4-5.5H4.3"/><circle cx="6" cy="12.8" r="1"/><circle cx="11.5" cy="12.8" r="1"/>',
  creditcard: '<rect x="1.8" y="3.5" width="12.4" height="9" rx="1.3"/><path d="M1.8 6.5h12.4M4 10h3"/>',
  iphone: '<rect x="4.5" y="1.8" width="7" height="12.4" rx="1.5"/><path d="M7 3.5h2"/>',
  globe: '<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"/>',
  "server.rack": '<rect x="2.5" y="2.5" width="11" height="4.5" rx="1"/><rect x="2.5" y="9" width="11" height="4.5" rx="1"/><path d="M5 4.8h.1M5 11.3h.1"/>',
  cpu: '<rect x="4" y="4" width="8" height="8" rx="1"/><rect x="6.3" y="6.3" width="3.4" height="3.4"/><path d="M6 2v2M10 2v2M6 12v2M10 12v2M2 6h2M2 10h2M12 6h2M12 10h2"/>',
  terminal: '<rect x="1.8" y="2.5" width="12.4" height="11" rx="1.3"/><path d="m4.5 6 2 2-2 2M8 10.5h3.5"/>',
  gearshape: '<circle cx="8" cy="8" r="2"/><path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M3.6 12.4l1.2-1.2M11.2 4.8l1.2-1.2"/>',
  "building.2": '<path d="M2 14V4.5l5-2V14M7 6.5l7 1.5V14M1.5 14h13M4 6h1M4 8.5h1M4 11h1M9.5 10h1.5M9.5 12h1.5"/>',
  "truck.box": '<path d="M1.5 4h8v7h-8zM9.5 6.5h2.7l2.3 2.5V11h-5"/><circle cx="4.5" cy="12" r="1.2"/><circle cx="11.5" cy="12" r="1.2"/>',
  "chart.bar": '<path d="M2 14h12M3.5 12V8M7 12V4M10.5 12V6.5M14 12V9.5"/>',
  "lock.shield": '<path d="M8 1.8 13 3.8v4c0 3-2.2 5.2-5 6.4-2.8-1.2-5-3.4-5-6.4v-4z"/><rect x="6" y="7.5" width="4" height="3.2" rx=".6"/><path d="M6.8 7.5V6.5a1.2 1.2 0 0 1 2.4 0v1"/>',
  "doc.text": '<path d="M3.5 1.8h6l3 3v9.4h-9z"/><path d="M9.5 1.8v3h3M5.5 8h5M5.5 10.5h5"/>',
  network: '<circle cx="8" cy="3.5" r="1.7"/><circle cx="3.5" cy="12.5" r="1.7"/><circle cx="12.5" cy="12.5" r="1.7"/><path d="M8 5.2v3M8 8.2 4.5 11M8 8.2l3.5 2.8"/>',
  cloud: '<path d="M4.5 12.5a3 3 0 0 1-.3-6A4 4 0 0 1 12 6.2a3.2 3.2 0 0 1 .4 6.3z"/>',
  "wrench.and.screwdriver": '<path d="M9.5 2.2a3 3 0 0 0 3.8 3.8L6 13.3a1.4 1.4 0 0 1-2-2l7.3-7.3M2.5 2.5l3 3M2 3.5 3.5 2"/>',
};

/// Image icons are read through the backend once and kept as data URLs.
const images = new Map();
const pending = new Set();
let onImage = () => {};
export const onIconLoaded = (handler) => {
  onImage = handler;
};

function imageFor(path) {
  if (images.has(path)) return images.get(path);
  if (!pending.has(path)) {
    pending.add(path);
    import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("icon_data", { path }))
      .then((url) => images.set(path, url))
      .catch(() => images.set(path, null))
      .finally(() => {
        pending.delete(path);
        onImage();
      });
  }
  return undefined;
}

export const forgetIcon = (path) => images.delete(path);

/**
 * The project's icon, as the macOS app draws it: an image (GitHub avatar,
 * favicon, upload), a symbol, an emoji, or a folder in its colour. A chosen
 * colour shows as a dot on anything but the folder.
 */
export function projectIcon(project, size = 15) {
  const value = project.icon ?? "";
  const tint = projectTint(project);
  const dot = project.color && value ? `<span class="project-icon__dot" style="background:${project.color}"></span>` : "";
  let inner;
  if (/^(gh|img):/.test(value)) {
    const url = imageFor(value.replace(/^(gh|img):/, ""));
    inner = url
      ? `<img src="${url}" width="${size}" height="${size}" alt="" />`
      : icon(SYMBOLS.folder, size);
  } else if (value.startsWith("sf:")) {
    inner = icon(SYMBOLS[value.slice(3)] ?? SYMBOLS.folder, size);
  } else if (value) {
    inner = `<span style="font-size:${size - 1}px">${value.replace(/[&<>"']/g, "")}</span>`;
  } else {
    inner = icon(SYMBOLS.folder, size);
  }
  return `<span class="project-icon" style="color:${tint};width:${size + 2}px">${inner}${dot}</span>`;
}

/// The app icon — the same artwork as the macOS app's AppIcon.svg: three
/// chevrons in convoy on the violet squircle, without the drop shadow that
/// only reads at dock size.
export const appMark = (size = 22) => `
  <svg width="${size}" height="${size}" viewBox="100 100 824 824" aria-hidden="true">
    <defs>
      <linearGradient id="convoy-mark" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#6B78E6"/><stop offset="1" stop-color="#3D47A8"/>
      </linearGradient>
    </defs>
    <rect x="100" y="100" width="824" height="824" rx="186" fill="url(#convoy-mark)"/>
    <g fill="none" stroke="#fff" stroke-linecap="round" stroke-linejoin="round" stroke-width="86">
      <path d="M300 340 L472 512 L300 684" stroke-opacity="0.45"/>
      <path d="M440 340 L612 512 L440 684" stroke-opacity="0.7"/>
      <path d="M580 340 L752 512 L580 684"/>
    </g>
  </svg>`;
