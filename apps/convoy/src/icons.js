// Inline SVG rather than an icon font: two dozen glyphs do not justify a
// download, and `currentColor` makes them follow the theme for free.
const icon = (paths, size = 16) =>
  `<svg width="${size}" height="${size}" viewBox="0 0 16 16" fill="none"
        stroke="currentColor" stroke-width="1.5" stroke-linecap="round"
        stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

export const icons = {
  search: icon('<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/>', 14),
  plus: icon('<path d="M8 3.5v9M3.5 8h9"/>'),
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
  sparkle: icon('<path d="M8 2v12M2 8h12M3.8 3.8l8.4 8.4M12.2 3.8l-8.4 8.4"/>', 14),
  awake: icon('<circle cx="8" cy="8" r="3.4"/><path d="M8 1.6v1.4M8 13v1.4M14.4 8H13M3 8H1.6"/>', 14),
  limits: icon('<path d="M2.5 13V9.5M6.2 13V4M9.8 13V7M13.5 13V2.5"/>', 14),
  chevron: icon('<path d="m6 4 4 4-4 4"/>', 14),
};
