import { useCallback, useEffect, useRef, useState } from "react";

import { SERVICE_WORKER_URL } from "./notifications";

export type PwaInstallState =
  | "checking"
  | "available"
  | "installing"
  | "submitted"
  | "installed"
  | "manual"
  | "unavailable";

interface BeforeInstallPromptEvent extends Event {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

interface NavigatorWithWebInstall extends Navigator {
  standalone?: boolean;
  install?: () => Promise<void>;
}

export function installFallbackState(
  userAgent: string,
  standalone: boolean,
  supportsWebInstall = false,
): PwaInstallState {
  if (standalone) {
    return "installed";
  }
  if (supportsWebInstall) {
    return "available";
  }
  return /iPad|iPhone|iPod/i.test(userAgent) ? "manual" : "unavailable";
}

type InstallSnapshot = {
  state: PwaInstallState;
  prompt: BeforeInstallPromptEvent | undefined;
};

let monitorInitialized = false;
let currentSnapshot: InstallSnapshot = {
  state: "checking",
  prompt: undefined,
};
const listeners = new Set<(snap: InstallSnapshot) => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener(currentSnapshot);
  }
}

/**
 * Attach app-wide `beforeinstallprompt` / `appinstalled` / serviceWorker
 * listeners at boot so opening Settings later never misses an early prompt event.
 */
export function initPwaInstallMonitor(): void {
  if (monitorInitialized || typeof window === "undefined") return;
  monitorInitialized = true;

  const nav = navigator as NavigatorWithWebInstall;
  const standalone =
    window.matchMedia("(display-mode: standalone)").matches ||
    nav.standalone === true;
  const isIpadDesktopMode =
    /Macintosh/i.test(nav.userAgent) && nav.maxTouchPoints > 1;
  const supportsWebInstall = typeof nav.install === "function";

  currentSnapshot = {
    state: installFallbackState(
      isIpadDesktopMode ? "iPad" : nav.userAgent,
      standalone,
      supportsWebInstall,
    ),
    prompt: undefined,
  };

  window.addEventListener("beforeinstallprompt", (rawEvent: Event) => {
    const event = rawEvent as BeforeInstallPromptEvent;
    event.preventDefault();
    currentSnapshot = {
      state: supportsWebInstall ? currentSnapshot.state : "available",
      prompt: event,
    };
    notifyListeners();
  });

  window.addEventListener("appinstalled", () => {
    currentSnapshot = { state: "installed", prompt: undefined };
    notifyListeners();
  });

  // Registration does not request notification permission and sw.js has no
  // fetch handler or offline cache. It only establishes the PWA/push worker.
  if (window.isSecureContext && "serviceWorker" in navigator) {
    void navigator.serviceWorker
      .register(SERVICE_WORKER_URL)
      .catch(() => undefined);
  }
}

export function usePwaInstall(): {
  state: PwaInstallState;
  install: () => Promise<void>;
} {
  const promptRef = useRef<BeforeInstallPromptEvent | undefined>(undefined);
  const [state, setState] = useState<PwaInstallState>(() => {
    initPwaInstallMonitor();
    promptRef.current = currentSnapshot.prompt;
    return currentSnapshot.state;
  });

  useEffect(() => {
    initPwaInstallMonitor();
    promptRef.current = currentSnapshot.prompt;
    setState(currentSnapshot.state);
    const listener = (snap: InstallSnapshot) => {
      promptRef.current = snap.prompt;
      setState(snap.state);
    };
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);

  const install = useCallback(async () => {
    const nav = navigator as NavigatorWithWebInstall;
    if (typeof nav.install === "function") {
      setState("installing");
      try {
        // Call directly from user click activation per Web Install API spec.
        await nav.install();
        currentSnapshot = { ...currentSnapshot, state: "installed" };
        notifyListeners();
      } catch (error) {
        if ((error as { name?: string })?.name === "AbortError") {
          currentSnapshot = { ...currentSnapshot, state: "available" };
          notifyListeners();
          return;
        }
        currentSnapshot = { ...currentSnapshot, state: "unavailable" };
        notifyListeners();
      }
      return;
    }

    const prompt = promptRef.current ?? currentSnapshot.prompt;
    if (prompt === undefined) {
      return;
    }
    // Single-use event: clear ref before awaiting user choice.
    promptRef.current = undefined;
    currentSnapshot = { state: "installing", prompt: undefined };
    notifyListeners();
    try {
      await prompt.prompt();
      const choice = await prompt.userChoice;
      currentSnapshot = {
        ...currentSnapshot,
        state:
          choice.outcome === "accepted"
            ? "submitted"
            : currentSnapshot.prompt !== undefined
              ? "available"
              : "unavailable",
      };
      notifyListeners();
    } catch (error) {
      currentSnapshot = {
        ...currentSnapshot,
        state:
          currentSnapshot.prompt !== undefined ? "available" : "unavailable",
      };
      notifyListeners();
      throw error;
    }
  }, []);

  return { state, install };
}
