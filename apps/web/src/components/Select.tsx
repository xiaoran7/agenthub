"use client";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

/** The widest the menu may get, before the viewport clamps it. */
const MAX_MENU_WIDTH = 560;
/** Room kept between the menu and the window's edges. */
const VIEWPORT_GUTTER = 12;

/**
 * Guess how wide a row wants to be, in pixels.
 *
 * The menu is measured before it is mounted, so there is no box to read. A
 * CJK glyph is roughly one em; a Latin character roughly half of one at this
 * size. The guess only has to be good enough to keep short titles on one line
 * — the mounted menu measures itself and corrects it.
 */
function estimateWidth(options: readonly SelectOption[]): number {
  let widest = 0;
  for (const option of options) {
    const label = option.label;
    let advance = 0;
    for (const char of label) {
      advance += /[　-鿿＀-￯]/u.test(char) ? 13.5 : 6.9;
    }
    // Tag chip, its gap, the check, and the row's own padding.
    const tag = option.tag === undefined ? 0 : 92;
    widest = Math.max(widest, advance + tag + 52);
  }
  return Math.min(widest, MAX_MENU_WIDTH);
}

/**
 * The console's single dropdown.
 *
 * A native `<select>` renders the operating system's own menu, which ignores
 * the product's typography, radii and theme; on Windows it is the one control
 * in the console that still looks like a form from another application. This
 * replaces it with a button plus a listbox popover that reuses the same
 * surface, border and shadow tokens as the device and agent popovers.
 *
 * The popover is portalled to `document.body` and positioned from the button's
 * rect: the launch dialog scrolls its own content, and an absolutely positioned
 * menu inside it would be clipped at the dialog's edge.
 */

export interface SelectOption {
  value: string;
  label: string;
  /** Optional second line, shown muted under the label. */
  description?: string;
  /** Short right-aligned chip, e.g. the engine that owns the row. */
  tag?: string;
  /** Provider key for the chip's colour, matching the tree's palette. */
  tagTone?: string;
  /** Full text for the native tooltip when `label` is a shortened form. */
  title?: string;
  disabled?: boolean;
}

interface SelectProps {
  value: string;
  options: readonly SelectOption[];
  onChange: (value: string) => void;
  /** Accessible name; also the popover's heading when no `label` is shown. */
  label: string;
  /** Shown in the trigger when `value` matches no option. */
  placeholder?: string;
  disabled?: boolean;
  /** `compact` is the composer's inline variant; `block` fills its container. */
  variant?: "block" | "compact";
  className?: string;
  title?: string;
}

interface Rect {
  left: number;
  top: number;
  width: number;
  /** True when the menu opens upward because there is no room below. */
  above: boolean;
}

export function Select({
  value,
  options,
  onChange,
  label,
  placeholder,
  disabled = false,
  variant = "block",
  className = "",
  title,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [rect, setRect] = useState<Rect | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const selectedIndex = useMemo(
    () => options.findIndex((option) => option.value === value),
    [options, value],
  );
  const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  const enabledIndexes = useMemo(
    () =>
      options
        .map((option, index) => (option.disabled ? -1 : index))
        .filter((index) => index >= 0),
    [options],
  );

  const measure = useCallback(() => {
    const button = buttonRef.current;
    if (button === null) return;
    const box = button.getBoundingClientRect();
    const menuHeight = Math.min(options.length * 40 + 12, 320);
    const below = window.innerHeight - box.bottom;
    const above = below < menuHeight + 12 && box.top > menuHeight + 12;

    // The menu is as wide as its widest row needs, never narrower than the
    // trigger and never wider than the window: a session title is far longer
    // than the 40px field it is displayed in, and a menu clamped to the field
    // would ellipsize every row it exists to let the reader compare. The
    // estimate is monospace-ish per character and refined once the menu has
    // mounted (see `refineWidth`), because the font is the page's, not ours.
    const widest = Math.min(
      window.innerWidth - VIEWPORT_GUTTER * 2,
      MAX_MENU_WIDTH,
    );
    const wanted = Math.max(box.width, estimateWidth(options));
    const width = Math.min(wanted, widest);
    const left = Math.max(
      VIEWPORT_GUTTER,
      Math.min(box.left, window.innerWidth - width - VIEWPORT_GUTTER),
    );
    setRect({
      left,
      top: above ? box.top - 6 : box.bottom + 6,
      width,
      above,
    });
  }, [options.length, options]);

  const close = useCallback((restoreFocus: boolean) => {
    setOpen(false);
    setActiveIndex(-1);
    if (restoreFocus) buttonRef.current?.focus();
  }, []);

  const openMenu = useCallback(() => {
    if (disabled || options.length === 0) return;
    measure();
    setOpen(true);
    setActiveIndex(
      selectedIndex >= 0 ? selectedIndex : (enabledIndexes[0] ?? -1),
    );
  }, [disabled, options.length, measure, selectedIndex, enabledIndexes]);

  const commit = useCallback(
    (index: number) => {
      const option = options[index];
      if (option === undefined || option.disabled) return;
      onChange(option.value);
      close(true);
    },
    [options, onChange, close],
  );

  // A menu anchored to a button has to follow that button while the page
  // scrolls or the window resizes; otherwise it detaches and floats.
  useEffect(() => {
    if (!open) return;
    const onReflow = () => measure();
    window.addEventListener("resize", onReflow);
    window.addEventListener("scroll", onReflow, true);
    return () => {
      window.removeEventListener("resize", onReflow);
      window.removeEventListener("scroll", onReflow, true);
    };
  }, [open, measure]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (
        buttonRef.current?.contains(target) ||
        menuRef.current?.contains(target)
      ) {
        return;
      }
      close(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [open, close]);

  // The guess above is only a guess: the rows are laid out in the page's own
  // font. Once they mount, each row's natural width is read from its clipped
  // label (whose `scrollWidth` is the full text, not the ellipsis) plus its
  // chip and check, and the menu takes the widest one. The effect runs once
  // per opening, so the menu does not resize while the keyboard walks it.
  useLayoutEffect(() => {
    if (!open || rect === null) return;
    const menu = menuRef.current;
    if (menu === null) return;
    let wanted = rect.width;
    for (const row of menu.querySelectorAll<HTMLElement>(
      ".cds-select-option",
    )) {
      const label = row.querySelector<HTMLElement>(".cds-select-option-label");
      if (label === null) continue;
      const tag = row.querySelector<HTMLElement>(".cds-select-tag");
      const check = row.querySelector<HTMLElement>(".cds-select-check");
      const rowWidth =
        label.scrollWidth +
        20 + // the row's own left and right padding
        (tag === null ? 0 : tag.offsetWidth + 8) +
        (check === null ? 0 : check.offsetWidth + 8);
      wanted = Math.max(wanted, rowWidth);
    }
    // Row padding, the menu's padding and its border sit outside the content.
    const capped = Math.min(
      wanted + 12,
      MAX_MENU_WIDTH,
      window.innerWidth - VIEWPORT_GUTTER * 2,
    );
    if (capped <= 0 || Math.abs(capped - rect.width) < 2) return;
    const left = Math.max(
      VIEWPORT_GUTTER,
      Math.min(rect.left, window.innerWidth - capped - VIEWPORT_GUTTER),
    );
    setRect({ ...rect, width: capped, left });
    // Only the rows that mounted decide this. Re-running on every rect change
    // would loop, because this effect writes the rect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Keep the highlighted row in view when the keyboard walks past the fold.
  useLayoutEffect(() => {
    if (!open || activeIndex < 0) return;
    const row = menuRef.current?.querySelector<HTMLElement>(
      `[data-index="${activeIndex}"]`,
    );
    row?.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (disabled) return;
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
        event.preventDefault();
        openMenu();
      }
      return;
    }
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        close(true);
        break;
      case "Tab":
        close(false);
        break;
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        if (enabledIndexes.length === 0) break;
        const current = enabledIndexes.indexOf(activeIndex);
        const next =
          current === -1
            ? enabledIndexes[step === 1 ? 0 : enabledIndexes.length - 1]
            : enabledIndexes[
                (current + step + enabledIndexes.length) % enabledIndexes.length
              ];
        setActiveIndex(next ?? -1);
        break;
      }
      case "Home":
        event.preventDefault();
        setActiveIndex(enabledIndexes[0] ?? -1);
        break;
      case "End":
        event.preventDefault();
        setActiveIndex(enabledIndexes[enabledIndexes.length - 1] ?? -1);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        commit(activeIndex);
        break;
      default:
        break;
    }
  };

  const empty = options.length === 0;

  return (
    <>
      <button
        type="button"
        ref={buttonRef}
        className={`cds-select-trigger cds-select-${variant} ${open ? "open" : ""} ${className}`}
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-controls={open ? listId : undefined}
        aria-label={label}
        disabled={disabled || empty}
        title={title ?? label}
        onClick={() => (open ? close(true) : openMenu())}
        onKeyDown={onKeyDown}
      >
        <span className="cds-select-value">
          {selected?.label ?? placeholder ?? ""}
        </span>
        <svg
          className={`cds-select-chevron ${open ? "open" : ""}`}
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {open &&
        rect !== null &&
        createPortal(
          <div
            ref={menuRef}
            id={listId}
            className="cds-select-menu"
            role="listbox"
            aria-label={label}
            tabIndex={-1}
            style={{
              left: `${rect.left}px`,
              width: `${rect.width}px`,
              ...(rect.above
                ? { top: `${rect.top}px`, transform: "translateY(-100%)" }
                : { top: `${rect.top}px` }),
            }}
            onKeyDown={onKeyDown}
          >
            {options.map((option, index) => {
              const isSelected = index === selectedIndex;
              return (
                <button
                  type="button"
                  key={`${option.value}-${index}`}
                  data-index={index}
                  role="option"
                  aria-selected={isSelected}
                  disabled={option.disabled}
                  className={`cds-select-option ${
                    isSelected ? "selected" : ""
                  } ${index === activeIndex ? "active" : ""}`}
                  onMouseEnter={() => {
                    if (!option.disabled) setActiveIndex(index);
                  }}
                  onClick={() => commit(index)}
                >
                  <span
                    className="cds-select-option-text"
                    title={option.title ?? option.label}
                  >
                    <span className="cds-select-option-label">
                      {option.label}
                    </span>
                    {option.description !== undefined && (
                      <span className="cds-select-option-desc">
                        {option.description}
                      </span>
                    )}
                  </span>
                  {option.tag !== undefined && (
                    <span
                      className="cds-select-tag"
                      data-tone={option.tagTone ?? "other"}
                    >
                      {option.tag}
                    </span>
                  )}
                  {isSelected && (
                    <svg
                      className="cds-select-check"
                      width="13"
                      height="13"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden="true"
                    >
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  )}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </>
  );
}
