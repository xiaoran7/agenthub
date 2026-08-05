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
import { useModalDialog } from "../lib/dialog-focus";
import type { NotificationState } from "../lib/notifications";
import { deviceShortId, formatClock } from "../lib/format";
import type { PwaInstallState } from "../lib/pwa-install";
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
  notificationPresentedAt: number | undefined;
  installState: PwaInstallState;
  onInstall: () => Promise<void>;
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

const INSTALL_HINTS: Record<PwaInstallState, string> = {
  checking: "正在检查当前浏览器的安装能力…",
  available: "安装后可从桌面或主屏幕直接打开，并使用独立窗口。",
  installing: "正在等待浏览器确认安装…",
  submitted: "安装请求已提交；完成后可从系统应用列表打开。",
  installed: "当前已经在独立应用模式中运行。",
  manual: "在 Safari 中点“分享”，然后选择“添加到主屏幕”。",
  unavailable: "当前浏览器暂未提供安装按钮；仍可正常使用网页版本。",
};

const INSTALL_LABELS: Record<PwaInstallState, string> = {
  checking: "检查中…",
  available: "安装应用",
  installing: "安装中…",
  submitted: "已提交",
  installed: "已安装",
  manual: "查看说明",
  unavailable: "暂不可用",
};

export function SettingsPanel({
  config,
  devices,
  onSave,
  onDevicesChanged,
  notificationPresentedAt,
  installState,
  onInstall,
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
  const [installError, setInstallError] = useState<string | undefined>(
    undefined,
  );
  const [binding, setBinding] = useState<DeviceBinding | undefined>(undefined);
  const [bindingCopied, setBindingCopied] = useState(false);
  const [bindingBusy, setBindingBusy] = useState(false);
  const [deviceActionError, setDeviceActionError] = useState<
    string | undefined
  >(undefined);
  const [revokingDevice, setRevokingDevice] = useState<string | undefined>(
    undefined,
  );
  const dialogRef = useModalDialog<HTMLFormElement>(onClose);

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

  async function installPwa() {
    setInstallError(undefined);
    try {
      await onInstall();
    } catch {
      setInstallError("浏览器未能启动安装流程，请稍后重试。");
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
    setBindingCopied(false);
    try {
      setBinding(await createDeviceBinding(config));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setBindingBusy(false);
    }
  }

  async function copyBindingCode() {
    if (binding === undefined) {
      return;
    }
    setDeviceActionError(undefined);
    try {
      await navigator.clipboard.writeText(binding.binding_code);
      setBindingCopied(true);
    } catch {
      setDeviceActionError("无法自动复制，请点按绑定码后手动复制。");
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
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <form
        ref={dialogRef}
        className="settings"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
        tabIndex={-1}
        onSubmit={handleSubmit}
      >
        <header className="settings-header">
          <div>
            <p className="eyebrow">面板设置</p>
            <h2 id="settings-title">面板设置</h2>
            <p>管理连接、设备与等待提醒。</p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭设置"
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className="settings-scroll">
          <section
            className="settings-section"
            aria-labelledby="connection-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ↗
              </span>
              <div>
                <h3 id="connection-settings">连接</h3>
                <p>配置面板读取数据时使用的网关。</p>
              </div>
            </div>
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
              留空时请求经当前源的 /v1 代理转发；非回环直连地址必须使用 https。
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
              仅供显式开启的本地开发模式使用；部署访问令牌只保留在 Web 服务端。
            </p>
          </section>

          <section
            className="settings-section"
            aria-labelledby="device-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ◫
              </span>
              <div>
                <h3 id="device-settings">设备</h3>
                <p>绑定新的 Runner，或撤销不再使用的设备。</p>
              </div>
            </div>
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
              <div className="binding-code">
                <input
                  className="mono"
                  value={binding.binding_code}
                  readOnly
                  aria-label="一次性绑定码"
                  onFocus={(event) => event.currentTarget.select()}
                />
                <button
                  type="button"
                  className="secondary"
                  onClick={() => void copyBindingCode()}
                >
                  {bindingCopied ? "已复制" : "复制"}
                </button>
                <p className="settings-hint">
                  10 分钟内有效，仅显示在本次设置窗口。
                </p>
              </div>
            )}
            {devices.length > 0 && (
              <div className="device-management">
                <span className="settings-subheading">已绑定设备</span>
                {devices.map((device) => (
                  <div className="device-management-row" key={device.device_id}>
                    <span className="device-management-copy">
                      <span
                        className="device-dot"
                        data-online={device.online && !device.revoked}
                      />
                      <span>
                        <span className="mono" title={device.device_id}>
                          {deviceShortId(device.device_id)}
                        </span>
                        <small>
                          {device.revoked
                            ? "已撤销"
                            : device.online
                              ? "在线"
                              : "离线"}
                        </small>
                      </span>
                    </span>
                    <button
                      type="button"
                      className="secondary danger-button"
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
              <p className="error-text" role="alert">
                {deviceActionError}
              </p>
            )}
          </section>

          <section
            className="settings-section"
            aria-labelledby="notification-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ◉
              </span>
              <div>
                <h3 id="notification-settings">等待提醒</h3>
                <p>面板关闭后仍可接收等待输入或权限通知。</p>
              </div>
            </div>
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
            {notifications === "on" &&
              notificationPresentedAt !== undefined && (
                <p className="settings-receipt" role="status">
                  最近展示请求：{" "}
                  {formatClock(new Date(notificationPresentedAt).toISOString())}
                  。Windows 是否弹出仍受系统设置影响。
                </p>
              )}
            {notificationError !== undefined && (
              <p className="error-text" role="alert">
                {notificationError}
              </p>
            )}
          </section>

          <section
            className="settings-section"
            aria-labelledby="install-settings"
          >
            <div className="settings-section-heading">
              <span className="settings-section-icon" aria-hidden="true">
                ⌂
              </span>
              <div>
                <h3 id="install-settings">安装到设备</h3>
                <p>以独立 PWA 窗口打开 AgentPulse。</p>
              </div>
            </div>
            <div className="settings-toggle">
              <span>应用安装</span>
              <button
                type="button"
                className="secondary"
                disabled={installState !== "available"}
                onClick={() => void installPwa()}
              >
                {INSTALL_LABELS[installState]}
              </button>
            </div>
            <p className="settings-hint">{INSTALL_HINTS[installState]}</p>
            {installError !== undefined && (
              <p className="error-text" role="alert">
                {installError}
              </p>
            )}
          </section>

          {error !== undefined && (
            <p className="error-text settings-form-error" role="alert">
              {error}
            </p>
          )}
        </div>

        <footer className="settings-actions">
          <button type="button" className="secondary" onClick={onClose}>
            取消
          </button>
          <button type="submit">保存</button>
        </footer>
      </form>
    </div>
  );
}
