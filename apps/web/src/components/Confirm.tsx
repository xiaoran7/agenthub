"use client";
import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";

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
  const cancelRef = useRef<HTMLButtonElement>(null);

  // Focus lands on the safe choice, not the destructive one, and Escape
  // always means "no".
  useEffect(() => {
    if (!open) return;
    cancelRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.preventDefault();
      onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, busy, onCancel]);

  if (!open) return null;

  return createPortal(
    <div
      className="cds-confirm-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        className="cds-confirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="cds-confirm-title"
        aria-describedby={
          description !== undefined ? "cds-confirm-desc" : undefined
        }
      >
        <h2 className="cds-confirm-title" id="cds-confirm-title">
          {title}
        </h2>
        {description !== undefined && (
          <p className="cds-confirm-desc" id="cds-confirm-desc">
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
