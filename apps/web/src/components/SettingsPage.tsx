import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
} from "react";

import {
  ApiError,
  createDeviceBinding,
  describeApiError,
  fetchDevices,
  revokeDevice,
  type DeviceBinding,
  type GatewayIdentity,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import type { NotificationState } from "../lib/notifications";
import { deviceShortId, formatClock } from "../lib/format";
import { usePwaInstall } from "../lib/pwa-install";
import type { DeviceRecord } from "../lib/types";
import { PluginSettingsSection } from "./PluginSettingsSection";
import {
  accountErrorMessage,
  changePassword,
  fetchAuditEvents,
  fetchWebSessions,
  requestEmailVerification,
  revokeOtherWebSessions,
  revokeWebSession,
  updateAvatar,
  type AccountView,
  type AuditPage,
  type AuditScope,
  type WebSessionView,
} from "../lib/account";
import { AvatarError, fileToAvatarDataUrl } from "../lib/avatar";
import { useDialogFocus } from "../lib/dialog-focus";
import { Confirm } from "./Confirm";
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
import {
  ArrowLeftIcon,
  AUDIT_ACTION_LABELS,
  auditDetail,
  AuditIcon,
  ConnectionIcon,
  DevicesIcon,
  INSTALL_HINTS,
  INSTALL_LABELS,
  InstallIcon,
  MonitorIcon,
  MoonIcon,
  NOTIFICATION_HINTS,
  NotificationIcon,
  PaletteIcon,
  PluginsIcon,
  SETTINGS_SECTIONS,
  ShieldIcon,
  SunIcon,
  type SettingsSectionKey,
} from "./settings-constants";

interface SettingsPageProps {
  account: AccountView | undefined;
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  onWorkspaceChanged: (account: AccountView) => void;
  notificationPresentedAt: number | undefined;
  onLoggedOut: () => void;
  onClose: () => void;
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
  // The signed-in account is the real identity; the panel config's id is only
  // the fallback for a Gateway running without accounts. Reading another
  // owner's devices is impossible when the request carries the account.
  const identity = useMemo<GatewayIdentity>(
    () => ({ userId: account?.user.user_id ?? config.userId }),
    [account?.user.user_id, config.userId],
  );
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
  const [pendingRevoke, setPendingRevoke] = useState<DeviceRecord | undefined>(
    undefined,
  );
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
  const [avatarPreview, setAvatarPreview] = useState<string | undefined>(
    account?.user.avatar_data_url,
  );
  // The preview is a local copy so an upload can show the new picture right
  // away, but it has to follow the account when that changes — otherwise
  // signing in as somebody else would still show the previous user's picture.
  useEffect(() => {
    setAvatarPreview(account?.user.avatar_data_url);
  }, [account?.user.avatar_data_url]);
  const avatarInputRef = useRef<HTMLInputElement>(null);

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
    if (activeEntry.key !== "devices") return;
    let cancelled = false;
    void fetchDevices(config, identity)
      .then((next) => {
        if (!cancelled) setDevices(next);
      })
      .catch((cause) => {
        if (!cancelled) setDeviceActionError(describeApiError(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [config, identity, activeEntry.key]);

  const knownDeviceIdsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (devices) {
      for (const d of devices) {
        knownDeviceIdsRef.current.add(d.device_id);
      }
    }
  }, [devices]);

  useEffect(() => {
    const shouldPoll = activeSection === "devices" || binding !== undefined;
    if (!shouldPoll) return;
    const intervalMs = binding !== undefined ? 2000 : 5000;
    const interval = setInterval(() => {
      void fetchDevices(config, identity)
        .then((next) => {
          setDevices(next);
          if (
            binding !== undefined &&
            next.some((d) => !knownDeviceIdsRef.current.has(d.device_id))
          ) {
            setBinding(undefined);
          }
        })
        .catch(() => {});
    }, intervalMs);
    return () => clearInterval(interval);
  }, [activeSection, binding, config, identity]);

  useEffect(() => {
    if (activeEntry.key !== "notifications") return;
    let cancelled = false;
    void readNotificationState(config, identity)
      .catch((): NotificationState => "unavailable")
      .then((state) => {
        if (!cancelled) {
          setNotifications(state);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [config, identity, activeEntry.key]);

  useEffect(() => {
    if (account === undefined || activeEntry.key !== "account") return;
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
  }, [account, activeEntry.key]);

  useEffect(() => {
    if (account === undefined || activeEntry.key !== "audit") return;
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
  }, [account, auditRevision, auditScope, activeEntry.key]);

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

  async function pickAvatar(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    // Clear the input so picking the same file twice fires a change again.
    event.target.value = "";
    if (file === undefined) return;
    setAccountError(undefined);
    setAccountMessage(undefined);
    let dataUrl: string;
    try {
      dataUrl = await fileToAvatarDataUrl(file);
    } catch (cause) {
      setAccountError(
        cause instanceof AvatarError ? cause.message : "这张图片无法读取",
      );
      return;
    }
    setAccountAction("avatar");
    try {
      const stored = await updateAvatar(dataUrl);
      setAvatarPreview(stored ?? undefined);
      setAccountMessage("头像已更新。");
      refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  async function clearAvatar() {
    setAccountError(undefined);
    setAccountMessage(undefined);
    setAccountAction("avatar");
    try {
      await updateAvatar(null);
      setAvatarPreview(undefined);
      setAccountMessage("头像已移除。");
      refreshAudit();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
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
          ? await disableNotifications(config, identity)
          : await enableNotifications(config, identity),
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
      setBinding(await createDeviceBinding(config, identity));
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

  function deviceFriendlyName(device: DeviceRecord): string {
    if (device.display_name && device.display_name.trim().length > 0) {
      return device.display_name.trim();
    }
    const shortTail = device.device_id.startsWith("dev_")
      ? device.device_id.slice(-6)
      : deviceShortId(device.device_id);
    return `电脑 · ${shortTail}`;
  }

  /**
   * A revoke is asked for on the console's own dialog, not by the operating
   * system: the question names the device and says what survives, which
   * `window.confirm` could only do as an unstyled wall of text.
   */
  function askRevoke(device: DeviceRecord) {
    if (device.revoked) return;
    setPendingRevoke(device);
  }

  async function confirmRevoke() {
    const device = pendingRevoke;
    if (device === undefined) return;
    setPendingRevoke(undefined);
    setRevokingDevice(device.device_id);
    setDeviceActionError(undefined);
    try {
      await revokeDevice(config, identity, device.device_id);
      setDevices(await fetchDevices(config, identity));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setRevokingDevice(undefined);
    }
  }

  const settingsDialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(true, settingsDialogRef);

  return (
    <div
      ref={settingsDialogRef}
      className="settings-page"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-dialog-title"
    >
      <header className="settings-page-header">
        <div>
          <h1 id="settings-dialog-title">设置</h1>
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
                  <p>
                    选择适合当前光线环境的界面配色，减轻长时间阅读与协作的视觉疲劳。
                  </p>
                </div>
              </div>

              <div className="settings-theme-grid">
                {(
                  [
                    {
                      mode: "system",
                      title: "跟随系统",
                      desc: "自动跟随系统与浏览器的日间、夜间设置",
                      badge: "自动",
                      icon: <MonitorIcon size={16} />,
                    },
                    {
                      mode: "light",
                      title: "温润纸张",
                      desc: "柔和温暖的浅色纸面质感，适合日间明亮环境阅读",
                      badge: "浅色",
                      icon: <SunIcon size={16} />,
                    },
                    {
                      mode: "dark",
                      title: "黑曜石暖炭",
                      desc: "低对比度的暖调深色背景，夜间专注使用更护眼",
                      badge: "深色",
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
                      aria-pressed={isSelected}
                      onClick={() => {
                        setTheme(item.mode);
                        setCurrentTheme(item.mode);
                      }}
                    >
                      <div className="settings-theme-card-top">
                        <div className="settings-theme-card-title">
                          <span className="settings-theme-card-icon">
                            {item.icon}
                          </span>
                          <strong>{item.title}</strong>
                        </div>
                        <span className="settings-theme-card-badge">
                          {item.badge}
                        </span>
                      </div>
                      <p className="settings-theme-card-desc">{item.desc}</p>
                    </button>
                  );
                })}
              </div>

              <p className="settings-hint">
                切换后立即生效，并自动保存在当前浏览器中。
              </p>
            </section>
          )}

          {activeEntry.key === "plugins" && (
            <PluginSettingsSection
              config={config}
              identity={identity}
              devices={devices}
              onEnsureDevices={() =>
                void fetchDevices(config, identity)
                  .then((next) => setDevices(next))
                  .catch(() => {})
              }
            />
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
              <div className="account-avatar-row">
                <span className="settings-subheading">头像</span>
                <div className="account-avatar-controls">
                  <span className="account-avatar-preview" aria-hidden="true">
                    {avatarPreview && avatarPreview.trim().length > 0 ? (
                      <img
                        src={avatarPreview}
                        alt=""
                        referrerPolicy="no-referrer"
                      />
                    ) : (
                      (
                        account.user.display_name?.[0] ||
                        account.user.email?.[0] ||
                        "U"
                      ).toUpperCase()
                    )}
                  </span>
                  <input
                    ref={avatarInputRef}
                    type="file"
                    id="accountAvatarInput"
                    accept="image/png,image/jpeg,image/webp,image/gif"
                    className="sr-only"
                    disabled={accountAction === "avatar"}
                    onChange={(event) => void pickAvatar(event)}
                  />
                  <label
                    className="secondary account-avatar-input-label"
                    htmlFor="accountAvatarInput"
                    aria-disabled={accountAction === "avatar"}
                  >
                    {accountAction === "avatar" ? "上传中…" : "上传图片"}
                  </label>
                  {avatarPreview !== undefined && (
                    <button
                      type="button"
                      className="secondary danger-button"
                      disabled={accountAction === "avatar"}
                      onClick={() => void clearAvatar()}
                    >
                      移除
                    </button>
                  )}
                </div>
                <p className="settings-hint">
                  支持 PNG、JPEG、WebP 或 GIF，会自动缩放为方形头像。
                </p>
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
                  <p>
                    {account !== undefined
                      ? "当前通过已登录的同源安全会话访问网关。"
                      : "配置面板读取数据时使用的网关。"}
                  </p>
                </div>
              </div>
              {account !== undefined ? (
                <p className="settings-hint">
                  当前处于加密会话 Cookie 模式，所有 API
                  与实时流请求均通过当前源 /v1、/v3
                  自动鉴权，无需手动修改网关地址。
                </p>
              ) : (
                <>
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
                </>
              )}
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
                          <span
                            className="mono"
                            title={`设备 ID: ${device.device_id}`}
                            style={{ fontWeight: 500 }}
                          >
                            {deviceFriendlyName(device)}
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
                        onClick={() => askRevoke(device)}
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
                  disabled={
                    installState !== "available" &&
                    installState !== "installing"
                  }
                  aria-disabled={installState !== "available"}
                  onClick={() => {
                    if (installState !== "available") return;
                    void installPwa();
                  }}
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
      <Confirm
        open={pendingRevoke !== undefined}
        title={
          pendingRevoke === undefined
            ? "撤销这台设备？"
            : `撤销 "${deviceFriendlyName(pendingRevoke)}"？`
        }
        description={
          "现有历史数据将完整保留。撤销后随时可在设备端使用新配对码重新绑定连接。"
        }
        confirmLabel="撤销设备"
        cancelLabel="保留"
        onConfirm={() => void confirmRevoke()}
        onCancel={() => setPendingRevoke(undefined)}
      />
    </div>
  );
}
