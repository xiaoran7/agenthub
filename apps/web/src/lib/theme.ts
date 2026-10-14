export type ThemeMode = "system" | "light" | "dark";

export const THEME_STORAGE_KEY = "agenthub_theme";
export const THEME_CHANGE_EVENT = "agenthub-theme-change";

export function getStoredTheme(): ThemeMode {
  if (typeof window === "undefined") return "system";
  try {
    const raw = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (raw === "light" || raw === "dark" || raw === "system") {
      return raw;
    }
  } catch {
    // ignore localStorage errors in privacy mode
  }
  return "system";
}

export function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "light" || mode === "dark") {
    return mode;
  }
  if (typeof window !== "undefined" && window.matchMedia) {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  return "dark";
}

export function applyTheme(mode: ThemeMode): "light" | "dark" {
  const resolved = resolveTheme(mode);
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", resolved);
    document.documentElement.style.colorScheme = resolved;
  }
  return resolved;
}

export function setTheme(mode: ThemeMode): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, mode);
  } catch {
    // ignore
  }
  applyTheme(mode);
  window.dispatchEvent(
    new CustomEvent(THEME_CHANGE_EVENT, { detail: { mode } }),
  );
}

export function initThemeListener(
  onChange?: (mode: ThemeMode) => void,
): () => void {
  if (typeof window === "undefined" || !window.matchMedia) {
    return () => {};
  }

  const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

  const handleMediaChange = () => {
    const current = getStoredTheme();
    if (current === "system") {
      applyTheme("system");
      onChange?.("system");
    }
  };

  const handleCustomEvent = (e: Event) => {
    const detail = (e as CustomEvent).detail;
    if (detail && detail.mode) {
      onChange?.(detail.mode);
    }
  };

  mediaQuery.addEventListener("change", handleMediaChange);
  window.addEventListener(THEME_CHANGE_EVENT, handleCustomEvent);

  // Apply on mount
  applyTheme(getStoredTheme());

  return () => {
    mediaQuery.removeEventListener("change", mediaChangeHandler);
    window.removeEventListener(THEME_CHANGE_EVENT, handleCustomEvent);
  };
}

const mediaChangeHandler = () => {
  if (getStoredTheme() === "system") {
    applyTheme("system");
  }
};
