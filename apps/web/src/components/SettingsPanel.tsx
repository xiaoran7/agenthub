import { useEffect, useState, type FormEvent } from "react";

import { ApiError, describeApiError } from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import type { NotificationState } from "../lib/notifications";
import {
  disableNotifications,
  enableNotifications,
  readNotificationState,
} from "../lib/notifications";

interface SettingsPanelProps {
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  onClose: () => void;
}

const NOTIFICATION_HINTS: Record<NotificationState, string> = {
  unavailable:
    "当前环境不支持通知：需要 https 或 localhost，且浏览器支持推送。",
  disabled: "网关未配置推送密钥，通知整体关闭。",
  denied: "浏览器已拒绝本站通知权限，需要在站点设置里恢复。",
  on: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
  off: "会话进入等待输入或等待权限时提醒你，面板关闭也能收到。",
};

export function SettingsPanel({ config, onSave, onClose }: SettingsPanelProps) {
  const [gatewayBase, setGatewayBase] = useState(config.gatewayBase);
  const [userId, setUserId] = useState(config.userId);
  const [error, setError] = useState<string | undefined>(undefined);
  const [notifications, setNotifications] = useState<
    NotificationState | undefined
  >(undefined);
  const [notificationError, setNotificationError] = useState<
    string | undefined
  >(undefined);
  const [notificationBusy, setNotificationBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void readNotificationState(config)
      .catch((): NotificationState => "unavailable")
      .then((state) => {
        if (!cancelled) {
          setNotifications(state);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [config]);

  async function toggleNotifications() {
    setNotificationBusy(true);
    setNotificationError(undefined);
    try {
      setNotifications(
        notifications === "on"
          ? await disableNotifications(config)
          : await enableNotifications(config),
      );
    } catch (cause) {
      setNotificationError(
        cause instanceof ApiError
          ? describeApiError(cause)
          : cause instanceof Error
            ? cause.message
            : "通知设置失败",
      );
    } finally {
      setNotificationBusy(false);
    }
  }

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
        <div className="settings-toggle">
          <span>推送通知</span>
          <button
            type="button"
            className="secondary"
            disabled={
              notifications === undefined ||
              notificationBusy ||
              notifications === "unavailable" ||
              notifications === "disabled" ||
              notifications === "denied"
            }
            aria-pressed={notifications === "on"}
            onClick={() => void toggleNotifications()}
          >
            {notifications === undefined
              ? "检查中…"
              : notificationBusy
                ? "处理中…"
                : notifications === "on"
                  ? "已开启"
                  : "开启"}
          </button>
        </div>
        <p className="settings-hint">
          {notifications === undefined
            ? "正在检查通知能力…"
            : NOTIFICATION_HINTS[notifications]}
        </p>
        {notificationError !== undefined && (
          <p className="error-text">{notificationError}</p>
        )}
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
