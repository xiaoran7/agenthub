import { useEffect, useState, type FormEvent } from "react";

import {
  ApiError,
  createDeviceBinding,
  describeApiError,
  revokeDevice,
  type DeviceBinding,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import type { NotificationState } from "../lib/notifications";
import { deviceShortId } from "../lib/format";
import type { DeviceRecord } from "../lib/types";
import {
  disableNotifications,
  enableNotifications,
  readNotificationState,
} from "../lib/notifications";

interface SettingsPanelProps {
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  devices: DeviceRecord[];
  onDevicesChanged: () => void;
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

export function SettingsPanel({
  config,
  devices,
  onSave,
  onDevicesChanged,
  onClose,
}: SettingsPanelProps) {
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
  const [binding, setBinding] = useState<DeviceBinding | undefined>(undefined);
  const [bindingBusy, setBindingBusy] = useState(false);
  const [deviceActionError, setDeviceActionError] = useState<
    string | undefined
  >(undefined);
  const [revokingDevice, setRevokingDevice] = useState<string | undefined>(
    undefined,
  );

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

  async function generateBindingCode() {
    setBindingBusy(true);
    setDeviceActionError(undefined);
    try {
      setBinding(await createDeviceBinding(config));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setBindingBusy(false);
    }
  }

  async function revoke(device: DeviceRecord) {
    if (
      device.revoked ||
      !window.confirm(
        `撤销设备 ${deviceShortId(device.device_id)}？现有历史会保留，但该设备将无法继续同步。`,
      )
    ) {
      return;
    }
    setRevokingDevice(device.device_id);
    setDeviceActionError(undefined);
    try {
      await revokeDevice(config, device.device_id);
      onDevicesChanged();
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setRevokingDevice(undefined);
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
          仅供显式开启的本地开发模式使用；部署访问令牌保留在 Web
          服务端，不会存入浏览器。
        </p>
        <div className="settings-toggle">
          <span>绑定新设备</span>
          <button
            type="button"
            className="secondary"
            disabled={bindingBusy}
            onClick={() => void generateBindingCode()}
          >
            {bindingBusy ? "生成中…" : "生成绑定码"}
          </button>
        </div>
        {binding !== undefined && (
          <>
            <input
              className="mono"
              value={binding.binding_code}
              readOnly
              aria-label="一次性绑定码"
              onFocus={(event) => event.currentTarget.select()}
            />
            <p className="settings-hint">
              10 分钟内使用，仅显示在本次设置窗口；点击输入框可全选复制。
            </p>
          </>
        )}
        {devices.length > 0 && (
          <div className="device-management">
            <span>已绑定设备</span>
            {devices.map((device) => (
              <div className="device-management-row" key={device.device_id}>
                <span className="mono" title={device.device_id}>
                  {deviceShortId(device.device_id)}
                </span>
                <button
                  type="button"
                  className="secondary"
                  disabled={
                    device.revoked || revokingDevice === device.device_id
                  }
                  onClick={() => void revoke(device)}
                >
                  {device.revoked
                    ? "已撤销"
                    : revokingDevice === device.device_id
                      ? "撤销中…"
                      : "撤销"}
                </button>
              </div>
            ))}
          </div>
        )}
        {deviceActionError !== undefined && (
          <p className="error-text">{deviceActionError}</p>
        )}
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
