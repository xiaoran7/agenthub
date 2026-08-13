"use client";

import { useState, type FormEvent } from "react";

import {
  accountErrorMessage,
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
      <section className="account-signal" aria-label="AgentPulse 状态信号">
        <div className="account-brand">
          <span className="account-brand-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <strong>AgentPulse</strong>
        </div>
        <div className="account-pulse" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
          <span />
          <span />
        </div>
        <div className="account-signal-copy">
          <span>CONTROL PLANE / 01</span>
          <h1>你的 Agent，保持在线。</h1>
          <p>统一查看 Claude Code 与 Codex 会话，并管理已连接的执行设备。</p>
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
                  ? "创建 AgentPulse 账户"
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
