// The "?" beside a term: its line from the glossary, in a tip that lives
// outside the rendered page, so a re-render or a scrolling box never cuts
// it off. Shown on hover and on focus; a click on the "?" does nothing else.

export function installHints() {
  const box = document.createElement("div");
  box.className = "hint-tip";
  box.setAttribute("role", "tooltip");
  box.hidden = true;
  document.body.append(box);
  let current = null;

  function show(node) {
    current = node;
    box.textContent = node.dataset.hint;
    box.hidden = false;
    const rect = node.getBoundingClientRect();
    const tip = box.getBoundingClientRect();
    const left = Math.max(8, Math.min(window.innerWidth - tip.width - 8, rect.left + rect.width / 2 - tip.width / 2));
    let top = rect.bottom + 6;
    if (top + tip.height > window.innerHeight - 8) top = rect.top - tip.height - 6;
    box.style.left = `${left}px`;
    box.style.top = `${top}px`;
  }

  function hide() {
    current = null;
    box.hidden = true;
  }

  const hintOf = (event) => event.target.closest?.("[data-hint]");
  document.addEventListener("pointerover", (event) => {
    const node = hintOf(event);
    if (node && node !== current) show(node);
    else if (!node && current) hide();
  });
  document.addEventListener("focusin", (event) => {
    const node = hintOf(event);
    if (node) show(node);
  });
  document.addEventListener("focusout", (event) => hintOf(event) && hide());
  document.addEventListener("pointerleave", hide);
  // A "?" inside a label or a button must not toggle what it explains.
  document.addEventListener("click", (event) => {
    if (!hintOf(event)) return;
    event.preventDefault();
    event.stopPropagation();
  }, true);
  // A redraw replaces the "?" a tip was shown for; the tip goes with it.
  const afterRender = () => current && !current.isConnected && hide();
  return { hide, afterRender };
}
