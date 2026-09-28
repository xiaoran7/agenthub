import { LogOut, Settings } from "lucide-react";
import type { Snapshot } from "../../types";

export function AccountMenu({
  snapshot,
  busy,
  onClose,
  onOpenSettings,
  onLogout,
}: {
  snapshot: Snapshot;
  busy: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
  onLogout: () => void;
}) {
  return (
    <aside className="account-menu" aria-label="账号菜单">
      <button
        className="account-menu-backdrop"
        aria-label="关闭账号菜单"
        onClick={onClose}
      />
      <div className="account-menu-card">
        <div className="account-menu-identity">
          <strong>{snapshot.account.user.display_name || "未设置名称"}</strong>
          <span>{snapshot.account.user.email}</span>
          <small>
            {snapshot.account.offline
              ? "本地工作区 · 未连接 Gateway"
              : snapshot.realtime_connected
                ? "实时连接"
                : "在线连接"}
          </small>
        </div>
        <button type="button" onClick={onOpenSettings}>
          <Settings size={15} />
          <span>设置</span>
          <small>Ctrl+,</small>
        </button>
        <div className="account-menu-separator" />
        <button type="button" disabled={busy} onClick={onLogout}>
          <LogOut size={15} />
          <span>
            {snapshot.account.offline ? "切换连接 / 登录" : "退出登录"}
          </span>
        </button>
      </div>
    </aside>
  );
}
