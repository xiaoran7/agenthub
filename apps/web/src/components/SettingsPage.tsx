"use client";

import { useEffect, useRef, useState, type FormEvent } from "react";

import {
  ApiError,
  createDeviceBinding,
  describeApiError,
  fetchDevices,
  revokeDevice,
  type DeviceBinding,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import type { NotificationState } from "../lib/notifications";
import { deviceShortId, formatClock } from "../lib/format";
import { usePwaInstall, type PwaInstallState } from "../lib/pwa-install";
import type { DeviceRecord } from "../lib/types";
import {
  accountErrorMessage,
  changePassword,
  fetchAuditEvents,
  fetchWebSessions,
  requestEmailVerification,
  revokeOtherWebSessions,
  revokeWebSession,
  type AccountView,
  type AuditEvent,
  type AuditPage,
  type AuditScope,
  type WebSessionView,
} from "../lib/account";
import {
  disableNotifications,
  enableNotifications,
  readNotificationState,
} from "../lib/notifications";
import {
  getStoredTheme,
  setTheme,
  initThemeListener,
  type ThemeMode,
} from "../lib/theme";

interface SettingsPageProps {
  account: AccountView | undefined;
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  onWorkspaceChanged: (account: AccountView) => void;
  notificationPresentedAt: number | undefined;
  onLoggedOut: () => void;
  onClose: () => void;
}

type SettingsSectionKey =
  | "appearance"
  | "account"
  | "audit"
  | "connection"
  | "devices"
  | "notifications"
  | "install";

function ArrowLeftIcon({
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

function PaletteIcon({
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

function ShieldIcon({
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

function AuditIcon({
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

function ConnectionIcon({
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

function DevicesIcon({
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

function NotificationIcon({
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

function InstallIcon({
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

function SunIcon({
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

function MoonIcon({
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

function MonitorIcon({
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

interface SettingsSectionEntry {
  key: SettingsSectionKey;
  label: string;
  icon: (props: { size?: number; className?: string }) => React.JSX.Element;
  requiresAccount: boolean;
}

const SETTINGS_SECTIONS: readonly SettingsSectionEntry[] = [
  {
    key: "appearance",
    label: "外观主题",
    icon: PaletteIcon,
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

const NOTIFICATION_HINTS: Record<NotificationState, string> = {
  unavailable:
    "当前环境不支持通知：需要 https 或 localhost，且浏览器支持推送。",
  disabled: "网关未配置推送密钥，通知整体关闭。",
  denied: "浏览器已拒绝本站通知权限，需要在站点设置里恢复。",
  on: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
  off: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
};

const INSTALL_HINTS: Record<PwaInstallState, string> = {
  checking: "正在检查当前浏览器的安装能力…",
  available: "安装后可从桌面或主屏幕直接打开，并使用独立窗口。",
  installing: "正在等待浏览器确认安装…",
  submitted: "安装请求已提交；完成后可从系统应用列表打开。",
  installed: "当前已经在独立应用模式中运行。",
  manual: "在 Safari 中点“分享”，然后选择“添加到主屏幕”。",
  unavailable: "当前浏览器暂未提供安装按钮；仍可正常使用网页版本。",
};

const INSTALL_LABELS: Record<PwaInstallState, string> = {
  checking: "检查中…",
  available: "安装应用",
  installing: "安装中…",
  submitted: "已提交",
  installed: "已安装",
  manual: "查看说明",
  unavailable: "暂不可用",
};

function roleLabel(role: "owner" | "admin" | "member"): string {
  return role === "owner" ? "所有者" : role === "admin" ? "管理员" : "成员";
}

const AUDIT_ACTION_LABELS: Record<AuditEvent["action"], string> = {
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

function auditDetail(event: AuditEvent): string {
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

export function SettingsPage({
  account,
  config,
  onSave,
  onWorkspaceChanged,
  notificationPresentedAt,
  onLoggedOut,
  onClose,
}: SettingsPageProps) {
  const [activeSection, setActiveSection] =
    useState<SettingsSectionKey>("appearance");
  const [currentTheme, setCurrentTheme] = useState<ThemeMode>(() =>
    getStoredTheme(),
  );
  const [gatewayBase, setGatewayBase] = useState(config.gatewayBase);
  const [userId, setUserId] = useState(config.userId);
  const [error, setError] = useState<string | undefined>(undefined);
  const [devices, setDevices] = useState<DeviceRecord[] | undefined>(undefined);
  const { state: installState, install: onInstall } = usePwaInstall();
  const [notifications, setNotifications] = useState<
    NotificationState | undefined
  >(undefined);
  const [notificationError, setNotificationError] = useState<
    string | undefined
  >(undefined);
  const [notificationBusy, setNotificationBusy] = useState(false);
  const [installError, setInstallError] = useState<string | undefined>(
    undefined,
  );
  const [binding, setBinding] = useState<DeviceBinding | undefined>(undefined);
  const [bindingCopied, setBindingCopied] = useState(false);
  const [bindingBusy, setBindingBusy] = useState(false);
  const [deviceActionError, setDeviceActionError] = useState<
    string | undefined
  >(undefined);
  const [revokingDevice, setRevokingDevice] = useState<string | undefined>(
    undefined,
  );
  const [webSessions, setWebSessions] = useState<WebSessionView[]>();
  const [accountAction, setAccountAction] = useState<string>();
  const [accountMessage, setAccountMessage] = useState<string>();
  const [accountError, setAccountError] = useState<string>();
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [auditScope, setAuditScope] = useState<AuditScope>("account");
  const [auditPage, setAuditPage] = useState<AuditPage>();
  const [auditError, setAuditError] = useState<string>();
  const [auditLoading, setAuditLoading] = useState(false);
  const [auditRevision, setAuditRevision] = useState(0);

  const sections = SETTINGS_SECTIONS.filter(
    (section) => !section.requiresAccount || account !== undefined,
  );
  const activeEntry =
    sections.find((section) => section.key === activeSection) ??
    sections.find((section) => section.key === "connection")!;

  useEffect(() => {
    return initThemeListener((mode) => {
      setCurrentTheme(mode);
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    void fetchDevices(config)
      .then((next) => {
        if (!cancelled) setDevices(next);
      })
      .catch((cause) => {
        if (!cancelled) setDeviceActionError(describeApiError(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [config, activeSection]);

  const knownDeviceIdsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (devices) {
      for (const d of devices) {
        knownDeviceIdsRef.current.add(d.device_id);
      }
    }
  }, [devices]);

  useEffect(() => {
    if (!binding) return;
    const interval = setInterval(() => {
      void fetchDevices(config)
        .then((next) => {
          setDevices(next);
          const hasNew = next.some(
            (d) => !knownDeviceIdsRef.current.has(d.device_id),
          );
          if (hasNew) {
            setBinding(undefined);
          }
        })
        .catch(() => {});
    }, 2000);
    return () => clearInterval(interval);
  }, [binding, config]);

  useEffect(() => {
    let cancelled = false;
    void readNotificationState(config)
      .catch((): NotificationState => "unavailable")
      .then((state) => {
        if (!cancelled) {
          setNotifications(state);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [config]);

  useEffect(() => {
    if (account === undefined) return;
    let cancelled = false;
    void fetchWebSessions()
      .then((sessions) => {
        if (!cancelled) setWebSessions(sessions);
      })
      .catch((cause) => {
        if (!cancelled) setAccountError(accountErrorMessage(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [account]);

  useEffect(() => {
    if (account === undefined) return;
    let cancelled = false;
    setAuditLoading(true);
    setAuditError(undefined);
    void fetchAuditEvents(auditScope)
      .then((page) => {
        if (!cancelled) setAuditPage(page);
      })
      .catch((cause) => {
        if (!cancelled) setAuditError(accountErrorMessage(cause));
      })
      .finally(() => {
        if (!cancelled) setAuditLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [account, auditRevision, auditScope]);

  function refreshAudit() {
    setAuditRevision((current) => current + 1);
  }

  async function loadMoreAudit() {
    const before = auditPage?.next_before;
    if (before === undefined || auditLoading) return;
    setAuditLoading(true);
    setAuditError(undefined);
    try {
      const next = await fetchAuditEvents(auditScope, before);
      setAuditPage((current) => ({
        ...next,
        events: [...(current?.events ?? []), ...next.events],
      }));
    } catch (cause) {
      setAuditError(accountErrorMessage(cause));
    } finally {
      setAuditLoading(false);
    }
  }

  async function resendVerification() {
    setAccountAction("verification");
    setAccountError(undefined);
    setAccountMessage(undefined);
    try {
      const result = await requestEmailVerification();
      setAccountMessage(
        result.verified
          ? "邮箱已经验证。"
          : result.accepted
            ? "验证邮件已发送。"
            : "验证请求没有产生新邮件。",
      );
      if (result.accepted) refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  async function revokeSession(session: WebSessionView) {
    setAccountAction(session.session_id);
    setAccountError(undefined);
    setAccountMessage(undefined);
    try {
      await revokeWebSession(session.session_id);
      if (session.current) {
        onLoggedOut();
        return;
      }
      setWebSessions((current) =>
        current?.filter((item) => item.session_id !== session.session_id),
      );
      setAccountMessage("登录会话已撤销。");
      refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  async function revokeOthers() {
    setAccountAction("others");
    setAccountError(undefined);
    setAccountMessage(undefined);
    try {
      const revoked = await revokeOtherWebSessions();
      setWebSessions((current) =>
        current?.filter((session) => session.current),
      );
      setAccountMessage(`已撤销 ${revoked} 个其他登录会话。`);
      if (revoked > 0) refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  async function submitPasswordChange() {
    setAccountError(undefined);
    setAccountMessage(undefined);
    if (newPassword !== confirmPassword) {
      setAccountError("两次输入的新密码不一致");
      return;
    }
    setAccountAction("password");
    try {
      const revoked = await changePassword({
        current_password: currentPassword,
        new_password: newPassword,
      });
      setCurrentPassword("");
      setNewPassword("");
      setConfirmPassword("");
      setWebSessions((current) =>
        current?.filter((session) => session.current),
      );
      setAccountMessage(`密码已更新，并撤销 ${revoked} 个其他登录会话。`);
      refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  async function toggleNotifications() {
    setNotificationBusy(true);
    setNotificationError(undefined);
    try {
      setNotifications(
        notifications === "on"
          ? await disableNotifications(config)
          : await enableNotifications(config),
      );
    } catch (cause) {
      setNotificationError(
        cause instanceof ApiError
          ? describeApiError(cause)
          : cause instanceof Error
            ? cause.message
            : "通知设置失败",
      );
    } finally {
      setNotificationBusy(false);
    }
  }

  async function installPwa() {
    setInstallError(undefined);
    try {
      await onInstall();
    } catch {
      setInstallError("浏览器未能启动安装流程，请稍后重试。");
    }
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      onSave({
        gatewayBase: normalizeGatewayBase(gatewayBase),
        userId: normalizeUserId(userId),
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "配置无效");
    }
  }

  async function generateBindingCode() {
    setBindingBusy(true);
    setDeviceActionError(undefined);
    setBindingCopied(false);
    try {
      setBinding(await createDeviceBinding(config));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setBindingBusy(false);
    }
  }

  async function copyBindingCode() {
    if (binding === undefined) {
      return;
    }
    setDeviceActionError(undefined);
    try {
      await navigator.clipboard.writeText(binding.binding_code);
      setBindingCopied(true);
    } catch {
      setDeviceActionError("无法自动复制，请点按绑定码后手动复制。");
    }
  }

  async function revoke(device: DeviceRecord) {
    if (
      device.revoked ||
      !window.confirm(
        `撤销设备 ${deviceShortId(device.device_id)}？现有历史会保留，但该设备将无法继续同步。`,
      )
    ) {
      return;
    }
    setRevokingDevice(device.device_id);
    setDeviceActionError(undefined);
    try {
      await revokeDevice(config, device.device_id);
      setDevices(await fetchDevices(config));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setRevokingDevice(undefined);
    }
  }

  return (
    <div className="settings-page">
      <header className="settings-page-header">
        <div>
          <h1>设置</h1>
          <p>管理账户安全、连接、设备与等待提醒。</p>
        </div>
        <button
          type="button"
          className="icon-button"
          aria-label="返回控制台"
          onClick={onClose}
        >
          <ArrowLeftIcon size={14} />
          <span>返回控制台</span>
        </button>
      </header>

      <div className="settings-page-body">
        <nav className="settings-page-nav" aria-label="设置分区">
          {sections.map((section) => {
            const SectionIcon = section.icon;
            return (
              <button
                key={section.key}
                type="button"
                data-active={section.key === activeEntry.key}
                aria-current={
                  section.key === activeEntry.key ? "page" : undefined
                }
                onClick={() => setActiveSection(section.key)}
              >
                <span className="settings-section-icon" aria-hidden="true">
                  <SectionIcon size={16} />
                </span>
                <span>{section.label}</span>
              </button>
            );
          })}
        </nav>

        <form
          key={activeEntry.key}
          className="settings-page-content settings-section-fade"
          aria-label="设置内容"
          onSubmit={handleSubmit}
        >
          {activeEntry.key === "appearance" && (
            <section
              className="settings-section"
              aria-labelledby="appearance-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <PaletteIcon size={18} />
                </span>
                <div>
                  <h3 id="appearance-settings">外观与主题</h3>
                  <p>采用 Claude Design System 人文配色与优雅排版系统。</p>
                </div>
              </div>

              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))",
                  gap: "12px",
                  margin: "18px 0 10px",
                }}
              >
                {(
                  [
                    {
                      mode: "system",
                      title: "跟随系统",
                      desc: "自动同步操作系统与浏览器的明暗设置",
                      badge: "Auto",
                      icon: <MonitorIcon size={16} />,
                    },
                    {
                      mode: "light",
                      title: "温润纸张",
                      desc: "天然柔和米白（Warm Paper），极度舒适的阅读质感",
                      badge: "Warm Paper",
                      icon: <SunIcon size={16} />,
                    },
                    {
                      mode: "dark",
                      title: "黑曜石暖炭",
                      desc: "黑曜石温暖暗色（Obsidian Charcoal），沉浸且不刺眼",
                      badge: "Obsidian",
                      icon: <MoonIcon size={16} />,
                    },
                  ] as const
                ).map((item) => {
                  const isSelected = currentTheme === item.mode;
                  return (
                    <button
                      key={item.mode}
                      type="button"
                      className="settings-theme-card"
                      data-selected={isSelected}
                      onClick={() => {
                        setTheme(item.mode);
                        setCurrentTheme(item.mode);
                      }}
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        alignItems: "flex-start",
                        gap: "8px",
                        padding: "16px",
                        textAlign: "left",
                        background: isSelected
                          ? "var(--remote-selected)"
                          : "var(--remote-surface)",
                        border: isSelected
                          ? "2px solid var(--remote-terracotta)"
                          : "1px solid var(--remote-line)",
                        borderRadius: "var(--radius-md, 12px)",
                        cursor: "pointer",
                        boxShadow: isSelected
                          ? "0 2px 8px rgba(217, 119, 87, 0.15)"
                          : "none",
                        transition: "all 0.16s cubic-bezier(0.16, 1, 0.3, 1)",
                      }}
                    >
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "space-between",
                          width: "100%",
                        }}
                      >
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: "8px",
                          }}
                        >
                          <span
                            style={{
                              color: isSelected
                                ? "var(--remote-terracotta)"
                                : "var(--remote-secondary)",
                              display: "inline-flex",
                              alignItems: "center",
                            }}
                          >
                            {item.icon}
                          </span>
                          <strong
                            style={{
                              fontSize: "14px",
                              color: "var(--remote-ink)",
                              fontWeight: 650,
                            }}
                          >
                            {item.title}
                          </strong>
                        </div>
                        <span
                          style={{
                            fontSize: "10px",
                            fontFamily: "var(--font-mono)",
                            padding: "2px 6px",
                            borderRadius: "4px",
                            background: isSelected
                              ? "var(--remote-terracotta-soft)"
                              : "var(--remote-hover)",
                            color: isSelected
                              ? "var(--remote-terracotta)"
                              : "var(--remote-muted)",
                            fontWeight: 600,
                          }}
                        >
                          {item.badge}
                        </span>
                      </div>
                      <p
                        style={{
                          margin: 0,
                          fontSize: "12px",
                          lineHeight: "1.5",
                          color: "var(--remote-secondary)",
                        }}
                      >
                        {item.desc}
                      </p>
                    </button>
                  );
                })}
              </div>

              <p className="settings-hint">
                所有主题均已完成 Anti-FOUC
                防跳闪优化，切换后即刻生效并持久保存。
              </p>
            </section>
          )}

          {activeEntry.key === "account" && account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="account-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <ShieldIcon size={18} />
                </span>
                <div>
                  <h3 id="account-settings">账户安全</h3>
                  <p>检查邮箱状态并管理所有网页登录会话。</p>
                </div>
              </div>
              <div className="account-security-status">
                <span
                  className="account-verification-dot"
                  data-verified={account.user.email_verified_at !== undefined}
                />
                <span>
                  <strong>{account.user.email}</strong>
                  <small>
                    {account.user.email_verified_at === undefined
                      ? "邮箱尚未验证"
                      : `已验证 · ${formatClock(account.user.email_verified_at)}`}
                  </small>
                </span>
                {account.user.email_verified_at === undefined && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={accountAction === "verification"}
                    onClick={() => void resendVerification()}
                  >
                    {accountAction === "verification"
                      ? "发送中…"
                      : "发送验证邮件"}
                  </button>
                )}
              </div>
              <div className="account-password-form">
                <span className="settings-subheading">更改密码</span>
                <div className="account-password-fields">
                  <label>
                    当前密码
                    <input
                      autoComplete="current-password"
                      minLength={10}
                      onChange={(event) =>
                        setCurrentPassword(event.target.value)
                      }
                      type="password"
                      value={currentPassword}
                    />
                  </label>
                  <label>
                    新密码
                    <input
                      autoComplete="new-password"
                      minLength={10}
                      onChange={(event) => setNewPassword(event.target.value)}
                      type="password"
                      value={newPassword}
                    />
                  </label>
                  <label>
                    确认新密码
                    <input
                      autoComplete="new-password"
                      minLength={10}
                      onChange={(event) =>
                        setConfirmPassword(event.target.value)
                      }
                      type="password"
                      value={confirmPassword}
                    />
                  </label>
                </div>
                <button
                  type="button"
                  className="secondary"
                  disabled={
                    accountAction !== undefined ||
                    currentPassword.length < 10 ||
                    newPassword.length < 10 ||
                    confirmPassword.length < 10
                  }
                  onClick={() => void submitPasswordChange()}
                >
                  {accountAction === "password" ? "更新中…" : "更新密码"}
                </button>
              </div>
              <div className="settings-toggle account-session-heading">
                <span>登录会话</span>
                <button
                  type="button"
                  className="secondary danger-button"
                  disabled={
                    accountAction !== undefined ||
                    !webSessions?.some((session) => !session.current)
                  }
                  onClick={() => void revokeOthers()}
                >
                  {accountAction === "others" ? "撤销中…" : "撤销其他会话"}
                </button>
              </div>
              <div className="account-session-list">
                {webSessions === undefined ? (
                  <p className="settings-hint">正在读取登录会话…</p>
                ) : (
                  webSessions.map((session) => (
                    <div
                      className="account-session-row"
                      key={session.session_id}
                    >
                      <span>
                        <strong>
                          {session.current ? "当前会话" : "网页登录"}
                        </strong>
                        <small>
                          最近活动 {formatClock(session.last_seen_at)} · 到期{" "}
                          {formatClock(session.expires_at)}
                        </small>
                      </span>
                      <button
                        type="button"
                        className="secondary danger-button"
                        disabled={accountAction === session.session_id}
                        onClick={() => void revokeSession(session)}
                      >
                        {accountAction === session.session_id
                          ? "撤销中…"
                          : "撤销"}
                      </button>
                    </div>
                  ))
                )}
              </div>
              {accountMessage !== undefined && (
                <p className="settings-receipt" role="status">
                  {accountMessage}
                </p>
              )}
              {accountError !== undefined && (
                <p className="error-text" role="alert">
                  {accountError}
                </p>
              )}
            </section>
          )}
          {activeEntry.key === "audit" && account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="audit-settings"
            >
              <div className="settings-section-heading audit-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <AuditIcon size={18} />
                </span>
                <div>
                  <h3 id="audit-settings">审计记录</h3>
                  <p>追踪账户安全、订阅和当前工作区的管理动作。</p>
                </div>
                <div className="audit-scope-switch" aria-label="审计范围">
                  {(["account", "workspace"] as const).map((scope) => (
                    <button
                      aria-pressed={auditScope === scope}
                      key={scope}
                      onClick={() => {
                        setAuditPage(undefined);
                        setAuditScope(scope);
                      }}
                      type="button"
                    >
                      {scope === "account" ? "账户" : "当前工作区"}
                    </button>
                  ))}
                </div>
              </div>
              {auditPage === undefined && auditLoading ? (
                <p className="settings-hint">正在读取审计记录…</p>
              ) : auditPage?.events.length === 0 ? (
                <p className="audit-empty">当前范围还没有审计记录。</p>
              ) : (
                <div className="audit-event-list">
                  {auditPage?.events.map((event) => {
                    const detail = auditDetail(event);
                    return (
                      <div className="audit-event-row" key={event.event_id}>
                        <span
                          className="audit-event-marker"
                          data-scope={event.scope}
                          aria-hidden="true"
                        />
                        <span className="audit-event-copy">
                          <strong>{AUDIT_ACTION_LABELS[event.action]}</strong>
                          {detail.length > 0 && <small>{detail}</small>}
                        </span>
                        <time dateTime={event.created_at}>
                          {formatClock(event.created_at)}
                        </time>
                      </div>
                    );
                  })}
                </div>
              )}
              {auditPage?.next_before !== undefined && (
                <button
                  className="secondary audit-load-more"
                  disabled={auditLoading}
                  onClick={() => void loadMoreAudit()}
                  type="button"
                >
                  {auditLoading ? "读取中…" : "加载更多"}
                </button>
              )}
              {auditError !== undefined && (
                <p className="error-text" role="alert">
                  {auditError}
                </p>
              )}
            </section>
          )}
          {activeEntry.key === "connection" && (
            <section
              className="settings-section"
              aria-labelledby="connection-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <ConnectionIcon size={18} />
                </span>
                <div>
                  <h3 id="connection-settings">连接</h3>
                  <p>配置面板读取数据时使用的网关。</p>
                </div>
              </div>
              <label>
                网关地址
                <input
                  value={gatewayBase}
                  onChange={(event) => setGatewayBase(event.target.value)}
                  placeholder="留空使用当前源（开发代理）"
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <p className="settings-hint">
                留空时请求经当前源的 /v1 代理转发；非回环直连地址必须使用
                https。
              </p>
              <label>
                用户标识（x-user-id）
                <input
                  value={userId}
                  onChange={(event) => setUserId(event.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <p className="settings-hint">
                仅供显式开启的本地开发模式使用；部署访问令牌只保留在 Web
                服务端。
              </p>
              <div className="settings-actions">
                <button type="submit">保存</button>
              </div>
            </section>
          )}

          {activeEntry.key === "devices" && (
            <section
              className="settings-section"
              aria-labelledby="device-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <DevicesIcon size={18} />
                </span>
                <div>
                  <h3 id="device-settings">设备</h3>
                  <p>绑定新的 Runner，或撤销不再使用的设备。</p>
                </div>
              </div>
              <div className="settings-toggle">
                <span>绑定新设备</span>
                <button
                  type="button"
                  className="secondary"
                  disabled={bindingBusy}
                  onClick={() => void generateBindingCode()}
                >
                  {bindingBusy ? "生成中…" : "生成绑定码"}
                </button>
              </div>
              {binding !== undefined && (
                <div className="binding-code">
                  <input
                    className="mono"
                    value={binding.binding_code}
                    readOnly
                    aria-label="一次性绑定码"
                    onFocus={(event) => event.currentTarget.select()}
                  />
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => void copyBindingCode()}
                  >
                    {bindingCopied ? "已复制" : "复制"}
                  </button>
                  <p className="settings-hint">
                    10 分钟内有效，仅显示在本次设置窗口。
                  </p>
                </div>
              )}
              {devices === undefined && (
                <p className="settings-hint">正在读取已绑定设备…</p>
              )}
              {devices !== undefined && devices.length === 0 && (
                <p className="settings-hint">
                  暂无已绑定设备。请点击上方按钮生成绑定码，并在客户端中完成配对。
                </p>
              )}
              {devices !== undefined && devices.length > 0 && (
                <div className="device-management">
                  <span className="settings-subheading">已绑定设备</span>
                  {devices.map((device) => (
                    <div
                      className="device-management-row"
                      key={device.device_id}
                    >
                      <span className="device-management-copy">
                        <span
                          className="device-dot"
                          data-online={device.online && !device.revoked}
                        />
                        <span>
                          <span className="mono" title={device.device_id}>
                            {deviceShortId(device.device_id)}
                          </span>
                          <small>
                            {device.revoked
                              ? "已撤销"
                              : device.online
                                ? "在线"
                                : "离线"}
                          </small>
                        </span>
                      </span>
                      <button
                        type="button"
                        className="secondary danger-button"
                        disabled={
                          device.revoked || revokingDevice === device.device_id
                        }
                        onClick={() => void revoke(device)}
                      >
                        {device.revoked
                          ? "已撤销"
                          : revokingDevice === device.device_id
                            ? "撤销中…"
                            : "撤销"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {deviceActionError !== undefined && (
                <p className="error-text" role="alert">
                  {deviceActionError}
                </p>
              )}
            </section>
          )}

          {activeEntry.key === "notifications" && (
            <section
              className="settings-section"
              aria-labelledby="notification-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <NotificationIcon size={18} />
                </span>
                <div>
                  <h3 id="notification-settings">等待提醒</h3>
                  <p>面板关闭后仍可接收等待输入或权限通知。</p>
                </div>
              </div>
              <div className="settings-toggle">
                <span>推送通知</span>
                <button
                  type="button"
                  className="secondary"
                  disabled={
                    notifications === undefined ||
                    notificationBusy ||
                    notifications === "unavailable" ||
                    notifications === "disabled" ||
                    notifications === "denied"
                  }
                  aria-pressed={notifications === "on"}
                  onClick={() => void toggleNotifications()}
                >
                  {notifications === undefined
                    ? "检查中…"
                    : notificationBusy
                      ? "处理中…"
                      : notifications === "on"
                        ? "已开启"
                        : "开启"}
                </button>
              </div>
              <p className="settings-hint">
                {notifications === undefined
                  ? "正在检查通知能力…"
                  : NOTIFICATION_HINTS[notifications]}
              </p>
              {notifications === "on" &&
                notificationPresentedAt !== undefined && (
                  <p className="settings-receipt" role="status">
                    最近展示请求：{" "}
                    {formatClock(
                      new Date(notificationPresentedAt).toISOString(),
                    )}
                    。Windows 是否弹出仍受系统设置影响。
                  </p>
                )}
              {notificationError !== undefined && (
                <p className="error-text" role="alert">
                  {notificationError}
                </p>
              )}
            </section>
          )}

          {activeEntry.key === "install" && (
            <section
              className="settings-section"
              aria-labelledby="install-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  <InstallIcon size={18} />
                </span>
                <div>
                  <h3 id="install-settings">安装到设备</h3>
                  <p>以独立 PWA 窗口打开 AgentHub。</p>
                </div>
              </div>
              <div className="settings-toggle">
                <span>应用安装</span>
                <button
                  type="button"
                  className="secondary"
                  disabled={installState !== "available"}
                  onClick={() => void installPwa()}
                >
                  {INSTALL_LABELS[installState]}
                </button>
              </div>
              <p className="settings-hint">{INSTALL_HINTS[installState]}</p>
              {installError !== undefined && (
                <p className="error-text" role="alert">
                  {installError}
                </p>
              )}
            </section>
          )}

          {error !== undefined && (
            <p className="error-text settings-form-error" role="alert">
              {error}
            </p>
          )}
        </form>
      </div>
    </div>
  );
}
