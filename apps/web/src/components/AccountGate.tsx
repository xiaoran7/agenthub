"use client";

import { useEffect, useState, type FormEvent } from "react";

import {
  accountErrorMessage,
  fetchAuthStatus,
  loginAccount,
  setupAdmin,
  type AccountView,
} from "../lib/account";
import "./account-desktop.css";

export function AccountGate({
  onAuthenticated,
}: {
  onAuthenticated: (account: AccountView) => void;
}) {
  const [needsSetup, setNeedsSetup] = useState<boolean | undefined>(undefined);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [displayName, setDisplayName] = useState("管理员");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void fetchAuthStatus()
      .then((res) => {
        if (!cancelled) setNeedsSetup(res.needs_setup);
      })
      .catch(() => {
        if (!cancelled) setNeedsSetup(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(undefined);

    if (needsSetup) {
      if (password.length < 10) {
        setError("密码长度至少需要 10 个字符");
        return;
      }
      if (password !== confirmPassword) {
        setError("两次输入的密码不一致");
        return;
      }
    }

    setBusy(true);
    try {
      let account: AccountView;
      if (needsSetup) {
        const payload: {
          email: string;
          password: string;
          display_name?: string;
        } = {
          email: email.trim(),
          password,
        };
        if (displayName.trim()) {
          payload.display_name = displayName.trim();
        }
        account = await setupAdmin(payload);
      } else {
        account = await loginAccount({ email: email.trim(), password });
      }
      onAuthenticated(account);
    } catch (nextError) {
      setError(accountErrorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  if (needsSetup === undefined) {
    return (
      <main className="account-entry">
        <section className="account-panel">
          <div
            className="account-panel-inner"
            style={{ textAlign: "center", padding: "48px 24px" }}
          >
            <p style={{ color: "var(--muted, #999)", fontSize: "14px" }}>
              正在检测服务器初始化状态…
            </p>
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="account-entry">
      <section className="account-panel">
        <div className="account-panel-inner">
          <div className="account-brand">
            <svg
              width="34"
              height="34"
              viewBox="0 0 48 48"
              fill="none"
              aria-hidden="true"
            >
              <rect
                x="3"
                y="3"
                width="42"
                height="42"
                rx="12"
                fill="currentColor"
                fillOpacity="0.08"
                stroke="currentColor"
                strokeOpacity="0.25"
                strokeWidth="2.5"
              />
              <path
                d="M12 25h6l3-9 6 16 3-10 3 3h3"
                stroke="currentColor"
                strokeWidth="3.2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              <circle cx="21" cy="16" r="2.2" fill="currentColor" />
              <circle cx="27" cy="32" r="2.2" fill="currentColor" />
            </svg>
            <span>AgentHub</span>
          </div>

          <header className="account-heading">
            <span>{needsSetup ? "系统初次安装" : "开源自部署"}</span>
            <h2>{needsSetup ? "初始化超级管理员" : "管理员登录"}</h2>
            <p>
              {needsSetup
                ? "检测到服务器首次启动，请设定全局唯一的管理员账号。设置后将永久锁定公开注册。"
                : "请输入服务器管理员凭据进入 AgentHub 控制台。"}
            </p>
          </header>

          <form className="account-form" onSubmit={submit}>
            {needsSetup && (
              <label>
                <span>显示名称</span>
                <input
                  autoComplete="name"
                  maxLength={80}
                  onChange={(event) => setDisplayName(event.target.value)}
                  placeholder="管理员显示名称"
                  required
                  value={displayName}
                />
              </label>
            )}

            <label>
              <span>管理员邮箱</span>
              <input
                autoComplete="email"
                inputMode="email"
                onChange={(event) => setEmail(event.target.value)}
                placeholder="admin@example.com"
                required
                type="email"
                value={email}
                autoFocus
              />
            </label>

            <label>
              <span>{needsSetup ? "设置密码（至少 10 位）" : "密码"}</span>
              <input
                autoComplete={needsSetup ? "new-password" : "current-password"}
                minLength={10}
                onChange={(event) => setPassword(event.target.value)}
                placeholder={needsSetup ? "至少 10 个字符" : "••••••••••"}
                required
                type="password"
                value={password}
              />
            </label>

            {needsSetup && (
              <label>
                <span>确认密码</span>
                <input
                  autoComplete="new-password"
                  minLength={10}
                  onChange={(event) => setConfirmPassword(event.target.value)}
                  placeholder="再次输入相同密码"
                  required
                  type="password"
                  value={confirmPassword}
                />
              </label>
            )}

            {error !== undefined && (
              <p className="account-error" role="alert">
                {error}
              </p>
            )}

            <button className="account-submit" disabled={busy} type="submit">
              {busy
                ? "正在验证…"
                : needsSetup
                  ? "完成初始化并进入控制台"
                  : "登录控制台"}
            </button>
          </form>

          <p className="account-policy">
            {needsSetup
              ? "自部署模式下管理员凭据加密储存在 SQLite 中。"
              : "登录会话仅保存在安全 HttpOnly Cookie 中。"}
          </p>
        </div>
      </section>
    </main>
  );
}
