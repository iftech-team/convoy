// Inline SVG rather than an icon font: a few dozen glyphs do not justify a
// download, and `currentColor` makes them follow the theme for free.
const icon = (paths, size = 16) =>
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
  warn: icon('<path d="M8 2.8 14 13H2z"/><path d="M8 6.8v2.6M8 11.3v.1"/>', 14),
};
