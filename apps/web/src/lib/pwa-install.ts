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

export function installFallbackState(
  userAgent: string,
  standalone: boolean,
): PwaInstallState {
  if (standalone) {
    return "installed";
  }
  return /iPad|iPhone|iPod/i.test(userAgent) ? "manual" : "unavailable";
}

export function usePwaInstall(): {
  state: PwaInstallState;
  install: () => Promise<void>;
} {
  const promptRef = useRef<BeforeInstallPromptEvent | undefined>(undefined);
  const [state, setState] = useState<PwaInstallState>("checking");

  useEffect(() => {
    const navigatorWithStandalone = navigator as Navigator & {
      standalone?: boolean;
    };
    const standalone =
      window.matchMedia("(display-mode: standalone)").matches ||
      navigatorWithStandalone.standalone === true;
    setState(installFallbackState(navigator.userAgent, standalone));

    const onBeforeInstall = (rawEvent: Event) => {
      const event = rawEvent as BeforeInstallPromptEvent;
      event.preventDefault();
      promptRef.current = event;
      setState("available");
    };
    const onInstalled = () => {
      promptRef.current = undefined;
      setState("installed");
    };
    window.addEventListener("beforeinstallprompt", onBeforeInstall);
    window.addEventListener("appinstalled", onInstalled);
    // Registration does not request notification permission and sw.js has no
    // fetch handler or offline cache. It only establishes the PWA/push worker.
    if (window.isSecureContext && "serviceWorker" in navigator) {
      void navigator.serviceWorker
        .register(SERVICE_WORKER_URL)
        .catch(() => undefined);
    }
    return () => {
      window.removeEventListener("beforeinstallprompt", onBeforeInstall);
      window.removeEventListener("appinstalled", onInstalled);
    };
  }, []);

  const install = useCallback(async () => {
    const prompt = promptRef.current;
    if (prompt === undefined) {
      return;
    }
    setState("installing");
    await prompt.prompt();
    const choice = await prompt.userChoice;
    promptRef.current = undefined;
    setState(choice.outcome === "accepted" ? "submitted" : "unavailable");
  }, []);

  return { state, install };
}
