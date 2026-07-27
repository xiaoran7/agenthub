import { deviceShortId, formatRelative } from "../lib/format";
import type { DeviceRecord } from "../lib/types";

interface DeviceStripProps {
  devices: DeviceRecord[];
  selected: string | undefined;
  nowMs: number;
  onSelect: (deviceId: string | undefined) => void;
}

export function DeviceStrip({
  devices,
  selected,
  nowMs,
  onSelect,
}: DeviceStripProps) {
  return (
    <div className="device-strip" aria-label="设备筛选">
      <button
        type="button"
        className="device-chip"
        data-selected={selected === undefined}
        onClick={() => onSelect(undefined)}
      >
        全部设备
      </button>
      {devices.map((device) => (
        <button
          type="button"
          key={device.device_id}
          className="device-chip"
          data-selected={selected === device.device_id}
          title={device.device_id}
          onClick={() =>
            onSelect(
              selected === device.device_id ? undefined : device.device_id,
            )
          }
        >
          <span className="device-dot" data-online={device.online} />
          <span className="mono">{deviceShortId(device.device_id)}</span>
          <span className="device-seen">
            {device.online
              ? "在线"
              : formatRelative(device.last_seen_at, nowMs)}
          </span>
        </button>
      ))}
    </div>
  );
}
