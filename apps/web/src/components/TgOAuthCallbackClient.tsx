"use client";

import { useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { accountErrorMessage, completeTgOAuthCallback } from "../lib/account";

export function TgOAuthCallbackClient() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [error, setError] = useState<string>();
  const [processing, setProcessing] = useState(true);

  useEffect(() => {
    let cancelled = false;
    async function processCallback() {
      const code = searchParams.get("code");
      const state = searchParams.get("state");
      const oauthError = searchParams.get("error");
      const oauthErrorDesc = searchParams.get("error_description");

      if (oauthError) {
        if (!cancelled) {
          setError(oauthErrorDesc || `OAuth 授权失败: ${oauthError}`);
          setProcessing(false);
        }
        return;
      }

      if (!code) {
        if (!cancelled) {
          setError("缺少授权码参数");
          setProcessing(false);
        }
        return;
      }

      const storedState = sessionStorage.getItem("tg_oauth_state");
      const storedVerifier = sessionStorage.getItem("tg_oauth_code_verifier");

      if (storedState && state && storedState !== state) {
        if (!cancelled) {
          setError("安全状态码校验不匹配，可能存在跨站请求风险");
          setProcessing(false);
        }
        return;
      }

      if (!storedVerifier) {
        if (!cancelled) {
          setError("未找到本地 PKCE 校验码，请返回登录页重试");
          setProcessing(false);
        }
        return;
      }

      try {
        const redirectUri = `${window.location.origin}/auth/callback/tg`;
        await completeTgOAuthCallback({
          code,
          code_verifier: storedVerifier,
          redirect_uri: redirectUri,
        });

        sessionStorage.removeItem("tg_oauth_state");
        sessionStorage.removeItem("tg_oauth_code_verifier");

        if (!cancelled) {
          router.replace("/");
        }
      } catch (err) {
        if (!cancelled) {
          setError(accountErrorMessage(err));
          setProcessing(false);
        }
      }
    }

    void processCallback();
    return () => {
      cancelled = true;
    };
  }, [router, searchParams]);

  return (
    <main
      className="account-entry"
      style={{
        minHeight: "100dvh",
        display: "grid",
        placeItems: "center",
        background: "#11110f",
        color: "#f5f2ec",
      }}
    >
      <div
        style={{
          textAlign: "center",
          padding: "36px",
          maxWidth: "420px",
          width: "90%",
          background: "#1b1b18",
          borderRadius: "24px",
          border: "1px solid #2a2a26",
        }}
      >
        {processing ? (
          <div>
            <span
              style={{
                color: "#ff4f87",
                fontSize: "24px",
                display: "block",
                marginBottom: "12px",
              }}
            >
              ◆
            </span>
            <h2
              style={{
                fontSize: "20px",
                margin: "0 0 12px",
                fontFamily: "sans-serif",
              }}
            >
              正在完成 TannerLab 统一登录…
            </h2>
            <p
              style={{
                color: "#b9b5ad",
                fontSize: "14px",
                margin: 0,
                lineHeight: 1.6,
              }}
            >
              验证 OAuth 凭证与账号绑定，即将跳转控制台
            </p>
          </div>
        ) : error ? (
          <div>
            <h2
              style={{ fontSize: "20px", margin: "0 0 12px", color: "#ef7b74" }}
            >
              登录未完成
            </h2>
            <p
              style={{
                color: "#b9b5ad",
                fontSize: "14px",
                margin: "0 0 20px",
                lineHeight: 1.6,
              }}
            >
              {error}
            </p>
            <button
              onClick={() => router.replace("/")}
              style={{
                background: "#7ed5a0",
                color: "#11110f",
                border: 0,
                padding: "12px 24px",
                borderRadius: "12px",
                fontWeight: 700,
                fontSize: "14px",
                cursor: "pointer",
              }}
            >
              返回登录页
            </button>
          </div>
        ) : null}
      </div>
    </main>
  );
}
