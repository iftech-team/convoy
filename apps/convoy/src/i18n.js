// The window's language. English is written in the code and is its own key:
// a string with no translation shows as written, so a missed one reads in
// English rather than as a key nobody can understand.
//
// The choice is per machine, like the sidebar's folding, and kept out of the
// workspace file: the macOS app writes that file too and has no such field.

import ru from "./locales/ru.js";

const KEY = "convoy.language";
const TABLES = { ru };

/// Languages the window can be shown in, with the name each one calls itself.
export const LANGUAGES = [
  ["en", "English"],
  ["ru", "Русский"],
];

const stored = () => {
  try {
    const value = localStorage.getItem(KEY);
    return value === "en" || value in TABLES ? value : "system";
  } catch {
    return "system";
  }
};

const resolve = (choice) => {
  if (choice !== "system") return choice;
  const wanted = (navigator.languages?.length ? navigator.languages : [navigator.language]) ?? [];
  for (const tag of wanted) {
    const base = String(tag ?? "").toLowerCase().split("-")[0];
    if (base === "en") return "en";
    if (base in TABLES) return base;
  }
  return "en";
};

let choice = stored();
let current = resolve(choice);
document.documentElement.lang = current;

/// "system", or a language code someone picked.
export const languageChoice = () => choice;
/// The language the window is drawn in, for `toLocale…` and `Intl`.
export const language = () => current;

export function setLanguage(next) {
  choice = next === "en" || next in TABLES ? next : "system";
  try {
    if (choice === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, choice);
  } catch {
    /* not remembered */
  }
  current = resolve(choice);
  document.documentElement.lang = current;
}

const fill = (text, values) =>
  values ? text.replace(/\{(\w+)\}/g, (match, name) => (name in values ? String(values[name]) : match)) : text;

/// `t("Delete {name}?", { name })`. Values are put in after the lookup, so the
/// key stays the same whatever they are. Escape the result, not the values.
export function t(text, values) {
  return fill(TABLES[current]?.strings?.[text] ?? text, values);
}

/// The word that goes after a count: "task", "tasks"; in Russian "задача",
/// "задачи", "задач". Looked up by the English singular.
export function countWord(count, one, many = `${one}s`) {
  const forms = TABLES[current]?.plurals?.[one];
  if (!forms) return count === 1 ? one : many;
  const rule = new Intl.PluralRules(current).select(count);
  return forms[{ one: 0, few: 1 }[rule] ?? 2];
}
