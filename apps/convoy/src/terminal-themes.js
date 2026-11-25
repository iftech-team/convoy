// The terminal's colour themes.
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
    id: "solarized-dark",
    name: "Solarized Dark",
    kind: "dark",
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

/// Paints the space around each terminal in its theme's background, so the
/// padding and an empty pane match the canvas.
export function applyTerminalColors() {
  const { colors } = activeTheme();
  const root = document.documentElement.style;
  root.setProperty("--term-bg", colors.background);
  root.setProperty("--term-fg", colors.foreground);
  return colors;
}
