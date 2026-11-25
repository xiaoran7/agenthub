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

/** The widest the desktop menu may get, before the viewport clamps it. */
const MAX_MENU_WIDTH = 560;
/** Room kept between the menu and the window's edges. */
const VIEWPORT_GUTTER = 12;
/** Mobile breakpoint where the dropdown upgrades to a thumb-friendly bottom sheet. */
const MOBILE_SHEET_BREAKPOINT = 700;

function estimateWidth(options: readonly SelectOption[]): number {
  let widest = 0;
  for (const option of options) {
    const label = option.label;
    let advance = 0;
    for (const char of label) {
      advance += /[　-鿿＀-￯]/u.test(char) ? 13.5 : 6.9;
    }
    const tag = option.tag === undefined ? 0 : 92;
    widest = Math.max(widest, advance + tag + 52);
  }
  return Math.min(widest, MAX_MENU_WIDTH);
}

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
  /** Explicitly prefer menu opening direction: "top", "bottom", or "auto". */
  placement?: "auto" | "top" | "bottom";
  className?: string;
  title?: string;
}

interface Rect {
  left: number;
  top: number;
  bottom: number;
  width: number;
  maxHeight: number;
  /** True when the menu opens upward because there is no room below. */
  above: boolean;
  /** True when viewport <= 700px so the menu renders as a bottom sheet. */
  mobileSheet: boolean;
}

export function Select({
  value,
  options,
  onChange,
  label,
  placeholder,
  disabled = false,
  variant = "block",
  placement = "auto",
  className = "",
  title,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [filterText, setFilterText] = useState("");
  const [rect, setRect] = useState<Rect | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const filterInputRef = useRef<HTMLInputElement>(null);
  const listId = useId();

  const selectedIndex = useMemo(
    () => options.findIndex((option) => option.value === value),
    [options, value],
  );
  const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  const showFilter = useMemo(() => {
    if (!rect) return false;
    return rect.mobileSheet ? options.length >= 8 : options.length >= 14;
  }, [rect, options.length]);

  const filteredEntries = useMemo(() => {
    const q = filterText.trim().toLowerCase();
    if (!q) {
      return options.map((option, originalIndex) => ({
        option,
        originalIndex,
      }));
    }
    return options
      .map((option, originalIndex) => ({ option, originalIndex }))
      .filter(({ option }) => {
        const hay =
          `${option.label} ${option.description ?? ""} ${option.tag ?? ""} ${option.value}`.toLowerCase();
        return hay.includes(q);
      });
  }, [options, filterText]);

  const enabledOriginalIndexes = useMemo(
    () =>
      filteredEntries
        .filter(({ option }) => !option.disabled)
        .map(({ originalIndex }) => originalIndex),
    [filteredEntries],
  );

  const measure = useCallback(() => {
    const button = buttonRef.current;
    if (button === null) return;
    const vpWidth = window.visualViewport?.width ?? window.innerWidth;
    const vpHeight = window.visualViewport?.height ?? window.innerHeight;
    const mobileSheet = window.innerWidth <= MOBILE_SHEET_BREAKPOINT;

    if (mobileSheet) {
      setRect({
        left: 0,
        top: 0,
        bottom: 0,
        width: window.innerWidth,
        maxHeight: Math.max(220, Math.round(vpHeight * 0.72)),
        above: true,
        mobileSheet: true,
      });
      return;
    }

    const box = button.getBoundingClientRect();
    const naturalHeight = Math.min(options.length * 40 + 16, 320);
    const spaceBelow = vpHeight - box.bottom - VIEWPORT_GUTTER - 6;
    const spaceAbove = box.top - VIEWPORT_GUTTER - 6;
    const above =
      placement === "top"
        ? spaceAbove >= 140 || spaceAbove > spaceBelow
        : placement === "bottom"
          ? spaceBelow < 120 && spaceAbove > spaceBelow
          : spaceBelow < naturalHeight && spaceAbove > spaceBelow;
    const availableSpace = above ? spaceAbove : spaceBelow;
    const maxHeight = Math.max(132, Math.min(320, Math.floor(availableSpace)));

    const widest = Math.min(vpWidth - VIEWPORT_GUTTER * 2, MAX_MENU_WIDTH);
    const wanted = Math.max(box.width, estimateWidth(options));
    const width = Math.min(wanted, widest);
    const left = Math.max(
      VIEWPORT_GUTTER,
      Math.min(box.left, vpWidth - width - VIEWPORT_GUTTER),
    );
    setRect({
      left,
      top: box.bottom + 6,
      bottom: Math.max(VIEWPORT_GUTTER, window.innerHeight - box.top + 6),
      width,
      maxHeight,
      above,
      mobileSheet: false,
    });
  }, [options, placement]);

  const close = useCallback((restoreFocus: boolean) => {
    setOpen(false);
    setActiveIndex(-1);
    setFilterText("");
    if (restoreFocus) {
      buttonRef.current?.focus({ preventScroll: true });
    }
  }, []);

  const openMenu = useCallback(() => {
    if (disabled || options.length === 0) return;
    setFilterText("");
    measure();
    setOpen(true);
    setActiveIndex(
      selectedIndex >= 0 ? selectedIndex : (enabledOriginalIndexes[0] ?? -1),
    );
  }, [
    disabled,
    options.length,
    measure,
    selectedIndex,
    enabledOriginalIndexes,
  ]);

  const commit = useCallback(
    (index: number) => {
      const option = options[index];
      if (option === undefined || option.disabled) return;
      onChange(option.value);
      close(true);
    },
    [options, onChange, close],
  );

  useEffect(() => {
    if (!open) return;
    const onReflow = () => measure();
    window.addEventListener("resize", onReflow);
    window.addEventListener("scroll", onReflow, true);
    window.visualViewport?.addEventListener("resize", onReflow);
    return () => {
      window.removeEventListener("resize", onReflow);
      window.removeEventListener("scroll", onReflow, true);
      window.visualViewport?.removeEventListener("resize", onReflow);
    };
  }, [open, measure]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (
        buttonRef.current?.contains(target) ||
        menuRef.current?.contains(target)
      ) {
        return;
      }
      close(false);
    };
    const onWindowEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close(true);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onWindowEscape, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onWindowEscape, true);
    };
  }, [open, close]);

  useLayoutEffect(() => {
    if (!open || rect === null || rect.mobileSheet) return;
    const menu = menuRef.current;
    if (menu === null) return;
    let wanted = rect.width;
    for (const row of menu.querySelectorAll<HTMLElement>(
      ".cds-select-option",
    )) {
      const rowLabel = row.querySelector<HTMLElement>(
        ".cds-select-option-label",
      );
      if (rowLabel === null) continue;
      const tag = row.querySelector<HTMLElement>(".cds-select-tag");
      const check = row.querySelector<HTMLElement>(".cds-select-check");
      const rowWidth =
        rowLabel.scrollWidth +
        20 +
        (tag === null ? 0 : tag.offsetWidth + 8) +
        (check === null ? 0 : check.offsetWidth + 8);
      wanted = Math.max(wanted, rowWidth);
    }
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

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
    const isFilterFocused = document.activeElement === filterInputRef.current;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case "Tab":
        if (
          !event.shiftKey &&
          showFilter &&
          document.activeElement === buttonRef.current
        ) {
          event.preventDefault();
          filterInputRef.current?.focus();
          break;
        }
        if (event.shiftKey && isFilterFocused) {
          event.preventDefault();
          buttonRef.current?.focus();
          break;
        }
        if (!rect?.mobileSheet) close(false);
        break;
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        if (enabledOriginalIndexes.length === 0) break;
        const current = enabledOriginalIndexes.indexOf(activeIndex);
        const next =
          current === -1
            ? enabledOriginalIndexes[
                step === 1 ? 0 : enabledOriginalIndexes.length - 1
              ]
            : enabledOriginalIndexes[
                (current + step + enabledOriginalIndexes.length) %
                  enabledOriginalIndexes.length
              ];
        setActiveIndex(next ?? -1);
        break;
      }
      case "Home":
        if (!isFilterFocused) {
          event.preventDefault();
          setActiveIndex(enabledOriginalIndexes[0] ?? -1);
        }
        break;
      case "End":
        if (!isFilterFocused) {
          event.preventDefault();
          setActiveIndex(
            enabledOriginalIndexes[enabledOriginalIndexes.length - 1] ?? -1,
          );
        }
        break;
      case " ":
        if (!isFilterFocused) {
          event.preventDefault();
          commit(activeIndex);
        }
        break;
      case "Enter":
        if (event.nativeEvent.isComposing || event.keyCode === 229) {
          break;
        }
        event.preventDefault();
        if (activeIndex >= 0) {
          commit(activeIndex);
        } else if (enabledOriginalIndexes.length > 0) {
          commit(enabledOriginalIndexes[0]!);
        }
        break;
      default:
        break;
    }
  };

  const empty = options.length === 0;
  const activeDescendantId =
    open && activeIndex >= 0 ? `${listId}-opt-${activeIndex}` : undefined;

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
        aria-activedescendant={activeDescendantId}
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
          <>
            {rect.mobileSheet && (
              <div
                className="cds-select-sheet-backdrop"
                aria-hidden="true"
                onClick={() => close(true)}
              />
            )}
            <div
              ref={menuRef}
              id={listId}
              className={`cds-select-menu ${rect.above ? "above" : ""} ${rect.mobileSheet ? "mobile-sheet" : ""}`}
              role="listbox"
              aria-label={label}
              aria-activedescendant={activeDescendantId}
              tabIndex={-1}
              style={
                rect.mobileSheet
                  ? { maxHeight: `${rect.maxHeight}px` }
                  : {
                      left: `${rect.left}px`,
                      width: `${rect.width}px`,
                      maxHeight: `${rect.maxHeight}px`,
                      ...(rect.above
                        ? { bottom: `${rect.bottom}px`, top: "auto" }
                        : { top: `${rect.top}px`, bottom: "auto" }),
                    }
              }
              onKeyDown={onKeyDown}
            >
              {rect.mobileSheet && (
                <div className="cds-select-sheet-header">
                  <span className="cds-select-sheet-pill" aria-hidden="true" />
                  <div className="cds-select-sheet-title-row">
                    <span className="cds-select-sheet-title">{label}</span>
                    <button
                      type="button"
                      className="cds-select-sheet-close"
                      aria-label="关闭选项列表"
                      onClick={() => close(true)}
                    >
                      完成
                    </button>
                  </div>
                </div>
              )}
              {showFilter && (
                <div className="cds-select-filter-wrap">
                  <input
                    ref={filterInputRef}
                    type="search"
                    className="cds-select-filter-input"
                    placeholder={`搜索${label}…`}
                    aria-label={`搜索${label}`}
                    aria-controls={listId}
                    aria-activedescendant={activeDescendantId}
                    value={filterText}
                    onChange={(e) => {
                      const next = e.target.value;
                      setFilterText(next);
                      setActiveIndex(-1);
                    }}
                  />
                </div>
              )}
              <div className="cds-select-options-scroll">
                {filteredEntries.length === 0 ? (
                  <div className="cds-select-empty">无匹配选项</div>
                ) : (
                  filteredEntries.map(({ option, originalIndex }) => {
                    const isSelected = originalIndex === selectedIndex;
                    return (
                      <button
                        type="button"
                        id={`${listId}-opt-${originalIndex}`}
                        key={`${option.value}-${originalIndex}`}
                        data-index={originalIndex}
                        role="option"
                        aria-selected={isSelected}
                        disabled={option.disabled}
                        className={`cds-select-option ${
                          isSelected ? "selected" : ""
                        } ${originalIndex === activeIndex ? "active" : ""}`}
                        onMouseEnter={() => {
                          if (!option.disabled) setActiveIndex(originalIndex);
                        }}
                        onClick={() => commit(originalIndex)}
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
                            width="14"
                            height="14"
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
                  })
                )}
              </div>
            </div>
          </>,
          document.body,
        )}
    </>
  );
}
