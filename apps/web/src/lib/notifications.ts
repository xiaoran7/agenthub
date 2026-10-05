import {
  deletePushSubscription,
  fetchPushKey,
  savePushSubscription,
} from "./api";
import type { PanelConfig } from "./config";

export const SERVICE_WORKER_URL = "/sw.js";

export type NotificationState =
  /** Service worker or Push API missing, or the page is not a secure context. */
  | "unavailable"
  /** The gateway has no VAPID keys, so nothing can be delivered. */
  | "disabled"
  /** The browser permanently refused notifications for this origin. */
  | "denied"
  | "on"
  | "off";

export function parseNotificationPresentedAt(
  value: unknown,
): number | undefined {
  if (
    typeof value !== "object" ||
    value === null ||
    (value as { type?: unknown }).type !== "notification-presented"
  ) {
    return undefined;
  }
  const presentedAt = (value as { presented_at_ms?: unknown }).presented_at_ms;
  return typeof presentedAt === "number" &&
    Number.isSafeInteger(presentedAt) &&
    presentedAt > 0
    ? presentedAt
    : undefined;
}

/**
 * Push requires a secure context. `localhost` counts as one, so development
 * over plain HTTP still works, but a LAN IP over HTTP does not.
 */
function isPushCapable(): boolean {
  return (
    typeof window !== "undefined" &&
    window.isSecureContext &&
    "serviceWorker" in navigator &&
    "PushManager" in window &&
    "Notification" in window
  );
}

/**
 * The application server key must be raw bytes; browsers reject the base64url
 * string the gateway serves.
 */
export function decodeApplicationServerKey(
  value: string,
): Uint8Array<ArrayBuffer> {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new Error("应用服务器公钥格式无效");
  }
  const base64 = value.replaceAll("-", "+").replaceAll("_", "/");
  const binary = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, "="));
  // Explicitly ArrayBuffer-backed: PushManager rejects a SharedArrayBuffer view.
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

export async function readNotificationState(
  config: PanelConfig,
): Promise<NotificationState> {
  if (!isPushCapable()) {
    return "unavailable";
  }
  if (Notification.permission === "denied") {
    return "denied";
  }
  try {
    await fetchPushKey(config);
  } catch {
    return "disabled";
  }
  const registration =
    await navigator.serviceWorker.getRegistration(SERVICE_WORKER_URL);
  const subscription = await registration?.pushManager.getSubscription();
  return subscription === undefined || subscription === null ? "off" : "on";
}

export async function enableNotifications(
  config: PanelConfig,
): Promise<NotificationState> {
  if (!isPushCapable()) {
    return "unavailable";
  }
  const applicationServerKey = decodeApplicationServerKey(
    await fetchPushKey(config),
  );
  // Ask for permission only once the gateway confirmed it can actually send:
  // a permission prompt that leads to no notifications burns the one chance
  // the browser gives us.
  const permission = await Notification.requestPermission();
  if (permission !== "granted") {
    return permission === "denied" ? "denied" : "off";
  }
  const registration =
    await navigator.serviceWorker.register(SERVICE_WORKER_URL);
  await navigator.serviceWorker.ready;
  const existing = await registration.pushManager.getSubscription();
  const subscription =
    existing ??
    (await registration.pushManager.subscribe({
      // Required by Chrome, and honest: every push we send shows a notification.
      userVisibleOnly: true,
      applicationServerKey,
    }));
  try {
    await savePushSubscription(config, subscription.toJSON());
  } catch (error) {
    // Never leave a browser subscribed to a gateway that does not know about
    // it: that subscription would sit dead until the user cleared site data.
    if (existing === null) {
      await subscription.unsubscribe();
    }
    throw error;
  }
  return "on";
}

export async function disableNotifications(
  config: PanelConfig,
): Promise<NotificationState> {
  if (!isPushCapable()) {
    return "unavailable";
  }
  const registration =
    await navigator.serviceWorker.getRegistration(SERVICE_WORKER_URL);
  const subscription = await registration?.pushManager.getSubscription();
  if (subscription != null) {
    // Drop the server record first; if unsubscribing fails afterwards the
    // gateway simply has nothing to send to.
    await deletePushSubscription(config, subscription.endpoint);
    await subscription.unsubscribe();
  }
  return "off";
}
