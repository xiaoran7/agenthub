import { useEffect, useRef } from "react";
import { Icon, type IconName } from "./Icon";

export interface SlashCommandOption {
  command: string;
  description: string;
  scope: "hub" | "native";
  icon: IconName;
}

export const AVAILABLE_SLASH_COMMANDS: SlashCommandOption[] = [
  {
    command: "/cancel",
    description: "立即中止当前 Agent 的流式输出或工具执行",
    scope: "hub",
    icon: "close",
  },
  {
    command: "/permission",
    description:
      "动态变更当前 Session 的审批策略 (如 /permission accept_edits)",
    scope: "hub",
    icon: "filter",
  },
  {
    command: "/compact",
    description: "触发上下文剪枝压缩，减少 Token 消耗 (Claude)",
    scope: "native",
    icon: "activity",
  },
  {
    command: "/cost",
    description: "获取当前会话准确消耗与模型调用明细 (Claude)",
    scope: "native",
    icon: "overview",
  },
  {
    command: "/review",
    description: "触发代码 Diff 审查流程与质量评估",
    scope: "native",
    icon: "check",
  },
  {
    command: "/doctor",
    description: "输出当前开发环境与工具依赖诊断报告",
    scope: "native",
    icon: "diagnostics",
  },
  {
    command: "/undo",
    description: "撤销上一个 Turn 的代码生成与文件修改 (Codex)",
    scope: "native",
    icon: "arrow-left",
  },
  {
    command: "/clear",
    description: "清空前端视图渲染（保留底层执行历史）",
    scope: "hub",
    icon: "clock",
  },
];

interface SlashCommandMenuProps {
  filter: string;
  selectedIndex: number;
  onSelect: (command: string) => void;
}

export function SlashCommandMenu({
  filter,
  selectedIndex,
  onSelect,
}: SlashCommandMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  const filtered = AVAILABLE_SLASH_COMMANDS.filter((cmd) =>
    cmd.command.toLowerCase().startsWith(filter.toLowerCase()),
  );

  if (filtered.length === 0) return null;

  return (
    <div
      ref={menuRef}
      className="slash-command-menu"
      style={{
        position: "absolute",
        bottom: "100%",
        left: 0,
        right: 0,
        marginBottom: "8px",
        background: "var(--paper)",
        border: "1px solid var(--line)",
        borderRadius: "8px",
        boxShadow: "0 8px 24px rgba(0, 0, 0, 0.15)",
        maxHeight: "240px",
        overflowY: "auto",
        zIndex: 50,
      }}
    >
      <div
        style={{
          padding: "6px 12px",
          fontSize: "11px",
          fontWeight: 700,
          textTransform: "uppercase",
          color: "var(--text-muted)",
          borderBottom: "1px solid var(--line)",
        }}
      >
        Slash 指令智能路由 (Slash Commands)
      </div>
      {filtered.map((item, index) => {
        const isSelected = index === selectedIndex;
        return (
          <div
            key={item.command}
            onClick={() => onSelect(item.command)}
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "8px 12px",
              cursor: "pointer",
              background: isSelected
                ? "var(--paper-subtle, #f5f5f5)"
                : "transparent",
              borderLeft: isSelected
                ? "3px solid var(--blue)"
                : "3px solid transparent",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
              <Icon name={item.icon} size={16} />
              <strong style={{ fontFamily: "monospace", fontSize: "13px" }}>
                {item.command}
              </strong>
              <span style={{ fontSize: "12px", color: "var(--text-muted)" }}>
                {item.description}
              </span>
            </div>
            <span
              style={{
                fontSize: "10px",
                fontWeight: 600,
                textTransform: "uppercase",
                padding: "2px 6px",
                borderRadius: "3px",
                backgroundColor:
                  item.scope === "hub"
                    ? "var(--blue-subtle, #e0f2fe)"
                    : "var(--paper)",
                color:
                  item.scope === "hub" ? "var(--blue)" : "var(--text-muted)",
                border: "1px solid var(--line)",
              }}
            >
              {item.scope}
            </span>
          </div>
        );
      })}
    </div>
  );
}
