import React, { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption<T extends string = string> {
  value: T;
  label: string;
  desc?: string;
  icon?: React.ReactNode;
}

interface SelectDropdownProps<T extends string = string> {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  onOpen?: () => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  size?: "sm" | "md" | "lg";
  align?: "left" | "right";
  direction?: "up" | "down" | "auto";
  ariaLabel?: string;
}

export function SelectDropdown<T extends string = string>({
  value,
  options,
  onChange,
  onOpen,
  placeholder = "请选择...",
  disabled = false,
  className = "",
  size = "md",
  align = "left",
  direction = "auto",
  ariaLabel,
}: SelectDropdownProps<T>) {
  const [open, setOpen] = useState(false);
  const [placementUp, setPlacementUp] = useState(direction === "up");
  const containerRef = useRef<HTMLDivElement>(null);

  const selectedOption = options.find((opt) => opt.value === value);

  useEffect(() => {
    if (!open) return;
    if (direction === "up") {
      setPlacementUp(true);
      return;
    }
    if (direction === "down") {
      setPlacementUp(false);
      return;
    }
    if (containerRef.current) {
      const rect = containerRef.current.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      const spaceAbove = rect.top;
      // When space below is tight (<260px) or space above is larger, open upward
      if (spaceBelow < 260 && spaceAbove > spaceBelow) {
        setPlacementUp(true);
      } else {
        setPlacementUp(false);
      }
    }
  }, [open, direction]);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setOpen(false);
      }
    }
    if (open) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => {
        document.removeEventListener("mousedown", handleClickOutside);
      };
    }
  }, [open]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (!open) return;
      if (event.key === "Escape") {
        setOpen(false);
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        const currentIndex = options.findIndex((opt) => opt.value === value);
        const nextIndex = (currentIndex + 1) % options.length;
        if (options[nextIndex]) onChange(options[nextIndex].value);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        const currentIndex = options.findIndex((opt) => opt.value === value);
        const prevIndex = (currentIndex - 1 + options.length) % options.length;
        if (options[prevIndex]) onChange(options[prevIndex].value);
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open, options, value, onChange]);

  return (
    <div
      ref={containerRef}
      className={`custom-select-dropdown ${size} ${open ? "open" : ""} ${placementUp ? "placement-up" : "placement-down"} ${disabled ? "disabled" : ""} ${className}`}
    >
      <button
        type="button"
        className="select-trigger"
        onClick={() => {
          if (disabled) return;
          setOpen((prev) => {
            const next = !prev;
            if (next && onOpen) {
              try {
                onOpen();
              } catch {
                // ignore
              }
            }
            return next;
          });
        }}
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel || selectedOption?.label || placeholder}
      >
        <span className="select-value-wrapper">
          {selectedOption?.icon && (
            <span className="select-icon">{selectedOption.icon}</span>
          )}
          <span className="select-value-text">
            {selectedOption ? selectedOption.label : placeholder}
          </span>
        </span>
        <ChevronDown
          size={14}
          className={`select-chevron ${open ? "rotate" : ""}`}
        />
      </button>

      {open && (
        <div
          className={`select-menu ${align} ${placementUp ? "up" : "down"}`}
          role="listbox"
          tabIndex={-1}
        >
          {options.map((option) => {
            const isSelected = option.value === value;
            return (
              <button
                key={option.value}
                type="button"
                className={`select-item ${isSelected ? "selected" : ""}`}
                role="option"
                aria-selected={isSelected}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <div className="select-item-content">
                  {option.icon && (
                    <span className="item-icon">{option.icon}</span>
                  )}
                  <div className="item-text-group">
                    <span className="item-label">{option.label}</span>
                    {option.desc && (
                      <span className="item-desc">{option.desc}</span>
                    )}
                  </div>
                </div>
                {isSelected && (
                  <Check size={14} className="select-item-check" />
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
