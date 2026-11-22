import { useCallback, useEffect, useRef, useState } from "react";

const MOBILE_MEDIA_QUERY = "(max-width: 700px)";
const EDGE_SWIPE_ZONE_PX = 28;
const SWIPE_COMMIT_PX = 56;

interface MobileTouchState {
  startX: number;
  startY: number;
  mode: "open-edge" | "close-drawer";
  locked: "horizontal" | "vertical" | null;
}

export interface MobileConsoleLayout {
  isMobile: boolean;
  mobileDrawerOpen: boolean;
  drawerAnimated: boolean;
  consoleRootRef: React.RefObject<HTMLDivElement | null>;
  mobileMenuBtnRef: React.RefObject<HTMLButtonElement | null>;
  openMobileDrawer: () => void;
  closeMobileDrawer: (restoreFocus?: boolean) => void;
  setMobileDrawerOpen: React.Dispatch<React.SetStateAction<boolean>>;
  onSidebarTouchStart: (event: React.TouchEvent<HTMLElement>) => void;
  onSidebarTouchMove: (event: React.TouchEvent<HTMLElement>) => void;
  onSidebarTouchEnd: (event: React.TouchEvent<HTMLElement>) => void;
  onMainTouchStart: (event: React.TouchEvent<HTMLElement>) => void;
  onMainTouchMove: (event: React.TouchEvent<HTMLElement>) => void;
  onMainTouchEnd: (event: React.TouchEvent<HTMLElement>) => void;
}

export function useMobileConsoleLayout(): MobileConsoleLayout {
  const [isMobile, setIsMobile] = useState(
    () =>
      typeof window !== "undefined" &&
      (typeof window.matchMedia === "function"
        ? window.matchMedia(MOBILE_MEDIA_QUERY).matches
        : window.innerWidth <= 700),
  );
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
  const [drawerAnimated, setDrawerAnimated] = useState(false);
  const consoleRootRef = useRef<HTMLDivElement>(null);
  const mobileMenuBtnRef = useRef<HTMLButtonElement>(null);
  const touchRef = useRef<MobileTouchState | null>(null);

  const openMobileDrawer = useCallback(() => {
    setDrawerAnimated(true);
    setMobileDrawerOpen(true);
  }, []);

  const closeMobileDrawer = useCallback((restoreFocus = false) => {
    setMobileDrawerOpen(false);
    if (restoreFocus) {
      requestAnimationFrame(() => {
        mobileMenuBtnRef.current?.focus({ preventScroll: true });
      });
    }
  }, []);

  useEffect(() => {
    const mql =
      typeof window.matchMedia === "function"
        ? window.matchMedia(MOBILE_MEDIA_QUERY)
        : null;
    const syncViewport = (mobile: boolean) => {
      setIsMobile(mobile);
      if (!mobile) {
        setMobileDrawerOpen(false);
        setDrawerAnimated(false);
      }
    };
    if (!mql) {
      const onResize = () => syncViewport(window.innerWidth <= 700);
      onResize();
      window.addEventListener("resize", onResize);
      return () => window.removeEventListener("resize", onResize);
    }
    syncViewport(mql.matches);
    const onChange = (event: MediaQueryListEvent) =>
      syncViewport(event.matches);
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, []);

  // Sync visualViewport height (`--vvh`) on mobile so iOS Safari / Android
  // virtual keyboards shrink the flex column instead of covering the composer
  // or pushing the sticky header above the screen.
  useEffect(() => {
    const root = consoleRootRef.current;
    if (!isMobile || !root) {
      root?.style.removeProperty("--vvh");
      return;
    }
    const vv = window.visualViewport;
    if (!vv) return;

    const syncVisualHeight = () => {
      const h = Math.round(vv.height);
      if (h > 120) {
        root.style.setProperty("--vvh", `${h}px`);
      }
    };
    syncVisualHeight();
    vv.addEventListener("resize", syncVisualHeight);
    vv.addEventListener("scroll", syncVisualHeight);
    return () => {
      vv.removeEventListener("resize", syncVisualHeight);
      vv.removeEventListener("scroll", syncVisualHeight);
      root.style.removeProperty("--vvh");
    };
  }, [isMobile]);

  const onMainTouchStart = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      if (!isMobile || mobileDrawerOpen || event.touches.length !== 1) {
        touchRef.current = null;
        return;
      }
      const touch = event.touches[0]!;
      if (touch.clientX > EDGE_SWIPE_ZONE_PX) {
        touchRef.current = null;
        return;
      }
      touchRef.current = {
        startX: touch.clientX,
        startY: touch.clientY,
        mode: "open-edge",
        locked: null,
      };
    },
    [isMobile, mobileDrawerOpen],
  );

  const onMainTouchMove = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      const state = touchRef.current;
      if (!state || state.mode !== "open-edge" || event.touches.length !== 1) {
        return;
      }
      const touch = event.touches[0]!;
      const dx = touch.clientX - state.startX;
      const dy = touch.clientY - state.startY;
      if (state.locked === null && (Math.abs(dx) > 8 || Math.abs(dy) > 8)) {
        state.locked =
          Math.abs(dx) > Math.abs(dy) * 1.35 ? "horizontal" : "vertical";
      }
    },
    [],
  );

  const onMainTouchEnd = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      const state = touchRef.current;
      touchRef.current = null;
      if (
        !state ||
        state.mode !== "open-edge" ||
        state.locked !== "horizontal" ||
        event.changedTouches.length === 0
      ) {
        return;
      }
      const dx = event.changedTouches[0]!.clientX - state.startX;
      if (dx >= SWIPE_COMMIT_PX) {
        openMobileDrawer();
      }
    },
    [openMobileDrawer],
  );

  const onSidebarTouchStart = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      if (!isMobile || !mobileDrawerOpen || event.touches.length !== 1) {
        touchRef.current = null;
        return;
      }
      const touch = event.touches[0]!;
      touchRef.current = {
        startX: touch.clientX,
        startY: touch.clientY,
        mode: "close-drawer",
        locked: null,
      };
    },
    [isMobile, mobileDrawerOpen],
  );

  const onSidebarTouchMove = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      const state = touchRef.current;
      if (
        !state ||
        state.mode !== "close-drawer" ||
        event.touches.length !== 1
      ) {
        return;
      }
      const touch = event.touches[0]!;
      const dx = touch.clientX - state.startX;
      const dy = touch.clientY - state.startY;
      if (state.locked === null && (Math.abs(dx) > 8 || Math.abs(dy) > 8)) {
        state.locked =
          Math.abs(dx) > Math.abs(dy) * 1.35 ? "horizontal" : "vertical";
      }
    },
    [],
  );

  const onSidebarTouchEnd = useCallback(
    (event: React.TouchEvent<HTMLElement>) => {
      const state = touchRef.current;
      touchRef.current = null;
      if (
        !state ||
        state.mode !== "close-drawer" ||
        state.locked !== "horizontal" ||
        event.changedTouches.length === 0
      ) {
        return;
      }
      const dx = event.changedTouches[0]!.clientX - state.startX;
      if (dx <= -SWIPE_COMMIT_PX) {
        closeMobileDrawer(true);
      }
    },
    [closeMobileDrawer],
  );

  return {
    isMobile,
    mobileDrawerOpen,
    drawerAnimated,
    consoleRootRef,
    mobileMenuBtnRef,
    openMobileDrawer,
    closeMobileDrawer,
    setMobileDrawerOpen,
    onSidebarTouchStart,
    onSidebarTouchMove,
    onSidebarTouchEnd,
    onMainTouchStart,
    onMainTouchMove,
    onMainTouchEnd,
  };
}
