import { useEffect, type RefObject } from "react";

export function nextDialogFocusIndex(
  currentIndex: number,
  focusableCount: number,
  reverse: boolean,
): number {
  if (focusableCount <= 0) {
    return -1;
  }
  if (currentIndex < 0) {
    return reverse ? focusableCount - 1 : 0;
  }
  return reverse
    ? (currentIndex - 1 + focusableCount) % focusableCount
    : (currentIndex + 1) % focusableCount;
}

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

const inertCounts = new WeakMap<Element, number>();

function pushInert(el: Element): void {
  const prev = inertCounts.get(el) ?? 0;
  inertCounts.set(el, prev + 1);
  if (prev === 0 && "inert" in el) {
    (el as HTMLElement).inert = true;
  }
}

function popInert(el: Element): void {
  const prev = inertCounts.get(el) ?? 0;
  if (prev <= 1) {
    inertCounts.delete(el);
    if ("inert" in el) {
      (el as HTMLElement).inert = false;
    }
  } else {
    inertCounts.set(el, prev - 1);
  }
}

/**
 * Manages modal dialog accessibility: traps Tab focus, marks background DOM
 * `inert` (stacked safely for nested confirms), and restores caller focus on
 * close.
 */
export function useDialogFocus(
  open: boolean,
  containerRef: RefObject<HTMLElement | null>,
  options?: {
    initialFocusRef?: RefObject<HTMLElement | null>;
    restoreFocus?: boolean;
  },
): void {
  useEffect(() => {
    if (!open || typeof document === "undefined") return;
    const container = containerRef.current;
    if (container === null) return;

    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;

    // Locate the top-level subtree under body holding the dialog (either #root
    // or a backdrop portal) and mark sibling branches `inert`.
    const hiddenSiblings: Element[] = [];
    for (const child of Array.from(document.body.children)) {
      if (child === container || child.contains(container)) continue;
      if (
        child instanceof HTMLScriptElement ||
        child instanceof HTMLLinkElement
      ) {
        continue;
      }
      pushInert(child);
      hiddenSiblings.push(child);
    }

    const focusTarget =
      options?.initialFocusRef?.current ??
      container.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
    if (focusTarget && !container.contains(document.activeElement)) {
      focusTarget.focus({ preventScroll: true });
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter(
        (el) => el.offsetParent !== null || el === document.activeElement,
      );
      if (focusable.length === 0) return;
      const active = document.activeElement as HTMLElement | null;
      const currentIndex = active ? focusable.indexOf(active) : -1;
      const nextIndex = nextDialogFocusIndex(
        currentIndex,
        focusable.length,
        event.shiftKey,
      );
      if (nextIndex >= 0 && focusable[nextIndex]) {
        event.preventDefault();
        focusable[nextIndex]!.focus();
      }
    };

    container.addEventListener("keydown", onKeyDown);
    return () => {
      container.removeEventListener("keydown", onKeyDown);
      for (const el of hiddenSiblings) {
        popInert(el);
      }
      if (
        options?.restoreFocus !== false &&
        previouslyFocused !== null &&
        previouslyFocused.isConnected
      ) {
        previouslyFocused.focus({ preventScroll: true });
      }
    };
  }, [open, containerRef, options?.initialFocusRef, options?.restoreFocus]);
}
