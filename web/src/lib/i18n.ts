// The interface language. Strings are written as pairs, t("中文", "English"),
// so each stays next to its translation. The language is fixed for the
// page's lifetime: switching saves the choice and reloads, so even strings
// built when a module loads come out in the right language.

export type Language = "system" | "zh" | "en";

const KEY = "mc-launcher.language";

function stored(): Language {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "zh" || value === "en" || value === "system") return value;
  } catch {
    // Storage can be unavailable; follow the system then.
  }
  return "system";
}

/** "system" follows the WebView's languages: Chinese if any is Chinese. */
export function resolve(pref: Language): "zh" | "en" {
  if (pref !== "system") return pref;
  const languages = typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language];
  return languages.some((l) => l.toLowerCase().startsWith("zh")) ? "zh" : "en";
}

export const preference: Language = stored();
export const lang: "zh" | "en" = resolve(preference);
export const isEnglish = lang === "en";

if (typeof document !== "undefined") {
  document.documentElement.lang = isEnglish ? "en" : "zh-CN";
  if (isEnglish) document.title = "Islet";
}

export function t(zh: string, en: string): string {
  return isEnglish ? en : zh;
}

/** Saves the choice for the next load. */
export function rememberLanguage(pref: Language) {
  try {
    localStorage.setItem(KEY, pref);
  } catch {
    // Without storage the saved setting still applies after a restart.
  }
}
