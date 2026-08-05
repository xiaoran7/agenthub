import { useEffect, useMemo, useState } from "react";

import {
  createDeviceBinding,
  describeApiError,
  type DeviceBinding,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { useModalDialog } from "../lib/dialog-focus";
import { Icon } from "./Icon";

interface DeviceConnectDialogProps {
  config: PanelConfig;
  onClose: () => void;
}

function formatRemaining(milliseconds: number): string {
  const totalSeconds = Math.max(0, Math.ceil(milliseconds / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function DeviceConnectDialog({
  config,
  onClose,
}: DeviceConnectDialogProps) {
  const dialogRef = useModalDialog<HTMLDivElement>(onClose);
  const [binding, setBinding] = useState<DeviceBinding | undefined>();
  const [bindingBusy, setBindingBusy] = useState(false);
  const [bindingCopied, setBindingCopied] = useState(false);
  const [connectionCopied, setConnectionCopied] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [error, setError] = useState<string | undefined>();

  const gatewayOrigin = useMemo(() => {
    if (config.gatewayBase !== "") return config.gatewayBase;
    if (typeof window !== "undefined") return window.location.origin;
    return "https://platform.tannerlab.xyz";
  }, [config.gatewayBase]);

  const parsedExpiresAtMs =
    binding === undefined ? Number.NaN : Date.parse(binding.expires_at);
  const expiresAtMs = Number.isFinite(parsedExpiresAtMs)
    ? parsedExpiresAtMs
    : undefined;
  const remainingMs =
    expiresAtMs === undefined ? undefined : expiresAtMs - nowMs;
  const expired = remainingMs !== undefined && remainingMs <= 0;

  useEffect(() => {
    if (binding === undefined || expired) return;
    const interval = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [binding, expired]);

  async function generateBindingCode() {
    setBindingBusy(true);
    setError(undefined);
    setBindingCopied(false);
    setConnectionCopied(false);
    try {
      setBinding(await createDeviceBinding(config));
      setNowMs(Date.now());
    } catch (cause) {
      setError(describeApiError(cause));
    } finally {
      setBindingBusy(false);
    }
  }

  async function copyText(text: string, onCopied: (copied: boolean) => void) {
    setError(undefined);
    try {
      await navigator.clipboard.writeText(text);
      onCopied(true);
    } catch {
      onCopied(false);
      setError("无法自动复制，请手动选中上面的内容复制。");
    }
  }

  const connectionText = binding
    ? `AgentPulse 设备接入\n平台地址：${gatewayOrigin}\n连接码：${binding.binding_code}`
    : "";

  return (
    <div
      className="device-connect-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="device-connect-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="device-connect-title"
        tabIndex={-1}
      >
        <header className="device-connect-header">
          <div>
            <p className="eyebrow">设备接入</p>
            <h2 id="device-connect-title">连接一台新设备</h2>
            <p>让这台电脑开始把 Claude Code 和 Codex 的状态同步到这里。</p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭设备接入"
            onClick={onClose}
          >
            <Icon name="close" size={18} />
          </button>
        </header>

        <div className="device-connect-body">
          <div className="device-connect-steps" aria-label="设备接入步骤">
            <div
              className="device-connect-step"
              data-active={binding === undefined || expired}
            >
              <span>01</span>
              <strong>生成连接码</strong>
            </div>
            <div
              className="device-connect-step"
              data-active={binding !== undefined && !expired}
            >
              <span>02</span>
              <strong>打开 Runner</strong>
            </div>
            <div className="device-connect-step">
              <span>03</span>
              <strong>等待设备上线</strong>
            </div>
          </div>

          <section className="device-connect-origin">
            <div>
              <span className="device-connect-label">平台地址</span>
              <strong className="mono">{gatewayOrigin}</strong>
            </div>
            <span className="device-connect-origin-state">
              <span className="status-dot" data-tone="live" />
              当前平台
            </span>
          </section>

          {binding === undefined || expired ? (
            <section className="device-connect-start">
              <div className="device-connect-icon">
                <Icon name="devices" size={24} />
              </div>
              <div>
                <h3>{expired ? "连接码已过期" : "准备连接新电脑"}</h3>
                <p>
                  点击生成一个 10 分钟内有效的连接码。连接码只用于这台设备，
                  不包含服务器访问令牌。
                </p>
              </div>
              <button
                type="button"
                className="primary device-connect-generate"
                disabled={bindingBusy}
                onClick={() => void generateBindingCode()}
              >
                {bindingBusy ? "生成中…" : expired ? "重新生成" : "生成连接码"}
                <Icon name="arrow-right" size={15} />
              </button>
            </section>
          ) : (
            <section className="device-connect-code-section">
              <div className="device-connect-code-heading">
                <div>
                  <span className="device-connect-label">本次连接码</span>
                  <p>在新设备的 Windows 接入包中输入，或复制完整连接信息。</p>
                </div>
                <span className="device-connect-expiry" data-expired={expired}>
                  <Icon name="clock" size={14} />
                  {remainingMs === undefined
                    ? "有效期未知"
                    : `剩余 ${formatRemaining(remainingMs)}`}
                </span>
              </div>
              <div className="device-connect-code" aria-label="一次性连接码">
                {binding.binding_code}
              </div>
              <div className="device-connect-actions">
                <button
                  type="button"
                  className="primary"
                  onClick={() =>
                    void copyText(binding.binding_code, setBindingCopied)
                  }
                >
                  <Icon name="check" size={15} />
                  {bindingCopied ? "已复制连接码" : "复制连接码"}
                </button>
                <button
                  type="button"
                  className="secondary"
                  onClick={() =>
                    void copyText(connectionText, setConnectionCopied)
                  }
                >
                  {connectionCopied ? "已复制完整信息" : "复制完整连接信息"}
                </button>
                <button
                  type="button"
                  className="secondary"
                  disabled={bindingBusy}
                  onClick={() => void generateBindingCode()}
                >
                  重新生成
                </button>
              </div>
            </section>
          )}

          <ol className="device-connect-instructions">
            <li>
              <span>1</span>
              <p>在目标电脑双击 Windows 接入包里的 Connect-AgentPulse.cmd。</p>
            </li>
            <li>
              <span>2</span>
              <p>把平台地址和连接码粘贴到接入窗口，按提示确认。</p>
            </li>
            <li>
              <span>3</span>
              <p>回到这里，设备上线后会自动出现在设备列表和总览中。</p>
            </li>
          </ol>

          {error !== undefined && (
            <p className="error-text device-connect-error" role="alert">
              {error}
            </p>
          )}
        </div>

        <footer className="device-connect-footer">
          <span>
            <span className="status-dot" data-tone="waiting" />
            连接码仅显示在本次窗口中
          </span>
          <button type="button" className="secondary" onClick={onClose}>
            稍后连接
          </button>
        </footer>
      </div>
    </div>
  );
}
