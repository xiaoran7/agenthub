"use client";

import { useEffect, useState } from "react";

import { accountErrorMessage, confirmEmailVerification } from "../lib/account";

type VerificationState =
  | { status: "checking" }
  | { status: "verified"; email: string }
  | { status: "failed"; message: string };

export function VerifyEmailClient() {
  const [state, setState] = useState<VerificationState>({ status: "checking" });

  useEffect(() => {
    let cancelled = false;
    const token =
      new URLSearchParams(window.location.hash.slice(1)).get("token") ?? "";
    window.history.replaceState(null, "", window.location.pathname);
    if (!/^[A-Za-z0-9_-]{43}$/.test(token)) {
      setState({ status: "failed", message: "验证链接无效或不完整。" });
      return;
    }
    void confirmEmailVerification(token)
      .then((account) => {
        if (!cancelled) {
          setState({ status: "verified", email: account.user.email });
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setState({ status: "failed", message: accountErrorMessage(error) });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main className="verification-page">
      <section className="verification-panel" aria-live="polite">
        <div className="account-brand">
          <span className="account-brand-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <strong>AgentPulse</strong>
        </div>
        <span
          className="verification-signal"
          data-status={state.status}
          aria-hidden="true"
        />
        {state.status === "checking" && (
          <>
            <p className="eyebrow">账户验证</p>
            <h1>正在确认邮箱</h1>
            <p>正在检查验证链接的有效性。</p>
          </>
        )}
        {state.status === "verified" && (
          <>
            <p className="eyebrow">验证完成</p>
            <h1>邮箱已验证</h1>
            <p>{state.email} 现在可以绑定设备并使用订阅功能。</p>
          </>
        )}
        {state.status === "failed" && (
          <>
            <p className="eyebrow">验证未完成</p>
            <h1>无法确认这个链接</h1>
            <p>{state.message}</p>
          </>
        )}
        {state.status !== "checking" && (
          <a className="verification-return" href="/">
            返回控制台
          </a>
        )}
      </section>
    </main>
  );
}
