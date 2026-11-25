import { useCallback, useMemo, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import type { AccountView } from "../lib/account";
import { fetchDevices, type GatewayIdentity } from "../lib/api";
import type { PanelConfig } from "../lib/config";
import { normalizeGatewayBase, normalizeUserId } from "../lib/config";
import { useDialogFocus } from "../lib/dialog-focus";
import type { DeviceRecord } from "../lib/types";
import { PluginSettingsSection } from "./PluginSettingsSection";
import {
  ArrowLeftIcon,
  SETTINGS_SECTIONS,
  type SettingsSectionKey,
} from "./settings-constants";
import { AccountSettingsSection } from "./settings/AccountSettingsSection";
import {
  AppearanceSettingsSection,
  ConnectionSettingsSection,
  InstallSettingsSection,
  NotificationsSettingsSection,
} from "./settings/AppearanceAndAppSections";
import { AuditSettingsSection } from "./settings/AuditSettingsSection";
import { DevicesSettingsSection } from "./settings/DevicesSettingsSection";

interface SettingsPageProps {
  account: AccountView | undefined;
  config: PanelConfig;
  onSave: (config: PanelConfig) => void;
  onAccountUpdated?: ((account: AccountView) => void) | undefined;
  onWorkspaceChanged?: (account: AccountView) => void;
  notificationPresentedAt: number | undefined;
  onLoggedOut: () => void;
  onClose: () => void;
}

export function SettingsPage({
  account,
  config,
  onSave,
  onAccountUpdated,
  notificationPresentedAt,
  onLoggedOut,
  onClose,
}: SettingsPageProps) {
  const [activeSection, setActiveSection] =
    useState<SettingsSectionKey>("appearance");
  const [gatewayBase, setGatewayBase] = useState(config.gatewayBase);
  const [userId, setUserId] = useState(config.userId);
  const identity = useMemo<GatewayIdentity>(
    () => ({ userId: account?.user.user_id ?? config.userId }),
    [account?.user.user_id, config.userId],
  );
  const [error, setError] = useState<string | undefined>(undefined);
  const [devices, setDevices] = useState<DeviceRecord[] | undefined>(undefined);
  const [auditRevision, setAuditRevision] = useState(0);

  const sections = SETTINGS_SECTIONS.filter(
    (section) => !section.requiresAccount || account !== undefined,
  );
  const activeEntry =
    sections.find((section) => section.key === activeSection) ??
    sections.find((section) => section.key === "connection")!;

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(undefined);
    try {
      onSave({
        gatewayBase: normalizeGatewayBase(gatewayBase),
        userId: normalizeUserId(userId),
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "配置无效");
    }
  }

  const devicesRequestRef = useRef<Promise<void> | null>(null);
  const handleEnsureDevices = useCallback(() => {
    if (devicesRequestRef.current !== null) return;
    const req = fetchDevices(config, identity)
      .then((next) => setDevices(next))
      .catch(() => {})
      .finally(() => {
        if (devicesRequestRef.current === req) {
          devicesRequestRef.current = null;
        }
      });
    devicesRequestRef.current = req;
  }, [config, identity]);

  const settingsDialogRef = useRef<HTMLDivElement>(null);
  useDialogFocus(true, settingsDialogRef);

  const content = (
    <div
      ref={settingsDialogRef}
      className="settings-page"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-dialog-title"
    >
      <header className="settings-page-header">
        <div>
          <h1 id="settings-dialog-title">设置</h1>
          <p>管理账户安全、连接、设备与等待提醒。</p>
        </div>
        <button
          type="button"
          className="icon-button"
          aria-label="返回控制台"
          onClick={onClose}
        >
          <ArrowLeftIcon size={14} />
          <span>返回控制台</span>
        </button>
      </header>

      <div className="settings-page-body">
        <nav className="settings-page-nav" aria-label="设置分区">
          {sections.map((section) => {
            const SectionIcon = section.icon;
            return (
              <button
                key={section.key}
                type="button"
                data-active={section.key === activeEntry.key}
                aria-current={
                  section.key === activeEntry.key ? "page" : undefined
                }
                onClick={() => setActiveSection(section.key)}
              >
                <span className="settings-section-icon" aria-hidden="true">
                  <SectionIcon size={16} />
                </span>
                <span>{section.label}</span>
              </button>
            );
          })}
        </nav>

        <div
          key={activeEntry.key}
          className="settings-page-content settings-section-fade"
          aria-label="设置内容"
        >
          {activeEntry.key === "appearance" && <AppearanceSettingsSection />}

          {activeEntry.key === "plugins" && (
            <PluginSettingsSection
              config={config}
              identity={identity}
              devices={devices}
              onEnsureDevices={handleEnsureDevices}
            />
          )}

          {activeEntry.key === "account" && account !== undefined && (
            <AccountSettingsSection
              account={account}
              onAccountUpdated={onAccountUpdated}
              onLoggedOut={onLoggedOut}
              onAuditMutated={() => setAuditRevision((current) => current + 1)}
            />
          )}

          {activeEntry.key === "audit" && account !== undefined && (
            <AuditSettingsSection revision={auditRevision} />
          )}

          {activeEntry.key === "connection" && (
            <form onSubmit={handleSubmit}>
              <ConnectionSettingsSection
                account={account}
                gatewayBase={gatewayBase}
                userId={userId}
                onGatewayBaseChange={setGatewayBase}
                onUserIdChange={setUserId}
              />
              {error !== undefined && (
                <p className="error-text settings-form-error" role="alert">
                  {error}
                </p>
              )}
            </form>
          )}

          {activeEntry.key === "devices" && (
            <DevicesSettingsSection
              config={config}
              identity={identity}
              devices={devices}
              onDevicesChange={setDevices}
            />
          )}

          {activeEntry.key === "notifications" && (
            <NotificationsSettingsSection
              config={config}
              identity={identity}
              notificationPresentedAt={notificationPresentedAt}
            />
          )}

          {activeEntry.key === "install" && <InstallSettingsSection />}
        </div>
      </div>
    </div>
  );

  return typeof document === "undefined"
    ? content
    : createPortal(content, document.body);
}
