import { useEffect, useState, type FormEvent } from "react";

import {
  ApiError,
  createDeviceBinding,
  describeApiError,
  revokeDevice,
  type DeviceBinding,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import { useModalDialog } from "../lib/dialog-focus";
import type { NotificationState } from "../lib/notifications";
import { deviceShortId, formatClock } from "../lib/format";
import type { PwaInstallState } from "../lib/pwa-install";
import type { DeviceRecord } from "../lib/types";
import {
  accountErrorMessage,
  changePassword,
  createBillingCheckout,
  createTeamInvitation,
  currentAccount,
  fetchAuditEvents,
  fetchBillingStatus,
  fetchTeamWorkspace,
  fetchWebSessions,
  openBillingPortal,
  removeTeamMember,
  requestEmailVerification,
  revokeOtherWebSessions,
  revokeTeamInvitation,
  revokeWebSession,
  switchWorkspace,
  updateTeamMember,
  type AccountView,
  type AuditEvent,
  type AuditPage,
  type AuditScope,
  type BillingStatus,
  type InviteRole,
  type TeamWorkspaceView,
  type WebSessionView,
} from "../lib/account";
import {
  disableNotifications,
  enableNotifications,
  readNotificationState,
} from "../lib/notifications";

interface SettingsPanelProps {
  account: AccountView | undefined;
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  devices: DeviceRecord[];
  onDevicesChanged: () => void;
  onWorkspaceChanged: (account: AccountView) => void;
  notificationPresentedAt: number | undefined;
  installState: PwaInstallState;
  onInstall: () => Promise<void>;
  onLoggedOut: () => void;
  onClose: () => void;
}

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

export function SettingsPanel({
  account,
  config,
  devices,
  onSave,
  onDevicesChanged,
  onWorkspaceChanged,
  notificationPresentedAt,
  installState,
  onInstall,
  onLoggedOut,
  onClose,
}: SettingsPanelProps) {
  const [gatewayBase, setGatewayBase] = useState(config.gatewayBase);
  const [userId, setUserId] = useState(config.userId);
  const [error, setError] = useState<string | undefined>(undefined);
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
  const [billing, setBilling] = useState<BillingStatus>();
  const [billingAction, setBillingAction] = useState<string>();
  const [billingMessage, setBillingMessage] = useState<string>();
  const [billingError, setBillingError] = useState<string>();
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [team, setTeam] = useState<TeamWorkspaceView>();
  const [teamEmail, setTeamEmail] = useState("");
  const [teamRole, setTeamRole] = useState<InviteRole>("member");
  const [teamAction, setTeamAction] = useState<string>();
  const [teamMessage, setTeamMessage] = useState<string>();
  const [teamError, setTeamError] = useState<string>();
  const [invitationUrl, setInvitationUrl] = useState<string>();
  const [auditScope, setAuditScope] = useState<AuditScope>("account");
  const [auditPage, setAuditPage] = useState<AuditPage>();
  const [auditError, setAuditError] = useState<string>();
  const [auditLoading, setAuditLoading] = useState(false);
  const [auditRevision, setAuditRevision] = useState(0);
  const dialogRef = useModalDialog<HTMLFormElement>(onClose);

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
    if (account === undefined) {
      setBilling(undefined);
      return;
    }
    let cancelled = false;
    setBillingError(undefined);
    void fetchBillingStatus()
      .then((status) => {
        if (!cancelled) setBilling(status);
      })
      .catch((cause) => {
        if (!cancelled) setBillingError(accountErrorMessage(cause));
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

  useEffect(() => {
    if (team?.workspace.role === "admin" && teamRole === "admin") {
      setTeamRole("member");
    }
  }, [team, teamRole]);

  useEffect(() => {
    if (account === undefined) return;
    let cancelled = false;
    void fetchTeamWorkspace()
      .then((next) => {
        if (!cancelled) setTeam(next);
      })
      .catch((cause) => {
        if (!cancelled) setTeamError(accountErrorMessage(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [account]);

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

  async function startBillingCheckout(plan: "pro" | "team") {
    setBillingAction(plan);
    setBillingError(undefined);
    setBillingMessage(undefined);
    try {
      const url = await createBillingCheckout(plan);
      window.location.assign(url);
    } catch (cause) {
      setBillingError(accountErrorMessage(cause));
    } finally {
      setBillingAction(undefined);
    }
  }

  async function startBillingPortal() {
    setBillingAction("portal");
    setBillingError(undefined);
    setBillingMessage(undefined);
    try {
      const url = await openBillingPortal();
      window.location.assign(url);
    } catch (cause) {
      setBillingError(accountErrorMessage(cause));
    } finally {
      setBillingAction(undefined);
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

  async function selectWorkspace(workspaceOwnerId: string) {
    setTeamAction("workspace");
    setTeamError(undefined);
    setInvitationUrl(undefined);
    try {
      await switchWorkspace(workspaceOwnerId);
      const next = await currentAccount();
      if (next === undefined) throw new Error("account unavailable");
      onWorkspaceChanged(next);
      setTeam(await fetchTeamWorkspace());
      setTeamMessage("工作区已切换。");
      refreshAudit();
    } catch (cause) {
      setTeamError(accountErrorMessage(cause));
    } finally {
      setTeamAction(undefined);
    }
  }

  async function inviteMember() {
    setTeamAction("invite");
    setTeamError(undefined);
    setTeamMessage(undefined);
    setInvitationUrl(undefined);
    try {
      const invitation = await createTeamInvitation({
        email: teamEmail,
        role: teamRole,
      });
      setInvitationUrl(
        invitation.invitation_url.startsWith("#")
          ? `${window.location.origin}/${invitation.invitation_url.replace("token=", "team_invitation=")}`
          : invitation.invitation_url,
      );
      setTeamEmail("");
      setTeam(await fetchTeamWorkspace());
      setTeamMessage("邀请已创建；链接只在本次设置窗口显示。");
      refreshAudit();
    } catch (cause) {
      setTeamError(accountErrorMessage(cause));
    } finally {
      setTeamAction(undefined);
    }
  }

  async function copyInvitation() {
    if (invitationUrl === undefined) return;
    try {
      await navigator.clipboard.writeText(invitationUrl);
      setTeamMessage("邀请链接已复制。");
    } catch {
      setTeamError("无法自动复制，请手动选择邀请链接。");
    }
  }

  async function changeMemberRole(userId: string, role: InviteRole) {
    setTeamAction(userId);
    setTeamError(undefined);
    try {
      await updateTeamMember(userId, role);
      setTeam(await fetchTeamWorkspace());
      setTeamMessage("成员角色已更新。");
      refreshAudit();
    } catch (cause) {
      setTeamError(accountErrorMessage(cause));
    } finally {
      setTeamAction(undefined);
    }
  }

  async function removeMember(userId: string) {
    if (!window.confirm("将这个成员移出当前工作区？其会话会回到个人工作区。")) {
      return;
    }
    setTeamAction(userId);
    setTeamError(undefined);
    try {
      await removeTeamMember(userId);
      setTeam(await fetchTeamWorkspace());
      setTeamMessage("成员已移出工作区。");
      refreshAudit();
    } catch (cause) {
      setTeamError(accountErrorMessage(cause));
    } finally {
      setTeamAction(undefined);
    }
  }

  async function cancelInvitation(invitationId: string) {
    setTeamAction(invitationId);
    setTeamError(undefined);
    try {
      await revokeTeamInvitation(invitationId);
      setTeam(await fetchTeamWorkspace());
      setTeamMessage("邀请已撤销。");
      refreshAudit();
    } catch (cause) {
      setTeamError(accountErrorMessage(cause));
    } finally {
      setTeamAction(undefined);
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
      onDevicesChanged();
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setRevokingDevice(undefined);
    }
  }

  return (
    <div
      className="settings-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <form
        ref={dialogRef}
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        tabIndex={-1}
        onSubmit={handleSubmit}
      >
        <header className="settings-header">
          <div>
            <p className="eyebrow">面板设置</p>
            <h2 id="settings-title">面板设置</h2>
            <p>管理账户安全、连接、设备与等待提醒。</p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭设置"
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className="settings-scroll">
          {account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="account-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  ◎
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
          {account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="billing-settings"
            >
              <div className="settings-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  $
                </span>
                <div>
                  <h3 id="billing-settings">订阅方案</h3>
                  <p>按账户方案控制设备、历史和远程任务额度。</p>
                </div>
              </div>
              <div className="billing-plan-card">
                <div>
                  <strong>{account.subscription.plan.toUpperCase()}</strong>
                  <small>
                    {account.subscription.status}
                    {account.subscription.current_period_end === undefined
                      ? ""
                      : ` · 到期 ${formatClock(account.subscription.current_period_end)}`}
                  </small>
                </div>
                {billing?.portal === true && (
                  <button
                    className="secondary"
                    disabled={billingAction !== undefined}
                    onClick={() => void startBillingPortal()}
                    type="button"
                  >
                    {billingAction === "portal" ? "打开中…" : "管理订阅"}
                  </button>
                )}
              </div>
              {billing === undefined && billingError === undefined && (
                <p className="settings-hint">正在读取支付状态…</p>
              )}
              {billing?.available === false && (
                <p className="settings-hint">订阅支付尚未配置。</p>
              )}
              {billing?.checkout === true &&
                account.subscription.plan === "free" && (
                  <div className="billing-plan-actions">
                    <button
                      className="secondary"
                      disabled={billingAction !== undefined}
                      onClick={() => void startBillingCheckout("pro")}
                      type="button"
                    >
                      {billingAction === "pro" ? "打开中…" : "升级 Pro"}
                    </button>
                    <button
                      className="primary"
                      disabled={billingAction !== undefined}
                      onClick={() => void startBillingCheckout("team")}
                      type="button"
                    >
                      {billingAction === "team" ? "打开中…" : "升级 Team"}
                    </button>
                  </div>
                )}
              {billingError !== undefined && (
                <p className="error-text" role="alert">
                  {billingError}
                </p>
              )}
              {billingMessage !== undefined && (
                <p className="settings-receipt" role="status">
                  {billingMessage}
                </p>
              )}
            </section>
          )}
          {account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="team-settings"
            >
              <div className="settings-section-heading team-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  ◇
                </span>
                <div>
                  <h3 id="team-settings">团队工作区</h3>
                  <p>切换共享数据空间，并管理 Team 方案成员。</p>
                </div>
                {team !== undefined && (
                  <span className="team-seat-count">
                    {team.members.length + team.invitations.length}/
                    {team.member_limit}
                  </span>
                )}
              </div>
              {account.workspaces !== undefined && (
                <label className="team-workspace-select">
                  当前工作区
                  <select
                    disabled={teamAction === "workspace"}
                    onChange={(event) =>
                      void selectWorkspace(event.target.value)
                    }
                    value={
                      account.workspaces.find((workspace) => workspace.current)
                        ?.workspace_owner_id ?? account.user.user_id
                    }
                  >
                    {account.workspaces.map((workspace) => (
                      <option
                        key={workspace.workspace_owner_id}
                        value={workspace.workspace_owner_id}
                      >
                        {workspace.name} · {roleLabel(workspace.role)}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {team === undefined ? (
                <p className="settings-hint">正在读取团队成员…</p>
              ) : (
                <>
                  <div className="team-member-list">
                    {team.members.map((member) => {
                      const actorRole = team.workspace.role;
                      const manageable =
                        member.role !== "owner" &&
                        (actorRole === "owner" ||
                          (actorRole === "admin" && member.role === "member"));
                      return (
                        <div className="team-member-row" key={member.user_id}>
                          <span className="team-avatar" aria-hidden="true">
                            {member.display_name.slice(0, 1).toUpperCase()}
                          </span>
                          <span className="team-member-copy">
                            <strong>{member.display_name}</strong>
                            <small>{member.email}</small>
                          </span>
                          {manageable && actorRole === "owner" ? (
                            <select
                              aria-label={`${member.display_name} 的角色`}
                              disabled={teamAction === member.user_id}
                              onChange={(event) =>
                                void changeMemberRole(
                                  member.user_id,
                                  event.target.value as InviteRole,
                                )
                              }
                              value={member.role}
                            >
                              <option value="admin">管理员</option>
                              <option value="member">成员</option>
                            </select>
                          ) : (
                            <span className="team-role-badge">
                              {roleLabel(member.role)}
                            </span>
                          )}
                          {manageable && (
                            <button
                              className="secondary danger-button"
                              disabled={teamAction === member.user_id}
                              onClick={() => void removeMember(member.user_id)}
                              type="button"
                            >
                              移除
                            </button>
                          )}
                        </div>
                      );
                    })}
                  </div>
                  {(team.workspace.role === "owner" ||
                    team.workspace.role === "admin") &&
                    team.member_limit > 1 && (
                      <div className="team-invite-form">
                        <label>
                          邀请已注册用户
                          <input
                            autoComplete="email"
                            onChange={(event) =>
                              setTeamEmail(event.target.value)
                            }
                            placeholder="member@example.com"
                            type="email"
                            value={teamEmail}
                          />
                        </label>
                        <label>
                          角色
                          <select
                            onChange={(event) =>
                              setTeamRole(event.target.value as InviteRole)
                            }
                            value={teamRole}
                          >
                            <option value="member">成员</option>
                            {team.workspace.role === "owner" && (
                              <option value="admin">管理员</option>
                            )}
                          </select>
                        </label>
                        <button
                          className="secondary"
                          disabled={
                            teamAction !== undefined ||
                            teamEmail.trim().length < 3 ||
                            team.members.length + team.invitations.length >=
                              team.member_limit
                          }
                          onClick={() => void inviteMember()}
                          type="button"
                        >
                          {teamAction === "invite" ? "创建中…" : "创建邀请"}
                        </button>
                      </div>
                    )}
                  {invitationUrl !== undefined && (
                    <div className="team-invitation-link">
                      <input
                        aria-label="团队邀请链接"
                        onFocus={(event) => event.currentTarget.select()}
                        readOnly
                        value={invitationUrl}
                      />
                      <button
                        className="secondary"
                        onClick={() => void copyInvitation()}
                        type="button"
                      >
                        复制
                      </button>
                    </div>
                  )}
                  {team.invitations.length > 0 && (
                    <div className="team-pending-list">
                      <span className="settings-subheading">待接受邀请</span>
                      {team.invitations.map((invitation) => (
                        <div key={invitation.invitation_id}>
                          <span>
                            <strong>{invitation.email}</strong>
                            <small>{roleLabel(invitation.role)}</small>
                          </span>
                          <button
                            className="secondary danger-button"
                            disabled={teamAction === invitation.invitation_id}
                            onClick={() =>
                              void cancelInvitation(invitation.invitation_id)
                            }
                            type="button"
                          >
                            撤销
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              )}
              {teamMessage !== undefined && (
                <p className="settings-receipt" role="status">
                  {teamMessage}
                </p>
              )}
              {teamError !== undefined && (
                <p className="error-text" role="alert">
                  {teamError}
                </p>
              )}
            </section>
          )}
          {account !== undefined && (
            <section
              className="settings-section"
              aria-labelledby="audit-settings"
            >
              <div className="settings-section-heading audit-section-heading">
                <span className="settings-section-icon" aria-hidden="true">
                  ◌
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
          <section
            className="settings-section"
            aria-labelledby="connection-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ↗
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
              留空时请求经当前源的 /v1 代理转发；非回环直连地址必须使用 https。
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
              仅供显式开启的本地开发模式使用；部署访问令牌只保留在 Web 服务端。
            </p>
          </section>

          <section
            className="settings-section"
            aria-labelledby="device-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ◫
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
            {devices.length > 0 && (
              <div className="device-management">
                <span className="settings-subheading">已绑定设备</span>
                {devices.map((device) => (
                  <div className="device-management-row" key={device.device_id}>
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

          <section
            className="settings-section"
            aria-labelledby="notification-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ◉
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
                  {formatClock(new Date(notificationPresentedAt).toISOString())}
                  。Windows 是否弹出仍受系统设置影响。
                </p>
              )}
            {notificationError !== undefined && (
              <p className="error-text" role="alert">
                {notificationError}
              </p>
            )}
          </section>

          <section
            className="settings-section"
            aria-labelledby="install-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ⌂
              </span>
              <div>
                <h3 id="install-settings">安装到设备</h3>
                <p>以独立 PWA 窗口打开 AgentPulse。</p>
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

          {error !== undefined && (
            <p className="error-text settings-form-error" role="alert">
              {error}
            </p>
          )}
        </div>

        <footer className="settings-actions">
          <button type="button" className="secondary" onClick={onClose}>
            取消
          </button>
          <button type="submit">保存</button>
        </footer>
      </form>
    </div>
  );
}
