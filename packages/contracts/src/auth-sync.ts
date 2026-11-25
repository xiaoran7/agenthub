import { ContractError } from "./contract-error.js";
import { expectKeys, expectObject } from "./expect.js";
import { expectAgentType } from "./plugins.js";
import {
  deviceIdForPublicKey,
  verifyDeviceSignature,
  verifyEd25519,
} from "./device-sync.js";
import {
  authStates,
  credentialSources,
  quotaStates,
  type AuthSnapshot,
  type AuthSyncBatch,
  type QuotaSnapshot,
  type QuotaWindow,
} from "./sync-types.js";
import {
  expectBoolean,
  expectDeviceId,
  expectEnum,
  expectHex,
  expectInteger,
  expectProtocolVersion,
  expectString,
  expectTimestamp,
} from "./sync-validators.js";

export function parseAuthSyncBatch(value: unknown): AuthSyncBatch {
  const object = expectObject(value, "auth sync batch");
  expectKeys(object, [
    "protocol_version",
    "device_id",
    "public_key",
    "checked_at",
    "snapshots",
    "signature",
  ]);
  const snapshots = object.snapshots;
  if (
    !Array.isArray(snapshots) ||
    snapshots.length === 0 ||
    snapshots.length > 8
  ) {
    throw new ContractError(
      "invalid_auth_snapshots",
      "snapshots must contain between 1 and 8 items",
    );
  }
  const providers = snapshots.map((snapshot, index) =>
    expectAgentType(
      expectObject(snapshot, `snapshots[${index}]`).provider,
      `snapshots[${index}].provider`,
    ),
  );
  if (new Set(providers).size !== providers.length) {
    throw new ContractError(
      "duplicate_auth_provider",
      "auth batch contains duplicate providers",
    );
  }
  const batch: AuthSyncBatch = {
    protocol_version: expectInteger(
      object.protocol_version,
      "protocol_version",
      1,
    ),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    checked_at: expectTimestamp(object.checked_at, "checked_at"),
    snapshots: snapshots.map((snapshot, index) =>
      parseAuthSnapshot(snapshot, index),
    ),
    signature: expectHex(object.signature, "signature", 128),
  };
  if (batch.protocol_version !== 1) {
    throw new ContractError(
      "unsupported_protocol",
      `unsupported auth protocol version ${batch.protocol_version}`,
    );
  }
  if (deviceIdForPublicKey(batch.public_key) !== batch.device_id) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  verifyAuthSyncBatch(batch);
  return batch;
}

export function unsignedAuthSyncBatchJson(batch: AuthSyncBatch): string {
  return JSON.stringify({
    protocol_version: batch.protocol_version,
    device_id: batch.device_id,
    public_key: batch.public_key,
    checked_at: batch.checked_at,
    snapshots: batch.snapshots.map((snapshot) =>
      canonicalAuthSnapshot(snapshot),
    ),
  });
}

export function verifyAuthSyncBatch(batch: AuthSyncBatch): void {
  if (
    !verifyEd25519(
      batch.public_key,
      Buffer.from(unsignedAuthSyncBatchJson(batch), "utf8"),
      batch.signature,
    )
  ) {
    throw new ContractError(
      "invalid_signature",
      "auth sync signature is invalid",
    );
  }
}

function parseAuthSnapshot(value: unknown, index: number): AuthSnapshot {
  const object = expectObject(value, `snapshots[${index}]`);
  expectKeys(
    object,
    [
      "provider",
      "state",
      "source",
      "checked_at",
      "access_token_present",
      "refresh_token_present",
    ],
    [
      "account_hint",
      "plan_type",
      "auth_mode",
      "access_token_expires_at",
      "quota",
      "error_code",
    ],
  );
  const snapshot: AuthSnapshot = {
    provider: expectAgentType(object.provider, `snapshots[${index}].provider`),
    state: expectEnum(object.state, authStates, `snapshots[${index}].state`),
    source: expectEnum(
      object.source,
      credentialSources,
      `snapshots[${index}].source`,
    ),
    checked_at: expectTimestamp(
      object.checked_at,
      `snapshots[${index}].checked_at`,
    ),
    access_token_present: expectBoolean(
      object.access_token_present,
      `snapshots[${index}].access_token_present`,
    ),
    refresh_token_present: expectBoolean(
      object.refresh_token_present,
      `snapshots[${index}].refresh_token_present`,
    ),
  };
  if (object.account_hint !== undefined) {
    snapshot.account_hint = expectString(
      object.account_hint,
      `snapshots[${index}].account_hint`,
      256,
    );
  }
  if (object.plan_type !== undefined) {
    snapshot.plan_type = expectString(
      object.plan_type,
      `snapshots[${index}].plan_type`,
      128,
    );
  }
  if (object.auth_mode !== undefined) {
    snapshot.auth_mode = expectString(
      object.auth_mode,
      `snapshots[${index}].auth_mode`,
      128,
    );
  }
  if (object.access_token_expires_at !== undefined) {
    snapshot.access_token_expires_at = expectTimestamp(
      object.access_token_expires_at,
      `snapshots[${index}].access_token_expires_at`,
    );
  }
  if (object.quota !== undefined) {
    snapshot.quota = parseQuotaSnapshot(
      object.quota,
      `snapshots[${index}].quota`,
    );
  }
  if (object.error_code !== undefined) {
    snapshot.error_code = expectString(
      object.error_code,
      `snapshots[${index}].error_code`,
      128,
    );
  }
  return snapshot;
}

function parseQuotaSnapshot(value: unknown, name: string): QuotaSnapshot {
  const object = expectObject(value, name);
  expectKeys(
    object,
    ["state", "source", "refreshed_at", "windows"],
    ["available_reset_credits"],
  );
  if (!Array.isArray(object.windows) || object.windows.length > 16) {
    throw new ContractError(
      "invalid_field",
      `${name}.windows must contain at most 16 items`,
    );
  }
  const quota: QuotaSnapshot = {
    state: expectEnum(object.state, quotaStates, `${name}.state`),
    source: expectEnum(object.source, credentialSources, `${name}.source`),
    refreshed_at: expectTimestamp(object.refreshed_at, `${name}.refreshed_at`),
    windows: object.windows.map((window, index) =>
      parseQuotaWindow(window, `${name}.windows[${index}]`),
    ),
  };
  if (object.available_reset_credits !== undefined) {
    quota.available_reset_credits = expectInteger(
      object.available_reset_credits,
      `${name}.available_reset_credits`,
      0,
    );
  }
  return quota;
}

function parseQuotaWindow(value: unknown, name: string): QuotaWindow {
  const object = expectObject(value, name);
  expectKeys(
    object,
    ["name"],
    ["used_percent", "window_duration_minutes", "resets_at"],
  );
  const window: QuotaWindow = {
    name: expectString(object.name, `${name}.name`, 64),
  };
  if (object.used_percent !== undefined) {
    const used = expectInteger(object.used_percent, `${name}.used_percent`, 0);
    if (used > 100) {
      throw new ContractError(
        "invalid_field",
        `${name}.used_percent must be <= 100`,
      );
    }
    window.used_percent = used;
  }
  if (object.window_duration_minutes !== undefined) {
    window.window_duration_minutes = expectInteger(
      object.window_duration_minutes,
      `${name}.window_duration_minutes`,
      0,
    );
  }
  if (object.resets_at !== undefined) {
    window.resets_at = expectTimestamp(object.resets_at, `${name}.resets_at`);
  }
  return window;
}

function canonicalAuthSnapshot(snapshot: AuthSnapshot): AuthSnapshot {
  const canonical = {
    provider: snapshot.provider,
    state: snapshot.state,
    source: snapshot.source,
    checked_at: snapshot.checked_at,
    ...(snapshot.account_hint !== undefined
      ? { account_hint: snapshot.account_hint }
      : {}),
    ...(snapshot.plan_type !== undefined
      ? { plan_type: snapshot.plan_type }
      : {}),
    ...(snapshot.auth_mode !== undefined
      ? { auth_mode: snapshot.auth_mode }
      : {}),
    access_token_present: snapshot.access_token_present,
    refresh_token_present: snapshot.refresh_token_present,
    ...(snapshot.access_token_expires_at !== undefined
      ? { access_token_expires_at: snapshot.access_token_expires_at }
      : {}),
    ...(snapshot.quota !== undefined
      ? {
          quota: {
            state: snapshot.quota.state,
            source: snapshot.quota.source,
            refreshed_at: snapshot.quota.refreshed_at,
            windows: snapshot.quota.windows.map((window) => {
              const canonicalWindow: QuotaWindow = { name: window.name };
              if (window.used_percent !== undefined) {
                canonicalWindow.used_percent = window.used_percent;
              }
              if (window.window_duration_minutes !== undefined) {
                canonicalWindow.window_duration_minutes =
                  window.window_duration_minutes;
              }
              if (window.resets_at !== undefined) {
                canonicalWindow.resets_at = window.resets_at;
              }
              return canonicalWindow;
            }),
            ...(snapshot.quota.available_reset_credits !== undefined
              ? {
                  available_reset_credits:
                    snapshot.quota.available_reset_credits,
                }
              : {}),
          },
        }
      : {}),
    ...(snapshot.error_code !== undefined
      ? { error_code: snapshot.error_code }
      : {}),
  } as AuthSnapshot;
  return canonical;
}
