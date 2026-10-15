import ru from "../../src/locales/ru.js";

/// The English keys among `keys` that have no Russian.
export const untranslated = (keys) => [...new Set(keys)].filter((key) => !(key in ru.strings));
