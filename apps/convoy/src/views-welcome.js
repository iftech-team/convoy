// The window before any project: what Convoy is, how a task moves through
// it, and what is still to set up. A first run starts from the idea, not
// from an empty list.

import { icons } from "./icons.js";
import { button, escape } from "./ui.js";
import { state } from "./state.js";
import { t } from "./i18n.js";
import { checklistCard } from "./views-home.js";
import { FLOW, PITCH } from "./onboarding.js";

/// The three steps as a row: the same picture Home shows behind its "?".
export const flow = () => `
  <ol class="flow" aria-label="${escape(t("How it works"))}">
    ${FLOW.map(
      ([mark, title, text], index) => `
      <li class="flow__step">
        <span class="flow__mark">${icons[mark] ?? ""}<span class="flow__number">${index + 1}</span></span>
        <span class="flow__title">${escape(t(title))}</span>
        <span class="flow__text">${escape(t(text))}</span>
      </li>`,
    ).join("")}
  </ol>`;

export function welcomeView() {
  return `
    <div class="welcome" data-scroll="welcome">
      <header class="welcome__intro">
        <h1 class="welcome__title">${escape(t("Welcome to Convoy"))}</h1>
        <p class="welcome__lead">${escape(t(PITCH))}</p>
      </header>
      ${flow()}
      <div class="welcome__start">
        ${button({ label: t("Open a project…"), icon: "folder", kind: "primary", action: "open-folder" })}
        <p class="welcome__note">${escape(t("A repository, or a folder of related ones. Nothing in it changes until you run a task."))}</p>
      </div>
      <div class="welcome__checklist">${checklistCard({ dismissible: false })}</div>
      ${
        state.macImport?.projects
          ? `<button type="button" class="link-button welcome__import" data-action="mac-import-offer">${escape(t("Used Convoy for macOS? Import its workspace…"))}</button>`
          : ""
      }
    </div>`;
}
