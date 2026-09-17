export interface Appearance {
  theme: "dark" | "light" | "amber";
  app_language: "auto" | "zh-CN" | "en-US";
  default_cwd: string;
}

const STORAGE_KEY = "agenthub:extended-preferences";
type AppearanceHints = {
  theme?: string;
  app_language?: string;
  default_cwd?: string;
};

function normalizeAppearance(value: AppearanceHints): Appearance {
  return {
    theme:
      value.theme === "light" || value.theme === "amber" ? value.theme : "dark",
    app_language:
      value.app_language === "zh-CN" || value.app_language === "en-US"
        ? value.app_language
        : "auto",
    default_cwd: typeof value.default_cwd === "string" ? value.default_cwd : "",
  };
}

// Appearance is local-only: a backend dashboard snapshot must not reset it.
export function readAppearance(
  fallback: AppearanceHints = {},
  storage?: Pick<Storage, "getItem">,
): Appearance {
  try {
    const stored = (storage ?? window.localStorage).getItem(STORAGE_KEY);
    const parsed: unknown = stored ? JSON.parse(stored) : null;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return normalizeAppearance(parsed as AppearanceHints);
    }
  } catch {
    // Restricted or malformed storage still permits the default appearance.
  }
  return normalizeAppearance(fallback);
}

export function saveAppearance(
  appearance: Appearance,
  storage?: Pick<Storage, "setItem">,
): void {
  (storage ?? window.localStorage).setItem(
    STORAGE_KEY,
    JSON.stringify(normalizeAppearance(appearance)),
  );
}

export function applyAppearance(appearance: Appearance): void {
  document.documentElement.setAttribute("data-theme", appearance.theme);
}
