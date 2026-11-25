import { useEffect, useState } from "react";
import type { AccountView } from "../../lib/account";
import {
  ApiError,
  describeApiError,
  type GatewayIdentity,
} from "../../lib/api";
import type { PanelConfig } from "../../lib/config";
import { formatClock } from "../../lib/format";
import {
  disableNotifications,
  enableNotifications,
  readNotificationState,
  type NotificationState,
} from "../../lib/notifications";
import { usePwaInstall } from "../../lib/pwa-install";
import {
  getStoredTheme,
  initThemeListener,
  setTheme,
  type ThemeMode,
} from "../../lib/theme";
import {
  ConnectionIcon,
  INSTALL_HINTS,
  INSTALL_LABELS,
  InstallIcon,
  MonitorIcon,
  MoonIcon,
  NOTIFICATION_HINTS,
  NotificationIcon,
  PaletteIcon,
  SunIcon,
} from "../settings-constants";

export function AppearanceSettingsSection() {
  const [currentTheme, setCurrentTheme] = useState<ThemeMode>(() =>
    getStoredTheme(),
  );

  useEffect(() => {
    return initThemeListener((mode) => {
      setCurrentTheme(mode);
    });
  }, []);

  return (
    <section className="settings-section" aria-labelledby="appearance-settings">
      <div className="settings-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <PaletteIcon size={18} />
        </span>
        <div>
          <h3 id="appearance-settings">外观与主题</h3>
          <p>
            选择适合当前光线环境的界面配色，减轻长时间阅读与协作的视觉疲劳。
          </p>
        </div>
      </div>

      <div className="settings-theme-grid">
        {(
          [
            {
              mode: "system",
              title: "跟随系统",
              desc: "自动跟随系统与浏览器的日间、夜间设置",
              badge: "自动",
              icon: <MonitorIcon size={16} />,
            },
            {
              mode: "light",
              title: "温润纸张",
              desc: "柔和温暖的浅色纸面质感，适合日间明亮环境阅读",
              badge: "浅色",
              icon: <SunIcon size={16} />,
            },
            {
              mode: "dark",
              title: "黑曜石暖炭",
              desc: "低对比度的暖调深色背景，夜间专注使用更护眼",
              badge: "深色",
              icon: <MoonIcon size={16} />,
            },
          ] as const
        ).map((item) => {
          const isSelected = currentTheme === item.mode;
          return (
            <button
              key={item.mode}
              type="button"
              className="settings-theme-card"
              data-selected={isSelected}
              aria-pressed={isSelected}
              onClick={() => {
                setTheme(item.mode);
                setCurrentTheme(item.mode);
              }}
            >
              <div className="settings-theme-card-top">
                <div className="settings-theme-card-title">
                  <span className="settings-theme-card-icon">{item.icon}</span>
                  <strong>{item.title}</strong>
                </div>
                <span className="settings-theme-card-badge">{item.badge}</span>
              </div>
              <p className="settings-theme-card-desc">{item.desc}</p>
            </button>
          );
        })}
      </div>

      <p className="settings-hint">
        切换后立即生效，并自动保存在当前浏览器中。
      </p>
    </section>
  );
}

export interface ConnectionSettingsSectionProps {
  account: AccountView | undefined;
  gatewayBase: string;
  userId: string;
  onGatewayBaseChange: (value: string) => void;
  onUserIdChange: (value: string) => void;
}

export function ConnectionSettingsSection({
  account,
  gatewayBase,
  userId,
  onGatewayBaseChange,
  onUserIdChange,
}: ConnectionSettingsSectionProps) {
  return (
    <section className="settings-section" aria-labelledby="connection-settings">
      <div className="settings-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <ConnectionIcon size={18} />
        </span>
        <div>
          <h3 id="connection-settings">连接</h3>
          <p>
            {account !== undefined
              ? "当前通过已登录的同源安全会话访问网关。"
              : "配置面板读取数据时使用的网关。"}
          </p>
        </div>
      </div>
      {account !== undefined ? (
        <p className="settings-hint">
          当前处于加密会话 Cookie 模式，所有 API 与实时流请求均通过当前源
          /v1、/v3 自动鉴权，无需手动修改网关地址。
        </p>
      ) : (
        <>
          <label>
            网关地址
            <input
              value={gatewayBase}
              onChange={(event) => onGatewayBaseChange(event.target.value)}
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
              onChange={(event) => onUserIdChange(event.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <p className="settings-hint">
            仅供显式开启的本地开发模式使用；部署访问令牌只保留在 Web 服务端。
          </p>
          <div className="settings-actions">
            <button type="submit">保存</button>
          </div>
        </>
      )}
    </section>
  );
}

export interface NotificationsSettingsSectionProps {
  config: PanelConfig;
  identity: GatewayIdentity;
  notificationPresentedAt: number | undefined;
}

export function NotificationsSettingsSection({
  config,
  identity,
  notificationPresentedAt,
}: NotificationsSettingsSectionProps) {
  const [notifications, setNotifications] = useState<
    NotificationState | undefined
  >(undefined);
  const [notificationError, setNotificationError] = useState<
    string | undefined
  >(undefined);
  const [notificationBusy, setNotificationBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void readNotificationState(config, identity)
      .catch((): NotificationState => "unavailable")
      .then((state) => {
        if (!cancelled) {
          setNotifications(state);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [config, identity]);

  async function toggleNotifications() {
    setNotificationBusy(true);
    setNotificationError(undefined);
    try {
      setNotifications(
        notifications === "on"
          ? await disableNotifications(config, identity)
          : await enableNotifications(config, identity),
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

  return (
    <section
      className="settings-section"
      aria-labelledby="notification-settings"
    >
      <div className="settings-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <NotificationIcon size={18} />
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
      {notifications === "on" && notificationPresentedAt !== undefined && (
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
  );
}

export function InstallSettingsSection() {
  const { state: installState, install: onInstall } = usePwaInstall();
  const [installError, setInstallError] = useState<string | undefined>(
    undefined,
  );

  async function installPwa() {
    setInstallError(undefined);
    try {
      await onInstall();
    } catch {
      setInstallError("浏览器未能启动安装流程，请稍后重试。");
    }
  }

  return (
    <section className="settings-section" aria-labelledby="install-settings">
      <div className="settings-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <InstallIcon size={18} />
        </span>
        <div>
          <h3 id="install-settings">安装到设备</h3>
          <p>以独立 PWA 窗口打开 AgentHub。</p>
        </div>
      </div>
      <div className="settings-toggle">
        <span>应用安装</span>
        <button
          type="button"
          className="secondary"
          disabled={
            installState !== "available" && installState !== "installing"
          }
          aria-disabled={installState !== "available"}
          onClick={() => {
            if (installState !== "available") return;
            void installPwa();
          }}
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
  );
}
