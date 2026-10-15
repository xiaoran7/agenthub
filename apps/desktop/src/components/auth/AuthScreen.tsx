import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { useState, type FormEvent } from "react";
import { SimpleTitleBar, useWindowControls } from "../layout/WindowControls";
import type { Snapshot } from "../../types";

export type AuthMode = "login" | "register";

function Brand({ size = 26 }: { size?: number }) {
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
  const { handleStartDrag, handleToggleMaximize } = useWindowControls();
  const [mode, setMode] = useState<AuthMode>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [serverUrl, setServerUrl] = useState(() => {
    try {
      return localStorage.getItem("agenthub_server_url") || "";
    } catch {
      return "";
    }
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>(initialError);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const cleanUrl = serverUrl.trim();
    if (!cleanUrl) {
      setError("请输入自部署服务器地址（如 http://localhost:3000）");
      return;
    }
    setBusy(true);
    setError(undefined);
    try {
      try {
        localStorage.setItem("agenthub_server_url", cleanUrl);
      } catch {}
      const next = await invoke<Snapshot>("authenticate", {
        input: {
          mode,
          email: email.trim(),
          password,
          display_name:
            mode === "register" && displayName.trim()
              ? displayName.trim()
              : undefined,
          server_url: cleanUrl,
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
    <div className="auth-page-container">
      <SimpleTitleBar />
      <main
        className="auth-shell"
        data-tauri-drag-region
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) {
            handleStartDrag(e);
          }
        }}
        onDoubleClick={(e) => {
          if (e.target === e.currentTarget) {
            handleToggleMaximize(e);
          }
        }}
      >
        <div className="auth-card">
          <header
            className="auth-header"
            data-tauri-drag-region
            onMouseDown={(e) => {
              const target = e.target as HTMLElement;
              if (
                target.tagName !== "INPUT" &&
                target.tagName !== "BUTTON" &&
                target.tagName !== "A"
              ) {
                handleStartDrag(e);
              }
            }}
            onDoubleClick={handleToggleMaximize}
          >
            <Brand size={36} />
            <h2>{mode === "login" ? "管理员登录" : "首次初始化管理员"}</h2>
            <p>
              {mode === "login"
                ? "连接到自部署的 AgentHub 服务，接续你的原生 Agent 会话"
                : "首次启动请配置服务器首位超级管理员账号"}
            </p>
          </header>

          <form onSubmit={submit} className="auth-form">
            <label>
              <span>服务器地址</span>
              <input
                type="url"
                placeholder="例如 http://localhost:3000 或 https://你的服务器"
                value={serverUrl}
                onChange={(e) => {
                  setServerUrl(e.target.value);
                  try {
                    localStorage.setItem("agenthub_server_url", e.target.value);
                  } catch {}
                }}
                required
                autoFocus={!serverUrl}
              />
            </label>

            <label>
              <span>管理员邮箱</span>
              <input
                type="email"
                placeholder="admin@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoFocus={Boolean(serverUrl)}
              />
            </label>

            <label>
              <span>密码</span>
              <input
                type="password"
                placeholder="••••••••••"
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
                  placeholder="管理员"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
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
                "连接并登录"
              ) : (
                "初始化管理员并连接"
              )}
            </button>

            <footer className="auth-footer">
              {mode === "login" ? (
                <span>
                  服务器尚未初始化？
                  <button type="button" onClick={() => setMode("register")}>
                    首次初始化
                  </button>
                </span>
              ) : (
                <span>
                  已有管理员账号？
                  <button type="button" onClick={() => setMode("login")}>
                    直接登录
                  </button>
                </span>
              )}
            </footer>
          </form>
        </div>
      </main>
    </div>
  );
}
