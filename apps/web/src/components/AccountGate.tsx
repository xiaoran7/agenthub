"use client";

import { useState, type FormEvent } from "react";

import {
  accountErrorMessage,
  getTgOAuthAuthorizeUrl,
  loginAccount,
  registerAccount,
  requestPasswordReset,
  type AccountView,
} from "../lib/account";
import "./account-desktop.css";

export function AccountGate({
  onAuthenticated,
}: {
  onAuthenticated: (account: AccountView) => void;
}) {
  const [mode, setMode] = useState<"login" | "register" | "reset">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string>();

  const enterSelfHost = () => {
    localStorage.setItem("agenthub_selfhost_direct", "true");
    onAuthenticated({
      user: {
        user_id: "selfhost-owner",
        email: "owner@self-hosted.local",
        display_name: "Self-Host Owner",
        status: "active",
        email_verified_at: new Date(0).toISOString(),
        created_at: new Date(0).toISOString(),
      },
      workspace: {
        workspace_owner_id: "selfhost-owner",
        name: "Default Workspace",
        role: "owner",
        current: true,
      },
      entitlements: {
        max_devices: 999,
        max_concurrent_tasks: 999,
        history_days: 3650,
        remote_tasks: true,
        team_members: 1,
      },
      subscription: {
        plan: "pro",
        status: "active",
      },
    });
  };

  const loginWithTgOAuth = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const redirectUri = `${window.location.origin}/auth/callback/tg`;
      const res = await getTgOAuthAuthorizeUrl({ redirect_uri: redirectUri });
      sessionStorage.setItem("tg_oauth_code_verifier", res.code_verifier);
      sessionStorage.setItem("tg_oauth_state", res.state);
      window.location.href = res.authorize_url;
    } catch (nextError) {
      setError(accountErrorMessage(nextError));
      setBusy(false);
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    setMessage(undefined);
    try {
      if (mode === "reset") {
        await requestPasswordReset(email);
        setMessage("如果这个邮箱已注册，重置邮件会在几分钟内送达。");
        return;
      }
      const account =
        mode === "login"
          ? await loginAccount({ email, password })
          : await registerAccount({
              email,
              password,
              display_name: displayName,
            });
      onAuthenticated(account);
    } catch (nextError) {
      setError(accountErrorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  const switchMode = (next: "login" | "register" | "reset") => {
    setMode(next);
    setError(undefined);
    setMessage(undefined);
  };

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
          {mode !== "reset" && (
            <div className="account-tabs" role="tablist" aria-label="账户操作">
              <button
                aria-selected={mode === "login"}
                onClick={() => switchMode("login")}
                role="tab"
                type="button"
              >
                登录
              </button>
              <button
                aria-selected={mode === "register"}
                onClick={() => switchMode("register")}
                role="tab"
                type="button"
              >
                注册
              </button>
            </div>
          )}
          <header className="account-heading">
            <span>
              {mode === "login"
                ? "欢迎回来"
                : mode === "register"
                  ? "建立工作区"
                  : "恢复访问"}
            </span>
            <h2>
              {mode === "login"
                ? "进入控制台"
                : mode === "register"
                  ? "创建 AgentHub 账户"
                  : "重置账户密码"}
            </h2>
            <p>
              {mode === "login"
                ? "登录后读取你的设备、会话与订阅权益。"
                : mode === "register"
                  ? "Free 方案包含 1 台设备和 7 天会话历史。"
                  : "我们会向已注册邮箱发送一小时内有效的重置链接。"}
            </p>
          </header>
          {mode !== "reset" && (
            <div className="account-oauth-box">
              <button
                type="button"
                className="account-oauth-submit"
                style={{ background: "#2563eb", borderColor: "#1d4ed8" }}
                disabled={busy}
                onClick={enterSelfHost}
              >
                <span className="oauth-icon">⚡</span>
                <span>自建服务端直连进入</span>
              </button>
              <div className="account-divider">
                <span>或使用账号密码</span>
              </div>
            </div>
          )}
          <form className="account-form" onSubmit={submit}>
            {mode === "register" && (
              <label>
                <span>显示名称</span>
                <input
                  autoComplete="name"
                  maxLength={80}
                  onChange={(event) => setDisplayName(event.target.value)}
                  placeholder="你的名称"
                  required
                  value={displayName}
                />
              </label>
            )}
            <label>
              <span>邮箱</span>
              <input
                autoComplete="email"
                inputMode="email"
                onChange={(event) => setEmail(event.target.value)}
                placeholder="name@example.com"
                required
                type="email"
                value={email}
              />
            </label>
            {mode !== "reset" && (
              <label>
                <span>密码</span>
                <input
                  autoComplete={
                    mode === "login" ? "current-password" : "new-password"
                  }
                  minLength={10}
                  onChange={(event) => setPassword(event.target.value)}
                  placeholder="至少 10 个字符"
                  required
                  type="password"
                  value={password}
                />
              </label>
            )}
            {mode === "login" && (
              <button
                className="account-text-action"
                onClick={() => switchMode("reset")}
                type="button"
              >
                忘记密码？
              </button>
            )}
            {message !== undefined && (
              <p className="account-success" role="status">
                {message}
              </p>
            )}
            {error !== undefined && (
              <p className="account-error" role="alert">
                {error}
              </p>
            )}
            <button className="account-submit" disabled={busy} type="submit">
              {busy
                ? mode === "reset"
                  ? "正在发送…"
                  : "正在验证…"
                : mode === "login"
                  ? "进入控制台"
                  : mode === "register"
                    ? "创建账户"
                    : "发送重置邮件"}
            </button>
            {mode === "reset" && (
              <button
                className="account-text-action account-return-action"
                onClick={() => switchMode("login")}
                type="button"
              >
                返回登录
              </button>
            )}
          </form>
          <p className="account-policy">登录会话仅保存在安全 Cookie 中。</p>
        </div>
      </section>
    </main>
  );
}
