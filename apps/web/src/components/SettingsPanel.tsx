import { useState, type FormEvent } from "react";

import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";

interface SettingsPanelProps {
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  onClose: () => void;
}

export function SettingsPanel({ config, onSave, onClose }: SettingsPanelProps) {
  const [gatewayBase, setGatewayBase] = useState(config.gatewayBase);
  const [userId, setUserId] = useState(config.userId);
  const [error, setError] = useState<string | undefined>(undefined);

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      onSave({
        gatewayBase: normalizeGatewayBase(gatewayBase),
        userId: normalizeUserId(userId),
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "配置无效");
    }
  }

  return (
    <div
      className="settings-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label="面板设置"
    >
      <form className="settings" onSubmit={handleSubmit}>
        <h2>设置</h2>
        <label>
          网关地址
          <input
            value={gatewayBase}
            onChange={(event) => setGatewayBase(event.target.value)}
            placeholder="留空使用当前源（开发代理）"
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <p className="settings-hint">
          留空时请求经当前源的 /v1 代理转发；直连其他地址需要网关允许跨源。
          非回环地址必须使用 https。
        </p>
        <label>
          用户标识（x-user-id）
          <input
            value={userId}
            onChange={(event) => setUserId(event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <p className="settings-hint">
          开发期身份标识，正式认证接入后会被替换。
        </p>
        {error !== undefined && <p className="error-text">{error}</p>}
        <div className="settings-actions">
          <button type="button" className="secondary" onClick={onClose}>
            取消
          </button>
          <button type="submit">保存</button>
        </div>
      </form>
    </div>
  );
}
