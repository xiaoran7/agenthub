import type { PanelConfig } from "./config";
import type { DeviceRecord, SessionRef } from "./types";

/**
 * The identity a gateway request carries.
 *
 * The signed-in account's id is the real identity; the panel config's id is
 * only the fallback for a Gateway running without accounts. Passing the
 * identity explicitly keeps every call site from silently falling back to the
 * fallback and reading another owner's devices.
 */
export interface GatewayIdentity {
  userId: string;
}

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

import type { AgentType } from "@agent-pulse/contracts";

export function deviceBindingsUrl(base: string): string {
  return `${base}/v1/device-bindings`;
}

export function deviceUrl(base: string, deviceId: string): string {
  return `${base}/v1/devices/${encodeURIComponent(deviceId)}`;
}

export function sessionsUrl(base: string, agentType?: AgentType): string {
  const query =
    agentType === undefined
      ? ""
      : `?${new URLSearchParams({ agent_type: agentType }).toString()}`;
  return `${base}/v1/sessions${query}`;
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

async function fetchJson<T>(
  url: string,
  identity: GatewayIdentity,
): Promise<T> {
  let response: Response;
  try {
    response = await fetch(url, {
      headers: { "x-user-id": identity.userId },
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
  identity: GatewayIdentity,
  method: "POST" | "DELETE",
  body: unknown,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: {
        "x-user-id": identity.userId,
        "content-type": "application/json",
      },
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

export async function fetchPushKey(
  config: PanelConfig,
  identity: GatewayIdentity,
): Promise<string> {
  const body = await fetchJson<{ public_key: string }>(
    pushKeyUrl(config.gatewayBase),
    identity,
  );
  return body.public_key;
}

export async function savePushSubscription(
  config: PanelConfig,
  identity: GatewayIdentity,
  subscription: PushSubscriptionJSON,
): Promise<void> {
  await sendJson(pushSubscriptionsUrl(config.gatewayBase), identity, "POST", {
    endpoint: subscription.endpoint,
    keys: {
      p256dh: subscription.keys?.p256dh,
      auth: subscription.keys?.auth,
    },
  });
}

export async function deletePushSubscription(
  config: PanelConfig,
  identity: GatewayIdentity,
  endpoint: string,
): Promise<void> {
  await sendJson(pushSubscriptionsUrl(config.gatewayBase), identity, "DELETE", {
    endpoint,
  });
}

export async function fetchDevices(
  config: PanelConfig,
  identity: GatewayIdentity,
): Promise<DeviceRecord[]> {
  const body = await fetchJson<{ devices: DeviceRecord[] }>(
    devicesUrl(config.gatewayBase),
    identity,
  );
  return body.devices;
}

export interface DeviceBinding {
  binding_code: string;
  expires_at: string;
}

export async function createDeviceBinding(
  config: PanelConfig,
  identity: GatewayIdentity,
): Promise<DeviceBinding> {
  let response: Response;
  try {
    response = await fetch(deviceBindingsUrl(config.gatewayBase), {
      method: "POST",
      headers: {
        "x-user-id": identity.userId,
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
  identity: GatewayIdentity,
  deviceId: string,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(deviceUrl(config.gatewayBase, deviceId), {
      method: "DELETE",
      headers: { "x-user-id": identity.userId },
      cache: "no-store",
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    throw new ApiError(response.status, await errorCode(response));
  }
}
