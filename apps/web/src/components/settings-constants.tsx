import type React from "react";

import type { AuditEvent } from "../lib/account";
import type { NotificationState } from "../lib/notifications";
import type { PwaInstallState } from "../lib/pwa-install";

export type SettingsSectionKey =
  | "appearance"
  | "plugins"
  | "account"
  | "audit"
  | "connection"
  | "devices"
  | "notifications"
  | "install";

export function PluginsIcon({
  size = 18,
  className,
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
    </svg>
  );
}

export function ArrowLeftIcon({
  size = 14,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <line x1="19" y1="12" x2="5" y2="12" />
      <polyline points="12 19 5 12 12 5" />
    </svg>
  );
}

export function PaletteIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <path d="M12 2a10 10 0 0 1 0 20" fill="currentColor" fillOpacity="0.25" />
    </svg>
  );
}

export function ShieldIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
    </svg>
  );
}

export function AuditIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="16" y1="13" x2="8" y2="13" />
      <line x1="16" y1="17" x2="8" y2="17" />
    </svg>
  );
}

export function ConnectionIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="2" y1="12" x2="22" y2="12" />
      <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
    </svg>
  );
}

export function DevicesIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
      <line x1="8" y1="21" x2="16" y2="21" />
      <line x1="12" y1="17" x2="12" y2="21" />
    </svg>
  );
}

export function NotificationIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
  );
}

export function InstallIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="2" y="3" width="20" height="18" rx="3" />
      <line x1="2" y1="9" x2="22" y2="9" />
      <circle cx="6" cy="6" r="1" fill="currentColor" />
      <circle cx="10" cy="6" r="1" fill="currentColor" />
    </svg>
  );
}

export function SunIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    </svg>
  );
}

export function MoonIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

export function MonitorIcon({
  size = 16,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
      <line x1="8" y1="21" x2="16" y2="21" />
      <line x1="12" y1="17" x2="12" y2="21" />
    </svg>
  );
}

export interface SettingsSectionEntry {
  key: SettingsSectionKey;
  label: string;
  icon: (props: { size?: number; className?: string }) => React.JSX.Element;
  requiresAccount: boolean;
}

export const SETTINGS_SECTIONS: readonly SettingsSectionEntry[] = [
  {
    key: "appearance",
    label: "外观主题",
    icon: PaletteIcon,
    requiresAccount: false,
  },
  {
    key: "plugins",
    label: "插件扩展",
    icon: PluginsIcon,
    requiresAccount: false,
  },
  {
    key: "account",
    label: "账户安全",
    icon: ShieldIcon,
    requiresAccount: true,
  },
  { key: "audit", label: "审计记录", icon: AuditIcon, requiresAccount: true },
  {
    key: "connection",
    label: "连接",
    icon: ConnectionIcon,
    requiresAccount: false,
  },
  { key: "devices", label: "设备", icon: DevicesIcon, requiresAccount: false },
  {
    key: "notifications",
    label: "等待提醒",
    icon: NotificationIcon,
    requiresAccount: false,
  },
  {
    key: "install",
    label: "安装到设备",
    icon: InstallIcon,
    requiresAccount: false,
  },
];

export const NOTIFICATION_HINTS: Record<NotificationState, string> = {
  unavailable:
    "当前环境不支持通知：需要 https 或 localhost，且浏览器支持推送。",
  disabled: "网关未配置推送密钥，通知整体关闭。",
  denied: "浏览器已拒绝本站通知权限，需要在站点设置里恢复。",
  on: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
  off: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
};

export const INSTALL_HINTS: Record<PwaInstallState, string> = {
  checking: "正在检查当前浏览器的安装能力…",
  available: "安装后可从桌面或主屏幕直接打开，并使用独立窗口。",
  installing: "正在等待浏览器确认安装…",
  submitted: "安装请求已提交；完成后可从系统应用列表打开。",
  installed: "当前已经在独立应用模式中运行。",
  manual: '在 Safari 中点 "分享"，然后选择 "添加到主屏幕"。',
  unavailable: "当前浏览器暂未提供安装按钮；仍可正常使用网页版本。",
};

export const INSTALL_LABELS: Record<PwaInstallState, string> = {
  checking: "检查中…",
  available: "安装应用",
  installing: "安装中…",
  submitted: "已提交",
  installed: "已安装",
  manual: "查看说明",
  unavailable: "暂不可用",
};

export function roleLabel(role: "owner" | "admin" | "member"): string {
  return role === "owner" ? "所有者" : role === "admin" ? "管理员" : "成员";
}

export const AUDIT_ACTION_LABELS: Record<AuditEvent["action"], string> = {
  "account.created": "账户已创建",
  "auth.session_created": "登录会话已建立",
  "auth.session_revoked": "登录会话已撤销",
  "auth.other_sessions_revoked": "其他登录会话已撤销",
  "auth.email_verification_requested": "邮箱验证已请求",
  "auth.email_verified": "邮箱已验证",
  "auth.password_reset_requested": "密码重置已请求",
  "auth.password_reset_completed": "密码已重置",
  "auth.password_changed": "密码已更新",
  "workspace.switched": "当前工作区已切换",
  "team.invitation_created": "团队邀请已创建",
  "team.invitation_revoked": "团队邀请已撤销",
  "team.invitation_accepted": "团队邀请已接受",
  "team.member_role_updated": "成员角色已更新",
  "team.member_removed": "成员已移出工作区",
  "subscription.updated": "订阅权益已更新",
  "device.binding_created": "设备绑定码已创建",
  "device.registered": "设备已登记",
  "device.revoked": "设备已撤销",
  "task.created": "远程任务已创建",
  "task.canceled": "远程任务已取消",
  "task.claimed": "设备已领取任务",
  "task.status_updated": "任务状态已更新",
};

function auditId(value: string | undefined): string | undefined {
  return value === undefined ? undefined : value.slice(0, 8);
}

export function auditDetail(event: AuditEvent): string {
  const details: string[] = [];
  const target = auditId(event.target_id);
  if (target !== undefined) details.push(`目标 ${target}`);
  const role = event.metadata.role;
  if (role === "owner" || role === "admin" || role === "member") {
    details.push(`角色 ${roleLabel(role)}`);
  }
  const plan = event.metadata.plan;
  if (typeof plan === "string") details.push(`方案 ${plan.toUpperCase()}`);
  const status = event.metadata.status;
  if (typeof status === "string") details.push(`状态 ${status}`);
  const revoked = event.metadata.revoked_sessions;
  if (typeof revoked === "number") details.push(`撤销 ${revoked} 个会话`);
  const agentType = event.metadata.agent_type;
  if (typeof agentType === "string") details.push(`Agent ${agentType}`);
  const previousStatus = event.metadata.previous_status;
  if (typeof previousStatus === "string")
    details.push(`原状态 ${previousStatus}`);
  const taskStatus = event.metadata.task_status;
  if (typeof taskStatus === "string") details.push(`任务状态 ${taskStatus}`);
  const actor = auditId(event.actor_user_id);
  if (event.scope === "workspace" && actor !== undefined) {
    details.push(`操作者 ${actor}`);
  }
  return details.join(" · ");
}
