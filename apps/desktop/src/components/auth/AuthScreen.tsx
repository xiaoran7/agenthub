import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, Sparkles } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import type {
  Snapshot,
  TgDeviceCodeResponse,
  TgDevicePollResponse,
} from "../../types";

export type AuthMode = "login" | "register";

export function Brand({ size = 26 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 48 48"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="brand-icon"
      aria-hidden="true"
    >
      <rect
        x="3"
        y="3"
        width="42"
        height="42"
        rx="12"
        className="brand-bg"
        fill="currentColor"
        fillOpacity="0.08"
      />
      <rect
        x="3"
        y="3"
        width="42"
        height="42"
        rx="12"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeOpacity="0.25"
      />
      <path
        d="M12 25 L18 25 L21 16 L27 32 L30 22 L33 25 L36 25"
        stroke="currentColor"
        strokeWidth="3.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx="21" cy="16" r="2.2" fill="currentColor" />
      <circle cx="27" cy="32" r="2.2" fill="currentColor" />
      <circle cx="30" cy="22" r="2" fill="currentColor" />
    </svg>
  );
}

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    if ("message" in error && typeof error.message === "string") {
      return error.message;
    }
    if ("code" in error && typeof error.code === "string") {
      return error.code;
    }
  }
  return "操作失败，请重试";
}

export function AuthScreen({
  initialError,
  onAuthenticated,
}: {
  initialError?: string;
  onAuthenticated: (snapshot: Snapshot) => void;
}) {
  const [mode, setMode] = useState<AuthMode>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [serverUrl, setServerUrl] = useState("https://platform.tannerlab.xyz");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>(initialError);

  // TG device code auth state
  const [deviceFlow, setDeviceFlow] = useState<TgDeviceCodeResponse>();
  const [polling, setPolling] = useState(false);

  const startTgDeviceAuth = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const resp = await invoke<TgDeviceCodeResponse>("start_tg_device_auth", {
        serverUrl: serverUrl.trim() || undefined,
      });
      setDeviceFlow(resp);
      setPolling(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!polling || !deviceFlow) return;
    let timer: number | undefined;
    let disposed = false;
    const intervalMs = Math.max(2000, deviceFlow.interval * 1000);

    const poll = async () => {
      try {
        const resp = await invoke<TgDevicePollResponse>("poll_tg_device_auth", {
          deviceCode: deviceFlow.device_code,
          serverUrl: serverUrl.trim() || undefined,
        });
        if (disposed) return;
        if (resp.status === "success" && resp.snapshot) {
          onAuthenticated(resp.snapshot);
          return;
        }
      } catch (err) {
        if (!disposed) {
          setError(errorMessage(err));
          setPolling(false);
        }
        return;
      }
      if (!disposed) {
        timer = window.setTimeout(poll, intervalMs);
      }
    };

    timer = window.setTimeout(poll, intervalMs);
    return () => {
      disposed = true;
      if (timer) window.clearTimeout(timer);
    };
  }, [polling, deviceFlow, serverUrl, onAuthenticated]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const next = await invoke<Snapshot>("authenticate", {
        input: {
          mode,
          email: email.trim(),
          password,
          display_name:
            mode === "register" && displayName.trim()
              ? displayName.trim()
              : undefined,
          server_url: serverUrl.trim() ? serverUrl.trim() : undefined,
        },
      });
      onAuthenticated(next);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="auth-shell">
      <div className="auth-card">
        <header className="auth-header">
          <Brand size={36} />
          <h2>{mode === "login" ? "登录 AgentHub" : "注册新账号"}</h2>
          <p>
            {mode === "login"
              ? "登录后选择工作目录，连接并接续你的原生 Agent 会话"
              : "创建账号以同步管理多个开发设备"}
          </p>
        </header>

        {deviceFlow && polling ? (
          <div className="tg-device-card">
            <p className="tg-device-instructions">
              请在浏览器打开以下页面并确认授权：
            </p>
            <a
              href={
                deviceFlow.verification_uri_complete ||
                deviceFlow.verification_uri
              }
              target="_blank"
              rel="noreferrer"
              className="tg-verify-link"
            >
              {deviceFlow.verification_uri}
            </a>
            <div className="tg-user-code-box">
              <span>验证码</span>
              <strong>{deviceFlow.user_code}</strong>
            </div>
            <div className="loading-line">
              <i />
            </div>
            <p className="tg-waiting-text">正在等待浏览器确认…</p>
            <button
              type="button"
              className="outline-button"
              onClick={() => {
                setPolling(false);
                setDeviceFlow(undefined);
              }}
            >
              返回账号密码登录
            </button>
          </div>
        ) : (
          <form onSubmit={submit} className="auth-form">
            <label>
              <span>邮箱</span>
              <input
                type="email"
                placeholder="developer@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoFocus
              />
            </label>
            <label>
              <span>密码</span>
              <input
                type="password"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>
            {mode === "register" && (
              <label>
                <span>显示名称（可选）</span>
                <input
                  type="text"
                  placeholder="Tang Tang"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                />
              </label>
            )}

            <button
              type="button"
              className="advanced-toggle"
              onClick={() => setShowAdvanced((v) => !v)}
            >
              {showAdvanced ? "收起高级设置" : "高级设置（自定义服务器）"}
            </button>

            {showAdvanced && (
              <label className="advanced-field">
                <span>服务器地址</span>
                <input
                  type="url"
                  placeholder="https://platform.tannerlab.xyz"
                  value={serverUrl}
                  onChange={(e) => setServerUrl(e.target.value)}
                />
              </label>
            )}

            {error && <p className="form-error">{error}</p>}

            <button
              type="submit"
              className="primary-button submit-btn"
              disabled={busy}
            >
              {busy ? (
                <>
                  <RefreshCw className="spin" size={16} /> 正在连接...
                </>
              ) : mode === "login" ? (
                "登录并连接"
              ) : (
                "创建账号"
              )}
            </button>

            <button
              type="button"
              className="outline-button tg-oauth-btn"
              onClick={startTgDeviceAuth}
              disabled={busy}
            >
              <Sparkles size={16} />
              通过 Telegram / 浏览器快捷登录
            </button>

            <footer className="auth-footer">
              {mode === "login" ? (
                <span>
                  还没有账号？
                  <button type="button" onClick={() => setMode("register")}>
                    立即注册
                  </button>
                </span>
              ) : (
                <span>
                  已有账号？
                  <button type="button" onClick={() => setMode("login")}>
                    直接登录
                  </button>
                </span>
              )}
            </footer>
          </form>
        )}
      </div>
    </main>
  );
}
