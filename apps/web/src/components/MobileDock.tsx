import type { SessionTab } from "../lib/classify";

interface MobileDockProps {
  selected: SessionTab;
  counts: Record<SessionTab, number>;
  deviceFilterActive: boolean;
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
  counts,
  deviceFilterActive,
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
          aria-label={
            item.tab === "waiting" && counts.waiting > 0
              ? `${item.label}，${counts.waiting} 条`
              : item.label
          }
          onClick={() => onSelect(item.tab)}
        >
          <span className="mobile-dock-icon" aria-hidden="true">
            {item.icon}
            {item.tab === "waiting" && counts.waiting > 0 && (
              <span className="mobile-dock-badge">
                {counts.waiting > 99 ? "99+" : counts.waiting}
              </span>
            )}
          </span>
          {item.label}
        </button>
      ))}
      <button
        type="button"
        data-filtered={deviceFilterActive}
        aria-label={deviceFilterActive ? "设备，已筛选" : "设备"}
        onClick={onOpenFilters}
      >
        <span className="mobile-dock-icon" aria-hidden="true">
          ◫
        </span>
        设备
      </button>
      <button type="button" onClick={onOpenSettings}>
        <span className="mobile-dock-icon" aria-hidden="true">
          ⚙
        </span>
        设置
      </button>
    </nav>
  );
}
