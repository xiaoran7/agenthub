import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

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

function focusableElements(dialog: HTMLElement): HTMLElement[] {
  return Array.from(
    dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter((element) => element.getClientRects().length > 0);
}

export function useModalDialog<T extends HTMLElement>(
  onClose: () => void,
  active = true,
  restoreFocus?: () => HTMLElement | null | undefined,
): RefObject<T | null> {
  const dialogRef = useRef<T>(null);

  useEffect(() => {
    if (!active) {
      return;
    }
    const dialog = dialogRef.current;
    if (dialog === null) {
      return;
    }
    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : undefined;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    const focusFrame = window.requestAnimationFrame(() => {
      const first = focusableElements(dialog)[0];
      (first ?? dialog).focus();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }
      const focusable = focusableElements(dialog);
      const currentIndex = focusable.indexOf(
        document.activeElement as HTMLElement,
      );
      const nextIndex = nextDialogFocusIndex(
        currentIndex,
        focusable.length,
        event.shiftKey,
      );
      event.preventDefault();
      if (nextIndex < 0) {
        dialog.focus();
      } else {
        focusable[nextIndex]?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("keydown", onKeyDown);
      document.body.style.overflow = previousOverflow;
      (restoreFocus?.() ?? previouslyFocused)?.focus();
    };
  }, [active, onClose, restoreFocus]);

  return dialogRef;
}
