"use client";

import { useEffect, useState, type FormEvent } from "react";

import { accountErrorMessage, confirmPasswordReset } from "../lib/account";

type ResetState = "ready" | "saving" | "changed" | "invalid";

export function ResetPasswordClient() {
  const [token, setToken] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [state, setState] = useState<ResetState>("ready");
  const [error, setError] = useState<string>();

  useEffect(() => {
    const nextToken =
      new URLSearchParams(window.location.hash.slice(1)).get("token") ?? "";
    window.history.replaceState(null, "", window.location.pathname);
    if (!/^[A-Za-z0-9_-]{43}$/.test(nextToken)) {
      setState("invalid");
      return;
    }
    setToken(nextToken);
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password !== confirmation) {
      setError("两次输入的新密码不一致");
      return;
    }
    setState("saving");
    setError(undefined);
    try {
      await confirmPasswordReset({ token, password });
      setState("changed");
    } catch (cause) {
      setError(accountErrorMessage(cause));
      setState("ready");
    }
  }

  return (
    <main className="verification-page">
      <section className="verification-panel" aria-live="polite">
        <div className="account-brand">
          <span className="account-brand-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <strong>AgentHub</strong>
        </div>
        <span
          className="verification-signal"
          data-status={
            state === "changed"
              ? "verified"
              : state === "invalid"
                ? "failed"
                : state === "saving"
                  ? "checking"
                  : "ready"
          }
          aria-hidden="true"
        />
        {state === "invalid" ? (
          <>
            <p className="eyebrow">重置未完成</p>
            <h1>无法使用这个链接</h1>
            <p>重置链接无效或不完整。</p>
          </>
        ) : state === "changed" ? (
          <>
            <p className="eyebrow">密码已更新</p>
            <h1>账户已重新保护</h1>
            <p>所有网页登录会话均已撤销，请使用新密码重新登录。</p>
          </>
        ) : (
          <>
            <p className="eyebrow">恢复访问</p>
            <h1>设置新密码</h1>
            <form className="reset-password-form" onSubmit={submit}>
              <label>
                <span>新密码</span>
                <input
                  autoComplete="new-password"
                  minLength={10}
                  onChange={(event) => setPassword(event.target.value)}
                  required
                  type="password"
                  value={password}
                />
              </label>
              <label>
                <span>确认新密码</span>
                <input
                  autoComplete="new-password"
                  minLength={10}
                  onChange={(event) => setConfirmation(event.target.value)}
                  required
                  type="password"
                  value={confirmation}
                />
              </label>
              {error !== undefined && (
                <p className="account-error" role="alert">
                  {error}
                </p>
              )}
              <button
                className="account-submit"
                disabled={state === "saving"}
                type="submit"
              >
                {state === "saving" ? "正在更新…" : "更新密码"}
              </button>
            </form>
          </>
        )}
        {(state === "changed" || state === "invalid") && (
          <a className="verification-return" href="/">
            返回登录
          </a>
        )}
      </section>
    </main>
  );
}
