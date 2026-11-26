// The font of terminals and code.
//
// Four popular coding fonts ship with the app (src/styles/fonts.css), so they
// work on any machine; the system's own monospace fonts and any font the user
// installed can be named too. The choice is per machine, like the theme.

/// What every choice falls back to, glyph by glyph, as tokens.css does.
export const MONO_FALLBACK = 'ui-monospace, "SF Mono", Menlo, Consolas, monospace';

/// `family`: the CSS family name; none for the system's default.
export const MONO_FONTS = [
  { id: "system", name: "System", family: null },
  { id: "jetbrains-mono", name: "JetBrains Mono", family: "JetBrains Mono" },
  { id: "fira-code", name: "Fira Code", family: "Fira Code" },
  { id: "cascadia-code", name: "Cascadia Code", family: "Cascadia Code" },
  { id: "ibm-plex-mono", name: "IBM Plex Mono", family: "IBM Plex Mono" },
  { id: "menlo", name: "Menlo", family: "Menlo" },
  { id: "custom", name: "Other installed font", family: null },
];

const KEY = "convoy.font.mono";
const CUSTOM_KEY = "convoy.font.monoCustom";

const known = (id) => MONO_FONTS.some((font) => font.id === id);

const read = (key) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};

const write = (key, value) => {
  try {
    if (value) localStorage.setItem(key, value);
    else localStorage.removeItem(key);
  } catch {
    /* not remembered */
  }
};

/// The chosen font's id, `system` for none or one no longer offered.
export const chosenMonoFont = () => (known(read(KEY)) ? read(KEY) : "system");

/// The family typed for "Other installed font".
export const customMonoFont = () => read(CUSTOM_KEY) ?? "";

export function chooseMonoFont(id) {
  write(KEY, known(id) && id !== "system" ? id : null);
}

export function setCustomMonoFont(name) {
  write(CUSTOM_KEY, String(name ?? "").replace(/["\\;{}]/g, "").trim());
}

/// The CSS stack for a choice: its family first, the fallbacks after. A
/// custom choice with no name is the system's default.
export function monoStack(id, custom = "") {
  const font = MONO_FONTS.find((each) => each.id === id);
  const family = id === "custom" ? custom.replace(/["\\;{}]/g, "").trim() : font?.family;
  return family ? `"${family}", ${MONO_FALLBACK}` : MONO_FALLBACK;
}

/// Sets `--font-mono` for the chosen font and waits until it can be drawn,
/// so a terminal measures the right glyph width when it is told after.
export async function applyMonoFont() {
  const stack = monoStack(chosenMonoFont(), customMonoFont());
  const root = document.documentElement.style;
  if (chosenMonoFont() === "system") root.removeProperty("--font-mono");
  else root.setProperty("--font-mono", stack);
  try {
    await Promise.all([document.fonts.load(`13px ${stack}`), document.fonts.load(`bold 13px ${stack}`)]);
  } catch {
    /* drawn with the fallback */
  }
  return stack;
}
