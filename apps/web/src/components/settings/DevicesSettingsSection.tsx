import { useEffect, useRef, useState } from "react";
import {
  createDeviceBinding,
  describeApiError,
  fetchDevices,
  revokeDevice,
  type DeviceBinding,
  type GatewayIdentity,
} from "../../lib/api";
import { copyTextToClipboard } from "../../lib/clipboard";
import type { PanelConfig } from "../../lib/config";
import { deviceShortId } from "../../lib/format";
import type { DeviceRecord } from "../../lib/types";
import { Confirm } from "../Confirm";
import { DevicesIcon } from "../settings-constants";

function deviceFriendlyName(device: DeviceRecord): string {
  if (device.display_name && device.display_name.trim().length > 0) {
    return device.display_name.trim();
  }
  const shortTail = device.device_id.startsWith("dev_")
    ? device.device_id.slice(-6)
    : deviceShortId(device.device_id);
  return `电脑 · ${shortTail}`;
}

export interface DevicesSettingsSectionProps {
  config: PanelConfig;
  identity: GatewayIdentity;
  devices: DeviceRecord[] | undefined;
  onDevicesChange: (devices: DeviceRecord[]) => void;
}

export function DevicesSettingsSection({
  config,
  identity,
  devices,
  onDevicesChange,
}: DevicesSettingsSectionProps) {
  const [binding, setBinding] = useState<DeviceBinding | undefined>(undefined);
  const [bindingCopied, setBindingCopied] = useState(false);
  const [bindingBusy, setBindingBusy] = useState(false);
  const [deviceActionError, setDeviceActionError] = useState<
    string | undefined
  >(undefined);
  const [pendingRevoke, setPendingRevoke] = useState<DeviceRecord | undefined>(
    undefined,
  );
  const [revokingDevice, setRevokingDevice] = useState<string | undefined>(
    undefined,
  );

  const knownDeviceIdsRef = useRef<Set<string>>(new Set());
  const initializedKnownRef = useRef(false);

  useEffect(() => {
    if (devices) {
      knownDeviceIdsRef.current = new Set(
        devices.filter((d) => !d.revoked).map((d) => d.device_id),
      );
      initializedKnownRef.current = true;
    }
  }, [devices]);

  useEffect(() => {
    let cancelled = false;
    let inFlight = false;
    const poll = async (isInitial: boolean) => {
      if (cancelled || inFlight) return;
      inFlight = true;
      try {
        const next = await fetchDevices(config, identity);
        if (cancelled) return;
        onDevicesChange(next);
        if (
          initializedKnownRef.current &&
          binding !== undefined &&
          next.some(
            (d) => !d.revoked && !knownDeviceIdsRef.current.has(d.device_id),
          )
        ) {
          setBinding(undefined);
        }
      } catch (cause) {
        if (!cancelled && isInitial) {
          setDeviceActionError(describeApiError(cause));
        }
      } finally {
        inFlight = false;
      }
    };
    void poll(true);
    const intervalMs = binding !== undefined ? 2000 : 5000;
    const interval = setInterval(() => void poll(false), intervalMs);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [binding, config, identity, onDevicesChange]);

  async function generateBindingCode() {
    setBindingBusy(true);
    setDeviceActionError(undefined);
    setBindingCopied(false);
    try {
      setBinding(await createDeviceBinding(config, identity));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setBindingBusy(false);
    }
  }

  async function copyBindingCode() {
    if (binding === undefined) return;
    setDeviceActionError(undefined);
    const copied = await copyTextToClipboard(binding.binding_code);
    if (copied) {
      setBindingCopied(true);
    } else {
      setDeviceActionError("无法自动复制，请点按绑定码后手动复制。");
    }
  }

  function askRevoke(device: DeviceRecord) {
    if (device.revoked) return;
    setPendingRevoke(device);
  }

  async function confirmRevoke() {
    const device = pendingRevoke;
    if (device === undefined) return;
    setPendingRevoke(undefined);
    setRevokingDevice(device.device_id);
    setDeviceActionError(undefined);
    try {
      await revokeDevice(config, identity, device.device_id);
      onDevicesChange(await fetchDevices(config, identity));
    } catch (cause) {
      setDeviceActionError(describeApiError(cause));
    } finally {
      setRevokingDevice(undefined);
    }
  }

  return (
    <>
      <section className="settings-section" aria-labelledby="device-settings">
        <div className="settings-section-heading">
          <span className="settings-section-icon" aria-hidden="true">
            <DevicesIcon size={18} />
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
        {devices === undefined && (
          <p className="settings-hint">正在读取已绑定设备…</p>
        )}
        {devices !== undefined && devices.length === 0 && (
          <p className="settings-hint">
            暂无已绑定设备。请点击上方按钮生成绑定码，并在客户端中完成配对。
          </p>
        )}
        {devices !== undefined && devices.length > 0 && (
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
                    <span
                      className="mono"
                      title={`设备 ID: ${device.device_id}`}
                      style={{ fontWeight: 500 }}
                    >
                      {deviceFriendlyName(device)}
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
                  onClick={() => askRevoke(device)}
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
      <Confirm
        open={pendingRevoke !== undefined}
        title={
          pendingRevoke === undefined
            ? "撤销这台设备？"
            : `撤销 "${deviceFriendlyName(pendingRevoke)}"？`
        }
        description={
          "现有历史数据将完整保留。撤销后随时可在设备端使用新配对码重新绑定连接。"
        }
        confirmLabel="撤销设备"
        cancelLabel="保留"
        onConfirm={() => void confirmRevoke()}
        onCancel={() => setPendingRevoke(undefined)}
      />
    </>
  );
}
