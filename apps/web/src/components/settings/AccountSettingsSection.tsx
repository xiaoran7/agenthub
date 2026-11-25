import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
} from "react";
import {
  accountErrorMessage,
  changePassword,
  fetchWebSessions,
  requestEmailVerification,
  revokeOtherWebSessions,
  revokeWebSession,
  updateAvatar,
  type AccountView,
  type WebSessionView,
} from "../../lib/account";
import { AvatarError, fileToAvatarDataUrl } from "../../lib/avatar";
import { formatClock } from "../../lib/format";
import { ShieldIcon } from "../settings-constants";

export interface AccountSettingsSectionProps {
  account: AccountView;
  onAccountUpdated?: ((account: AccountView) => void) | undefined;
  onLoggedOut: () => void;
  onAuditMutated: () => void;
}

export function AccountSettingsSection({
  account,
  onAccountUpdated,
  onLoggedOut,
  onAuditMutated,
}: AccountSettingsSectionProps) {
  const [webSessions, setWebSessions] = useState<WebSessionView[]>();
  const [accountAction, setAccountAction] = useState<string>();
  const [accountMessage, setAccountMessage] = useState<string>();
  const [accountError, setAccountError] = useState<string>();
  const [currentPassword, setCurrentPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [avatarPreview, setAvatarPreview] = useState<string | undefined>(
    account.user.avatar_data_url,
  );
  const avatarInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setAvatarPreview(account.user.avatar_data_url);
  }, [account.user.avatar_data_url]);

  useEffect(() => {
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

  async function pickAvatar(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
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
      const nextAvatar = stored ?? undefined;
      setAvatarPreview(nextAvatar);
      const { avatar_data_url: _prevAvatar, ...restUser } = account.user;
      onAccountUpdated?.({
        ...account,
        user: {
          ...restUser,
          ...(nextAvatar !== undefined ? { avatar_data_url: nextAvatar } : {}),
        },
      });
      setAccountMessage("头像已更新。");
      onAuditMutated();
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
      const { avatar_data_url: _removedAvatar, ...restUser } = account.user;
      onAccountUpdated?.({
        ...account,
        user: restUser,
      });
      setAccountMessage("头像已移除。");
      onAuditMutated();
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
      if (result.accepted) onAuditMutated();
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
      onAuditMutated();
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
      if (revoked > 0) onAuditMutated();
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
      onAuditMutated();
    } catch (cause) {
      setAccountError(accountErrorMessage(cause));
    } finally {
      setAccountAction(undefined);
    }
  }

  return (
    <section className="settings-section" aria-labelledby="account-settings">
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
              <img src={avatarPreview} alt="" referrerPolicy="no-referrer" />
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
            {accountAction === "verification" ? "发送中…" : "发送验证邮件"}
          </button>
        )}
      </div>
      <form
        className="account-password-form"
        onSubmit={(event: FormEvent<HTMLFormElement>) => {
          event.preventDefault();
          if (
            accountAction !== undefined ||
            currentPassword.length < 10 ||
            newPassword.length < 10 ||
            confirmPassword.length < 10
          ) {
            return;
          }
          void submitPasswordChange();
        }}
      >
        <span className="settings-subheading">更改密码</span>
        <div className="account-password-fields">
          <label>
            当前密码
            <input
              autoComplete="current-password"
              minLength={10}
              onChange={(event) => setCurrentPassword(event.target.value)}
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
              onChange={(event) => setConfirmPassword(event.target.value)}
              type="password"
              value={confirmPassword}
            />
          </label>
        </div>
        <button
          type="submit"
          className="secondary"
          disabled={
            accountAction !== undefined ||
            currentPassword.length < 10 ||
            newPassword.length < 10 ||
            confirmPassword.length < 10
          }
        >
          {accountAction === "password" ? "更新中…" : "更新密码"}
        </button>
      </form>
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
            <div className="account-session-row" key={session.session_id}>
              <span>
                <strong>{session.current ? "当前会话" : "网页登录"}</strong>
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
                {accountAction === session.session_id ? "撤销中…" : "撤销"}
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
  );
}
