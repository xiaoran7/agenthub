/**
 * Copies plain text to the system clipboard using the async Clipboard API
 * first, falling back to a temporary off-screen `<textarea>` and
 * `document.execCommand("copy")` when unavailable or rejected (for example on
 * non-secure origins or older mobile browsers). Returns `true` only when a copy
 * actually succeeded.
 */
export async function copyTextToClipboard(text: string): Promise<boolean> {
  if (!text) return false;

  if (
    typeof navigator !== "undefined" &&
    typeof navigator.clipboard?.writeText === "function"
  ) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Fall through to legacy textarea copy fallback.
    }
  }

  if (typeof document === "undefined" || !document.body) {
    return false;
  }

  let textarea: HTMLTextAreaElement | null = null;
  try {
    textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.top = "0";
    textarea.style.left = "-9999px";
    textarea.style.opacity = "0";
    textarea.style.pointerEvents = "none";
    document.body.appendChild(textarea);
    textarea.focus({ preventScroll: true });
    textarea.select();
    textarea.setSelectionRange(0, text.length);
    return document.execCommand("copy") === true;
  } catch {
    return false;
  } finally {
    if (textarea !== null && textarea.parentNode !== null) {
      textarea.parentNode.removeChild(textarea);
    }
  }
}
