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
      <section className="account-signal" aria-label="Agent Hub 状态信号">
        <div className="account-brand">
          <span
            className="account-brand-mark"
            aria-hidden="true"
            style={{
              display: "inline-flex",
              alignItems: "center",
              width: 32,
              height: 32,
            }}
          >
            <svg
              viewBox="0 0 512 512"
              width={32}
              height={32}
              style={{ filter: "drop-shadow(0 0 8px rgba(0, 229, 255, 0.7))" }}
            >
              <g
                stroke="#00e5ff"
                strokeWidth="20"
                strokeLinecap="round"
                strokeLinejoin="round"
                fill="none"
              >
                <line x1="256" y1="256" x2="256" y2="136" />
                <line x1="256" y1="256" x2="360" y2="196" />
                <line x1="256" y1="256" x2="360" y2="316" />
                <line x1="256" y1="256" x2="256" y2="376" />
                <line x1="256" y1="256" x2="152" y2="316" />
                <line x1="256" y1="256" x2="152" y2="196" />
                <polygon points="256,136 360,196 360,316 256,376 152,316 152,196" />
              </g>
              <g stroke="#00e5ff" strokeWidth="14" fill="#0b0f19">
                <circle cx="256" cy="136" r="22" />
                <circle cx="360" cy="196" r="22" />
                <circle cx="360" cy="316" r="22" />
                <circle cx="256" cy="376" r="22" />
                <circle cx="152" cy="316" r="22" />
                <circle cx="152" cy="196" r="22" />
              </g>
              <circle cx="256" cy="256" r="42" fill="#00e5ff" opacity="0.4" />
              <circle cx="256" cy="256" r="26" fill="#ffffff" />
            </svg>
          </span>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              lineHeight: 1.15,
            }}
          >
            <span
              style={{
                fontWeight: 800,
                fontSize: "18px",
                letterSpacing: "0.02em",
                color: "#f8fafc",
              }}
            >
              Agent{" "}
              <span
                style={{
                  background:
                    "linear-gradient(90deg, #00f5d4, #00bbf9, #a855f7)",
                  WebkitBackgroundClip: "text",
                  WebkitTextFillColor: "transparent",
                }}
              >
                Hub
              </span>
            </span>
            <span
              style={{
                fontSize: "11px",
                color: "#94a3b8",
                letterSpacing: "0.08em",
              }}
            >
              智能体集成中心
            </span>
          </div>
        </div>
        <div className="account-hero-visual" aria-hidden="true">
          <img
            src="/brand-banner.png"
            alt="Agent Hub Banner"
            style={{
              width: "100%",
              maxWidth: "520px",
              borderRadius: "14px",
              border: "1px solid rgba(0, 229, 255, 0.18)",
              boxShadow: "0 0 32px rgba(0, 229, 255, 0.12)",
              marginTop: "24px",
              marginBottom: "24px",
            }}
          />
        </div>
        <div className="account-signal-copy">
          <span>CONTROL PLANE / 01</span>
          <h1>你的 Agent，保持在线。</h1>
          <p>
            统一查看 Claude Code、Codex、Antigravity 与 Grok Build
            会话，并管理已连接的执行设备。
          </p>
        </div>
        <dl className="account-signal-meta">
          <div>
            <dt>通道</dt>
            <dd>加密会话</dd>
          </div>
          <div>
            <dt>凭据</dt>
            <dd>仅保留本机</dd>
          </div>
        </dl>
      </section>
      <section className="account-panel">
        <div className="account-panel-inner">
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
                disabled={busy}
                onClick={loginWithTgOAuth}
              >
                <span className="oauth-icon">◆</span>
                <span>使用 TannerLab / Tg-oauth 统一登录</span>
              </button>
              <div className="account-divider">
                <span>或使用邮箱密码</span>
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
