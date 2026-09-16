import React, { useRef, useEffect } from "react";
import { Paperclip, AtSign, Slash, DollarSign } from "lucide-react";

interface ComposerPlusMenuProps {
  isOpen: boolean;
  onClose: () => void;
  onAttachFiles: () => void;
  onInsertToken: (token: string) => void;
}

export function ComposerPlusMenu({
  isOpen,
  onClose,
  onAttachFiles,
  onInsertToken,
}: ComposerPlusMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose();
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("keydown", handleKeyDown);
      return () => {
        document.removeEventListener("mousedown", handleClickOutside);
        document.removeEventListener("keydown", handleKeyDown);
      };
    }
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="composer-plus-menu" ref={menuRef} role="menu">
      <button
        type="button"
        className="plus-menu-item"
        role="menuitem"
        onClick={() => {
          onClose();
          onAttachFiles();
        }}
      >
        <span className="menu-item-icon">
          <Paperclip size={15} />
        </span>
        <span className="menu-item-label">添加附件</span>
      </button>

      <button
        type="button"
        className="plus-menu-item"
        role="menuitem"
        onClick={() => {
          onClose();
          onInsertToken("@");
        }}
      >
        <span className="menu-item-icon">
          <AtSign size={15} />
        </span>
        <span className="menu-item-label">使用 @ 添加上下文</span>
      </button>

      <button
        type="button"
        className="plus-menu-item"
        role="menuitem"
        onClick={() => {
          onClose();
          onInsertToken("/");
        }}
      >
        <span className="menu-item-icon">
          <Slash size={15} />
        </span>
        <span className="menu-item-label">使用 / 选择能力</span>
      </button>

      <button
        type="button"
        className="plus-menu-item"
        role="menuitem"
        onClick={() => {
          onClose();
          onInsertToken("$");
        }}
      >
        <span className="menu-item-icon">
          <DollarSign size={15} />
        </span>
        <span className="menu-item-label">使用 $ 选择技能</span>
      </button>
    </div>
  );
}
