import type { PanelConfig } from "./config";
import type {
  AuthRecord,
  DeviceRecord,
  SessionRecord,
  SessionRef,
} from "./types";

export const DEFAULT_TIMELINE_LIMIT = 100;

export class ApiError extends Error {
  /** HTTP status; 0 means the request never reached the gateway. */
  readonly status: number;
  /** Stable gateway error code, or a client-side classification. */
  readonly code: string;

  constructor(status: number, code: string) {
    super(`gateway request failed: ${status} ${code}`);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

export function devicesUrl(base: string): string {
  return `${base}/v1/devices`;
}

export function deviceBindingsUrl(base: string): string {
  return `${base}/v1/device-bindings`;
}

export function deviceUrl(base: string, deviceId: string): string {
  return `${base}/v1/devices/${encodeURIComponent(deviceId)}`;
}

export function sessionsUrl(
  base: string,
  agentType?: "claude_code" | "codex",
): string {
  const query =
    agentType === undefined
      ? ""
      : `?${new URLSearchParams({ agent_type: agentType }).toString()}`;
  return `${base}/v1/sessions${query}`;
}

export function deviceAuthUrl(base: string): string {
  return `${base}/v1/device-auth`;
}

export function streamUrl(base: string): string {
  return `${base}/v1/stream`;
}

export function pushKeyUrl(base: string): string {
  return `${base}/v1/push/key`;
}

export function pushSubscriptionsUrl(base: string): string {
  return `${base}/v1/push/subscriptions`;
}

export function sessionEventsUrl(
  base: string,
  ref: SessionRef,
  limit: number,
): string {
  const query = new URLSearchParams({
    device_id: ref.device_id,
    agent_type: ref.agent_type,
    agent_session_id: ref.agent_session_id,
    limit: String(limit),
  });
  return `${base}/v1/sessions/events?${query.toString()}`;
}

export function describeApiError(error: unknown): string {
  if (!(error instanceof ApiError)) {
    return "未知错误";
  }
  if (error.status === 0) {
    return "无法连接网关";
  }
  if (error.code === "authentication_required") {
    return "网关访问令牌缺失或无效，请检查服务端配置";
  }
  if (error.status === 401 || error.status === 403) {
    return "网关拒绝了当前身份";
  }
  if (error.status >= 500) {
    return "网关内部错误";
  }
  return `网关返回错误（${error.status} ${error.code}）`;
}

async function fetchJson<T>(url: string, userId: string): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { "x-user-id": userId },
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
  return (await response.json()) as T;
}

async function sendJson(
  url: string,
  userId: string,
  method: "POST" | "DELETE",
  body: unknown,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: { "x-user-id": userId, "content-type": "application/json" },
      body: JSON.stringify(body),
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
}

async function errorCode(response: Response): Promise<string> {
  try {
    const body: unknown = await response.json();
    if (
      typeof body === "object" &&
      body !== null &&
      "error" in body &&
      typeof body.error === "string"
    ) {
      return body.error;
    }
  } catch {
    // Keep the generic classification when the body is not JSON.
  }
  return "http_error";
}

export async function fetchPushKey(config: PanelConfig): Promise<string> {
  const body = await fetchJson<{ public_key: string }>(
    pushKeyUrl(config.gatewayBase),
    config.userId,
  );
  return body.public_key;
}

export async function savePushSubscription(
  config: PanelConfig,
  subscription: PushSubscriptionJSON,
): Promise<void> {
  await sendJson(
    pushSubscriptionsUrl(config.gatewayBase),
    config.userId,
    "POST",
    {
      endpoint: subscription.endpoint,
      keys: {
        p256dh: subscription.keys?.p256dh,
        auth: subscription.keys?.auth,
      },
    },
  );
}

export async function deletePushSubscription(
  config: PanelConfig,
  endpoint: string,
): Promise<void> {
  await sendJson(
    pushSubscriptionsUrl(config.gatewayBase),
    config.userId,
    "DELETE",
    { endpoint },
  );
}

export async function fetchDevices(
  config: PanelConfig,
): Promise<DeviceRecord[]> {
  const body = await fetchJson<{ devices: DeviceRecord[] }>(
    devicesUrl(config.gatewayBase),
    config.userId,
  );
  return body.devices;
}

export interface DeviceBinding {
  binding_code: string;
  expires_at: string;
}

export async function createDeviceBinding(
  config: PanelConfig,
): Promise<DeviceBinding> {
  let response: Response;
  try {
    response = await fetch(deviceBindingsUrl(config.gatewayBase), {
      method: "POST",
      headers: {
        "x-user-id": config.userId,
        "content-type": "application/json",
      },
      body: JSON.stringify({ expires_in_seconds: 600 }),
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
  return (await response.json()) as DeviceBinding;
}

export async function revokeDevice(
  config: PanelConfig,
  deviceId: string,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(deviceUrl(config.gatewayBase, deviceId), {
      method: "DELETE",
      headers: { "x-user-id": config.userId },
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
}

export async function fetchSessions(
  config: PanelConfig,
  agentType?: "claude_code" | "codex",
): Promise<SessionRecord[]> {
  const body = await fetchJson<{ sessions: SessionRecord[] }>(
    sessionsUrl(config.gatewayBase, agentType),
    config.userId,
  );
  return body.sessions;
}

export async function fetchDeviceAuth(
  config: PanelConfig,
): Promise<AuthRecord[]> {
  const body = await fetchJson<{ snapshots: AuthRecord[] }>(
    deviceAuthUrl(config.gatewayBase),
    config.userId,
  );
  return body.snapshots;
}

export async function fetchSessionEvents(
  config: PanelConfig,
  ref: SessionRef,
  limit: number = DEFAULT_TIMELINE_LIMIT,
): Promise<SessionRecord[]> {
  const body = await fetchJson<{ events: SessionRecord[] }>(
    sessionEventsUrl(config.gatewayBase, ref, limit),
    config.userId,
  );
  return body.events;
}

async function sendJsonResponse<T>(
  url: string,
  userId: string,
  method: "POST" | "DELETE",
  body: unknown,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: { "x-user-id": userId, "content-type": "application/json" },
      body: JSON.stringify(body),
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
  return (await response.json()) as T;
}
