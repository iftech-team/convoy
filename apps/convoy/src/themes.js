// The window's colour themes: each colours the app and its terminals.
//
// A theme's `ui` sets the window's surfaces, borders, text and accent; the
// Convoy themes have none and keep the look in tokens.css. Its `colors` are
// the terminal's.
//
// xterm draws the 16 ANSI colours from its own built-in palette unless it is
// given one, and that palette is made for a black background: on a white
// terminal its "black" rows, pale cyan paths and light-grey text were the
// problem. Every theme here sets all 16, chosen for its own background.
//
// The app's appearance picks between two choices: the theme used when the
// window is light and the one used when it is dark. Both are kept per machine,
// like the interface zoom, and out of the workspace file the macOS app shares.

const ANSI = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
];
export const ANSI_NAMES = ANSI;

const palette = (values) => Object.fromEntries(ANSI.map((name, index) => [name, values[index]]));

/// `own`: made for Convoy, held to a contrast check in the tests. The others
/// are well-known open schemes kept as their authors drew them; xterm's
/// minimum contrast still lifts any colour that would be hard to read.
export const TERMINAL_THEMES = [
  {
    id: "convoy-light",
    name: "Convoy Light",
    kind: "light",
    own: true,
    colors: {
      background: "#ffffff",
      foreground: "#26262b",
      cursor: "#5b5bd6",
      cursorAccent: "#ffffff",
      selectionBackground: "rgba(91, 91, 214, 0.2)",
      ...palette([
        "#26262b", "#c4314b", "#287a3d", "#8f5f00", "#3b5bdb", "#9c36b5", "#0b7285", "#b8b8c0",
        "#6e6e78", "#d63a52", "#2f8a47", "#9e6a00", "#4263eb", "#ae3ec9", "#0c8599", "#e4e4e9",
      ]),
    },
  },
  {
    id: "daylight",
    name: "Daylight",
    kind: "light",
    own: true,
    ui: { bg: "#ffffff", subtle: "#f6f8fa", sunken: "#eff2f5", hover: "#f3f4f6", active: "#eaeef2", border: "#d0d7de", borderStrong: "#afb8c1", text: "#1f2328", muted: "#59636e", faint: "#8c959f", accent: "#0969da", accentText: "#0550ae" },
    colors: {
      background: "#ffffff",
      foreground: "#1f2328",
      cursor: "#0969da",
      cursorAccent: "#ffffff",
      selectionBackground: "rgba(9, 105, 218, 0.18)",
      ...palette([
        "#24292f", "#cf222e", "#116329", "#7d4e00", "#0969da", "#8250df", "#1b7c83", "#6e7781",
        "#57606a", "#a40e26", "#1a7f37", "#633c01", "#0550ae", "#6639ba", "#136061", "#8c959f",
      ]),
    },
  },
  {
    id: "paper",
    name: "Paper",
    kind: "light",
    own: true,
    ui: { bg: "#fbf8f1", subtle: "#f6f2e9", sunken: "#f1ece1", hover: "#efe9dc", active: "#e8e1d2", border: "#e3dccd", borderStrong: "#d2c9b6", text: "#2f2d29", muted: "#6f6a60", faint: "#a39d90", accent: "#a0611a", accentText: "#8a5014" },
    colors: {
      background: "#fbf8f1",
      foreground: "#3b3a36",
      cursor: "#8a5a00",
      cursorAccent: "#fbf8f1",
      selectionBackground: "rgba(138, 90, 0, 0.18)",
      ...palette([
        "#3b3a36", "#b3261e", "#44700d", "#875800", "#2f5f9e", "#8e3a8a", "#1f6f6f", "#c9c4b8",
        "#75716a", "#c7362b", "#527f17", "#9a6500", "#3a6db3", "#a2449d", "#267b7b", "#ebe6da",
      ]),
    },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    kind: "light",
    ui: { bg: "#fdf6e3", subtle: "#f8f1dc", sunken: "#eee8d5", hover: "#f3ecd7", active: "#e9e2cc", border: "#e4dcc4", borderStrong: "#d3cbb2", text: "#073642", muted: "#586e75", faint: "#839496", accent: "#268bd2", accentText: "#1f6fa8" },
    colors: {
      background: "#fdf6e3",
      foreground: "#586e75",
      cursor: "#586e75",
      cursorAccent: "#fdf6e3",
      selectionBackground: "rgba(147, 161, 161, 0.3)",
      ...palette([
        "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
      ]),
    },
  },
  {
    id: "one-light",
    name: "One Light",
    kind: "light",
    ui: { bg: "#fafafa", subtle: "#f0f0f1", sunken: "#e9e9eb", hover: "#ececed", active: "#e0e0e2", border: "#dbdbdc", borderStrong: "#c8c8ca", text: "#383a42", muted: "#696c77", faint: "#96979d", accent: "#4078f2", accentText: "#2f5fd0" },
    colors: {
      background: "#fafafa",
      foreground: "#383a42",
      cursor: "#526fff",
      cursorAccent: "#fafafa",
      selectionBackground: "rgba(64, 120, 242, 0.18)",
      ...palette([
        "#383a42", "#e45649", "#50a14f", "#c18401", "#4078f2", "#a626a4", "#0184bc", "#a0a1a7",
        "#696c77", "#e45649", "#50a14f", "#c18401", "#4078f2", "#a626a4", "#0184bc", "#ffffff",
      ]),
    },
  },
  {
    // Gruvbox puts its background in ANSI black on a light terminal; black
    // here is dark text and white a light shade, as programs expect.
    id: "gruvbox-light",
    name: "Gruvbox Light",
    kind: "light",
    ui: { bg: "#fbf1c7", subtle: "#f2e5bc", sunken: "#ebdbb2", hover: "#ebdbb2", active: "#d5c4a1", border: "#e0cfa9", borderStrong: "#d5c4a1", text: "#3c3836", muted: "#665c54", faint: "#928374", accent: "#af3a03", accentText: "#af3a03" },
    colors: {
      background: "#fbf1c7",
      foreground: "#3c3836",
      cursor: "#3c3836",
      cursorAccent: "#fbf1c7",
      selectionBackground: "rgba(124, 111, 100, 0.25)",
      ...palette([
        "#3c3836", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#d5c4a1",
        "#7c6f64", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#427b58", "#ebdbb2",
      ]),
    },
  },
  {
    // The window's text is a shade deeper than Dawn's own, which reads
    // light at the app's small sizes.
    id: "rose-pine-dawn",
    name: "Rosé Pine Dawn",
    kind: "light",
    ui: { bg: "#faf4ed", subtle: "#fffaf3", sunken: "#f2e9e1", hover: "#f4ede8", active: "#dfdad9", border: "#dfdad9", borderStrong: "#cecacd", text: "#464261", muted: "#6e6a86", faint: "#9893a5", accent: "#286983", accentText: "#286983" },
    colors: {
      background: "#faf4ed",
      foreground: "#575279",
      cursor: "#575279",
      cursorAccent: "#faf4ed",
      selectionBackground: "rgba(152, 147, 165, 0.25)",
      ...palette([
        "#575279", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e", "#dfdad9",
        "#797593", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e", "#f2e9e1",
      ]),
    },
  },
  {
    id: "convoy-dark",
    name: "Convoy Dark",
    kind: "dark",
    own: true,
    colors: {
      background: "#131316",
      foreground: "#d6d6dc",
      cursor: "#8b8bf0",
      cursorAccent: "#131316",
      selectionBackground: "rgba(110, 110, 230, 0.35)",
      ...palette([
        "#2a2a31", "#f2777a", "#7ccf8a", "#e5c07b", "#8b9cf7", "#d38bf0", "#6fd3d6", "#c9c9d1",
        "#7a7a85", "#ff9a9c", "#9be3a7", "#f2d59b", "#a9b6ff", "#e2a9f7", "#93e4e6", "#f4f4f7",
      ]),
    },
  },
  {
    id: "midnight",
    name: "Midnight",
    kind: "dark",
    own: true,
    ui: { bg: "#0d1117", subtle: "#10151c", sunken: "#161b22", hover: "#1b222c", active: "#232b36", border: "#21262d", borderStrong: "#30363d", text: "#e6edf3", muted: "#9198a1", faint: "#6e7681", accent: "#1f6feb", accentText: "#79c0ff" },
    colors: {
      background: "#0d1117",
      foreground: "#c9d1d9",
      cursor: "#79c0ff",
      cursorAccent: "#0d1117",
      selectionBackground: "rgba(56, 139, 253, 0.3)",
      ...palette([
        "#1f2630", "#ff7b72", "#7ee787", "#e3b341", "#79c0ff", "#d2a8ff", "#56d4dd", "#b1bac4",
        "#6e7681", "#ffa198", "#a6f0ad", "#f2cc60", "#a5d6ff", "#e2c5ff", "#8be4ea", "#f0f6fc",
      ]),
    },
  },
  {
    id: "nord",
    name: "Nord",
    kind: "dark",
    ui: { bg: "#2e3440", subtle: "#323946", sunken: "#3b4252", hover: "#3b4252", active: "#434c5e", border: "#3b4252", borderStrong: "#4c566a", text: "#eceff4", muted: "#a7b1c2", faint: "#7b88a1", accent: "#5e81ac", accentText: "#88c0d0" },
    colors: {
      background: "#2e3440",
      foreground: "#d8dee9",
      cursor: "#d8dee9",
      cursorAccent: "#2e3440",
      selectionBackground: "rgba(136, 192, 208, 0.25)",
      ...palette([
        "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
        "#616e88", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
      ]),
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    kind: "dark",
    ui: { bg: "#282a36", subtle: "#2c2e3b", sunken: "#21222c", hover: "#343746", active: "#44475a", border: "#343746", borderStrong: "#44475a", text: "#f8f8f2", muted: "#b4b7c8", faint: "#7a86b6", accent: "#8c62d9", accentText: "#bd93f9" },
    colors: {
      background: "#282a36",
      foreground: "#f8f8f2",
      cursor: "#f8f8f2",
      cursorAccent: "#282a36",
      selectionBackground: "rgba(98, 114, 164, 0.45)",
      ...palette([
        "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
        "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
      ]),
    },
  },
  {
    // Catppuccin's Frappé flavour. Its mauve is too light for the white text
    // of a button, so buttons take Latte's deeper mauve, as in Mocha.
    id: "catppuccin-frappe",
    name: "Catppuccin Frappé",
    kind: "dark",
    ui: { bg: "#303446", subtle: "#292c3c", sunken: "#232634", hover: "#414559", active: "#51576d", border: "#414559", borderStrong: "#51576d", text: "#c6d0f5", muted: "#a5adce", faint: "#838ba7", accent: "#8839ef", accentText: "#ca9ee6" },
    colors: {
      background: "#303446",
      foreground: "#c6d0f5",
      cursor: "#f2d5cf",
      cursorAccent: "#303446",
      selectionBackground: "rgba(148, 156, 187, 0.3)",
      ...palette([
        "#51576d", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be", "#b5bfe2",
        "#626880", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be", "#a5adce",
      ]),
    },
  },
  {
    // Catppuccin's Macchiato flavour. Its mauve is too light for the white
    // text of a button, so buttons take Latte's deeper mauve, as in Mocha.
    id: "catppuccin-macchiato",
    name: "Catppuccin Macchiato",
    kind: "dark",
    ui: { bg: "#24273a", subtle: "#1e2030", sunken: "#181926", hover: "#363a4f", active: "#494d64", border: "#363a4f", borderStrong: "#494d64", text: "#cad3f5", muted: "#a5adcb", faint: "#8087a2", accent: "#8839ef", accentText: "#c6a0f6" },
    colors: {
      background: "#24273a",
      foreground: "#cad3f5",
      cursor: "#f4dbd6",
      cursorAccent: "#24273a",
      selectionBackground: "rgba(147, 154, 183, 0.3)",
      ...palette([
        "#494d64", "#ed8796", "#a6da95", "#eed49f", "#8aadf4", "#f5bde6", "#8bd5ca", "#b8c0e0",
        "#5b6078", "#ed8796", "#a6da95", "#eed49f", "#8aadf4", "#f5bde6", "#8bd5ca", "#a5adcb",
      ]),
    },
  },
  {
    // Catppuccin's Mocha flavour. Its mauve is too light for the white text
    // of a button, so buttons take Latte's deeper mauve; text keeps Mocha's.
    id: "catppuccin-mocha",
    name: "Catppuccin Mocha",
    kind: "dark",
    ui: { bg: "#1e1e2e", subtle: "#181825", sunken: "#11111b", hover: "#313244", active: "#45475a", border: "#313244", borderStrong: "#45475a", text: "#cdd6f4", muted: "#a6adc8", faint: "#7f849c", accent: "#8839ef", accentText: "#cba6f7" },
    colors: {
      background: "#1e1e2e",
      foreground: "#cdd6f4",
      cursor: "#f5e0dc",
      cursorAccent: "#1e1e2e",
      selectionBackground: "rgba(147, 153, 178, 0.3)",
      ...palette([
        "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
        "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
      ]),
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    kind: "dark",
    ui: { bg: "#002b36", subtle: "#03303c", sunken: "#073642", hover: "#0a3c4a", active: "#0f4654", border: "#0f4654", borderStrong: "#1c5664", text: "#eee8d5", muted: "#93a1a1", faint: "#657b83", accent: "#268bd2", accentText: "#6cb6e8" },
    colors: {
      background: "#002b36",
      foreground: "#93a1a1",
      cursor: "#93a1a1",
      cursorAccent: "#002b36",
      selectionBackground: "rgba(88, 110, 117, 0.45)",
      ...palette([
        "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
        "#586e75", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
      ]),
    },
  },
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    kind: "dark",
    ui: { bg: "#1a1b26", subtle: "#16161e", sunken: "#13131a", hover: "#222333", active: "#292e42", border: "#292e42", borderStrong: "#3b4261", text: "#c0caf5", muted: "#a9b1d6", faint: "#737aa2", accent: "#3d59a1", accentText: "#7aa2f7" },
    colors: {
      background: "#1a1b26",
      foreground: "#c0caf5",
      cursor: "#c0caf5",
      cursorAccent: "#1a1b26",
      selectionBackground: "rgba(122, 162, 247, 0.25)",
      ...palette([
        "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
        "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
      ]),
    },
  },
  {
    id: "one-dark",
    name: "One Dark",
    kind: "dark",
    ui: { bg: "#282c34", subtle: "#21252b", sunken: "#1e2227", hover: "#2c313a", active: "#3e4451", border: "#3a3f4b", borderStrong: "#4b5263", text: "#d7dae0", muted: "#abb2bf", faint: "#7f848e", accent: "#4d78cc", accentText: "#61afef" },
    colors: {
      background: "#282c34",
      foreground: "#abb2bf",
      cursor: "#528bff",
      cursorAccent: "#282c34",
      selectionBackground: "rgba(97, 175, 239, 0.22)",
      ...palette([
        "#3f4451", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#d7dae0",
        "#5c6370", "#e06c75", "#98c379", "#d19a66", "#61afef", "#c678dd", "#56b6c2", "#ffffff",
      ]),
    },
  },
  {
    id: "gruvbox-dark",
    name: "Gruvbox Dark",
    kind: "dark",
    ui: { bg: "#282828", subtle: "#1d2021", sunken: "#32302f", hover: "#32302f", active: "#3c3836", border: "#3c3836", borderStrong: "#504945", text: "#ebdbb2", muted: "#bdae93", faint: "#928374", accent: "#d65d0e", accentText: "#fe8019" },
    colors: {
      background: "#282828",
      foreground: "#ebdbb2",
      cursor: "#ebdbb2",
      cursorAccent: "#282828",
      selectionBackground: "rgba(146, 131, 116, 0.35)",
      ...palette([
        "#3c3836", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
        "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
      ]),
    },
  },
  {
    id: "rose-pine",
    name: "Rosé Pine",
    kind: "dark",
    ui: { bg: "#191724", subtle: "#1f1d2e", sunken: "#16141f", hover: "#26233a", active: "#403d52", border: "#26233a", borderStrong: "#403d52", text: "#e0def4", muted: "#908caa", faint: "#6e6a86", accent: "#31748f", accentText: "#c4a7e7" },
    colors: {
      background: "#191724",
      foreground: "#e0def4",
      cursor: "#e0def4",
      cursorAccent: "#191724",
      selectionBackground: "rgba(110, 106, 134, 0.35)",
      ...palette([
        "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
        "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
      ]),
    },
  },
  {
    id: "monokai",
    name: "Monokai",
    kind: "dark",
    ui: { bg: "#272822", subtle: "#1e1f1c", sunken: "#2d2e27", hover: "#3e3d32", active: "#49483e", border: "#3e3d32", borderStrong: "#49483e", text: "#f8f8f2", muted: "#cfcfc2", faint: "#75715e", accent: "#c2185b", accentText: "#ff6f9a" },
    colors: {
      background: "#272822",
      foreground: "#f8f8f2",
      cursor: "#f8f8f0",
      cursorAccent: "#272822",
      selectionBackground: "rgba(117, 113, 94, 0.4)",
      ...palette([
        "#272822", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f8f8f2",
        "#75715e", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4", "#f9f8f5",
      ]),
    },
  },
];

export const DEFAULT_THEME = { light: "convoy-light", dark: "convoy-dark" };

const KEY = { light: "convoy.terminal.light", dark: "convoy.terminal.dark" };

/// The themes offered for a light or a dark window.
export const themesOf = (kind) => TERMINAL_THEMES.filter((theme) => theme.kind === kind);

/// The theme `id` names for `kind`, or that kind's default for an id it does
/// not know (a removed theme, or one of the other kind).
export function themeFor(kind, id) {
  return themesOf(kind).find((theme) => theme.id === id) ?? TERMINAL_THEMES.find((theme) => theme.id === DEFAULT_THEME[kind]);
}

/// The chosen theme's id for a light or a dark window.
export function chosenTheme(kind) {
  try {
    return themeFor(kind, localStorage.getItem(KEY[kind])).id;
  } catch {
    return DEFAULT_THEME[kind];
  }
}

/// Remembers the theme for `kind`; the default is not stored.
export function chooseTheme(kind, id) {
  const theme = themeFor(kind, id);
  try {
    if (theme.id === DEFAULT_THEME[kind]) localStorage.removeItem(KEY[kind]);
    else localStorage.setItem(KEY[kind], theme.id);
  } catch {
    /* not remembered */
  }
  return theme.id;
}

/// Whether the window is drawn dark now: its own setting, else the desktop's.
function windowIsDark() {
  const set = document.documentElement.getAttribute("data-theme");
  if (set === "dark" || set === "light") return set === "dark";
  return matchMedia("(prefers-color-scheme: dark)").matches;
}

/// The theme the terminals use now.
export function activeTheme() {
  const kind = windowIsDark() ? "dark" : "light";
  return themeFor(kind, chosenTheme(kind));
}

/// Every window colour a theme's `ui` sets, worked out from its few choices:
/// the accent's hover, pressed and soft shades are mixed from the accent.
export function uiVariables(theme) {
  const ui = theme.ui;
  if (!ui) return {};
  const dark = theme.kind === "dark";
  const toward = dark ? "white" : "black";
  return {
    "--bg": ui.bg,
    "--bg-subtle": ui.subtle,
    "--bg-sunken": ui.sunken,
    "--bg-hover": ui.hover,
    "--bg-active": ui.active,
    "--border": ui.border,
    "--border-strong": ui.borderStrong,
    "--text": ui.text,
    "--text-muted": ui.muted,
    "--text-faint": ui.faint,
    "--accent": ui.accent,
    "--accent-hover": `color-mix(in srgb, ${ui.accent} 88%, ${toward})`,
    "--accent-active": `color-mix(in srgb, ${ui.accent} 78%, ${toward})`,
    "--accent-soft": `color-mix(in srgb, ${ui.accent} ${dark ? 18 : 10}%, ${ui.bg})`,
    "--accent-soft-hover": `color-mix(in srgb, ${ui.accent} ${dark ? 24 : 15}%, ${ui.bg})`,
    "--accent-text": ui.accentText,
    "--accent-ring": `color-mix(in srgb, ${ui.accent} 35%, transparent)`,
  };
}

/// Every property `applyThemeColors` may set, so a change of theme clears
/// what the last one set and tokens.css shows through again.
const SET = [...Object.keys(uiVariables(TERMINAL_THEMES.find((theme) => theme.ui))), "--term-bg", "--term-fg"];

/// Colours the window with the active theme and paints the space around each
/// terminal in its background. Returns the terminal's colours.
export function applyThemeColors() {
  const theme = activeTheme();
  const root = document.documentElement.style;
  for (const name of SET) root.removeProperty(name);
  const set = { ...uiVariables(theme), "--term-bg": theme.colors.background, "--term-fg": theme.colors.foreground };
  for (const [name, value] of Object.entries(set)) root.setProperty(name, value);
  return theme.colors;
}
