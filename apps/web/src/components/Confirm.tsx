import { useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";

import { useDialogFocus } from "../lib/dialog-focus";

/**
 * The console's one way to ask before doing something irreversible.
 *
 * `window.confirm` is the operating system's dialog: it ignores the product's
 * type, colour and radius, it cannot be styled, and on Windows it blocks the
 * whole renderer while it is open. This asks the same question on the
 * console's own surface, so a destructive action looks like part of the
 * product it is about to change rather than a browser warning.
 */

export interface ConfirmProps {
  open: boolean;
  title: string;
  /** One or two sentences on what will happen. */
  description?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** A red confirm button, for actions that destroy or detach something. */
  danger?: boolean;
  /** The action is in flight; both buttons are disabled. */
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function Confirm({
  open,
  title,
  description,
  confirmLabel = "确定",
  cancelLabel = "取消",
  danger = true,
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmProps) {
  const dialogId = useId();
  const titleId = `${dialogId}-title`;
  const descId = `${dialogId}-desc`;
  const cancelRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(open, dialogRef, { initialFocusRef: cancelRef });

  // Escape always means "no", and must never bubble out to close an underlying
  // surface even while an irreversible action is in flight.
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      event.stopImmediatePropagation();
      if (!busy) onCancel();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, busy, onCancel]);

  if (!open) return null;

  return createPortal(
    <div
      className="cds-confirm-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        ref={dialogRef}
        className="cds-confirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description !== undefined ? descId : undefined}
      >
        <h2 className="cds-confirm-title" id={titleId}>
          {title}
        </h2>
        {description !== undefined && (
          <p className="cds-confirm-desc" id={descId}>
            {description}
          </p>
        )}
        <div className="cds-confirm-actions">
          <button
            type="button"
            ref={cancelRef}
            className="cds-confirm-btn"
            disabled={busy}
            onClick={onCancel}
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            className={`cds-confirm-btn cds-confirm-primary ${danger ? "danger" : ""}`}
            disabled={busy}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
