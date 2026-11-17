// The window's zoom: the whole interface drawn larger or smaller, as a
// browser's page zoom does, for a screen where the default size reads small.
//
// The choice is per machine, like the window's language, and kept out of the
// workspace file: it belongs to the screen, and the macOS app writes that file
// too and has no such field. A terminal's own zoom stays an offset from its
// font size and multiplies with this one.

const KEY = "convoy.zoom.ui";

/// The steps ⌘+ and ⌘− move through, and the choices Settings offers.
export const ZOOMS = [0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

const known = (value) => ZOOMS.includes(value);

const stored = () => {
  try {
    const value = Number(localStorage.getItem(KEY));
    return known(value) ? value : 1;
  } catch {
    return 1;
  }
};

let factor = stored();

/// The zoom the window is drawn at, 1 for the default size.
export const uiZoom = () => factor;

/// Draws the window at the stored zoom. Without a native window, as under the
/// UI tests, there is nothing to zoom and nothing to report.
export async function applyZoom() {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().setZoom(factor);
  } catch {
    /* no native window */
  }
}

/// Remembers `next` and draws the window at it. A value that is not one of
/// the steps is the default size.
export function setZoom(next) {
  factor = known(Number(next)) ? Number(next) : 1;
  try {
    if (factor === 1) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, String(factor));
  } catch {
    /* not remembered */
  }
  return applyZoom().then(() => factor);
}

/// The step after `current` in the direction of `step` (+1 larger, −1
/// smaller), or the default size for 0. Stops at either end.
export function stepFrom(current, step) {
  if (step === 0) return 1;
  const index = ZOOMS.indexOf(current);
  const from = index === -1 ? ZOOMS.indexOf(1) : index;
  return ZOOMS[Math.max(0, Math.min(ZOOMS.length - 1, from + Math.sign(step)))];
}

/// One step larger, smaller, or back to the default size.
export const stepZoom = (step) => setZoom(stepFrom(factor, step));

/// A zoom as the percentage people read it in.
export const percent = (value) => `${Math.round(value * 100)}%`;
