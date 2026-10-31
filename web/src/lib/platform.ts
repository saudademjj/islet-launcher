export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);

/** Modifier shown in shortcut hints. */
export const MOD_KEY = isMac ? "⌘" : "Ctrl+";
