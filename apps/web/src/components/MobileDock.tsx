import type { SessionTab } from "../lib/classify";

interface MobileDockProps {
  selected: SessionTab;
  onSelect: (tab: SessionTab) => void;
  onOpenFilters: () => void;
  onOpenSettings: () => void;
}

const ITEMS: ReadonlyArray<{
  tab: SessionTab;
  icon: string;
  label: string;
}> = [
  { tab: "all", icon: "⌂", label: "全部" },
  { tab: "live", icon: "◉", label: "活动" },
  { tab: "waiting", icon: "!", label: "待处理" },
];

export function MobileDock({
  selected,
  onSelect,
  onOpenFilters,
  onOpenSettings,
}: MobileDockProps) {
  return (
    <nav className="mobile-dock" aria-label="移动端快捷导航">
      {ITEMS.map((item) => (
        <button
          type="button"
          key={item.tab}
          data-selected={selected === item.tab}
          aria-current={selected === item.tab ? "page" : undefined}
          onClick={() => onSelect(item.tab)}
        >
          <span aria-hidden="true">{item.icon}</span>
          {item.label}
        </button>
      ))}
      <button type="button" onClick={onOpenFilters}>
        <span aria-hidden="true">◫</span>
        设备
      </button>
      <button type="button" onClick={onOpenSettings}>
        <span aria-hidden="true">⚙</span>
        设置
      </button>
    </nav>
  );
}
