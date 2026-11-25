import { ContractError } from "./contract-error.js";
import { SYNC_PROTOCOL_VERSION } from "./sync-types.js";

export function expectString(
  value: unknown,
  name: string,
  maxLength: number,
): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > maxLength ||
    /[\u0000-\u001f\u007f]/.test(value) ||
    // Lone surrogates cannot exist in the Rust runner's UTF-8 strings and are
    // rejected by PostgreSQL jsonb; refusing them keeps both stores and both
    // languages inside the same contract.
    !value.isWellFormed()
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a non-empty string up to ${maxLength} characters`,
    );
  }
  return value;
}

export function expectInteger(
  value: unknown,
  name: string,
  minimum: number,
): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < minimum
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a safe integer >= ${minimum}`,
    );
  }
  return value;
}

export function expectProtocolVersion(value: unknown): number {
  const version = expectInteger(value, "protocol_version", 1);
  if (version !== SYNC_PROTOCOL_VERSION) {
    throw new ContractError(
      "unsupported_protocol",
      `unsupported protocol version ${version}`,
    );
  }
  return version;
}

export function expectIdempotencyKey(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length < 8 ||
    value.length > 128 ||
    !/^[A-Za-z0-9][A-Za-z0-9._:-]+$/.test(value)
  ) {
    throw new ContractError("invalid_field", "idempotency_key is invalid");
  }
  return value;
}

export function expectSafeCode(value: unknown, name: string): string {
  if (
    typeof value !== "string" ||
    value.length > 128 ||
    !/^[a-z][a-z0-9_]*$/.test(value)
  ) {
    throw new ContractError("invalid_field", `${name} is invalid`);
  }
  return value;
}

export function expectBoolean(value: unknown, name: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError("invalid_field", `${name} must be a boolean`);
  }
  return value;
}

export function expectSchemaVersion(value: unknown, name: string): number {
  const version = expectInteger(value, name, 1);
  if (version !== 1) {
    throw new ContractError(
      "unsupported_schema",
      `unsupported event schema version ${version}`,
    );
  }
  return version;
}

export function expectEnum<const T extends readonly string[]>(
  value: unknown,
  allowed: T,
  name: string,
): T[number] {
  if (typeof value !== "string" || !allowed.includes(value)) {
    throw new ContractError("invalid_field", `${name} is invalid`);
  }
  return value as T[number];
}

export function expectHex(
  value: unknown,
  name: string,
  length: number,
): string {
  if (
    typeof value !== "string" ||
    value.length !== length ||
    !/^[0-9a-f]+$/.test(value)
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be ${length} lowercase hex characters`,
    );
  }
  return value;
}

// Version 1 signing bytes sort map keys by Unicode code point (Rust BTreeMap
// order). String comparison must not be locale-sensitive, and detail keys are
// restricted to a charset where UTF-16 code-unit order equals code-point order.
export function compareCodeUnits(left: string, right: string): number {
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

export function expectDetailKey(value: unknown): string {
  if (typeof value !== "string" || !/^[a-z][a-z0-9_]{0,63}$/.test(value)) {
    throw new ContractError(
      "invalid_field",
      "raw status detail keys must be snake_case ASCII up to 64 characters",
    );
  }
  return value;
}

export function expectDeviceId(value: unknown): string {
  if (typeof value !== "string" || !/^dev_[0-9a-f]{32}$/.test(value)) {
    throw new ContractError("invalid_field", "device_id is invalid");
  }
  return value;
}

export function expectUuid(value: unknown, name: string): string {
  // Lowercase only: PostgreSQL normalizes uuid columns to lowercase, so an
  // uppercase variant would break replay idempotency between parse and store.
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a lowercase UUID`,
    );
  }
  return value;
}

const RFC3339_TIMESTAMP_RE =
  /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(?:Z|([+-])(\d{2}):(\d{2}))$/;

export function expectTimestamp(value: unknown, name: string): string {
  if (typeof value !== "string" || value.length > 64) {
    throw new ContractError(
      "invalid_field",
      `${name} must be an RFC 3339 time`,
    );
  }
  const match = RFC3339_TIMESTAMP_RE.exec(value);
  if (match === null) {
    throw new ContractError(
      "invalid_field",
      `${name} must be an RFC 3339 time`,
    );
  }
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hour = Number(match[4]);
  const minute = Number(match[5]);
  const second = Number(match[6]);
  const offsetHour = match[8] === undefined ? 0 : Number(match[8]);
  const offsetMinute = match[9] === undefined ? 0 : Number(match[9]);
  const utcDate = new Date(0);
  utcDate.setUTCFullYear(year, month - 1, day);
  if (
    year < 1 ||
    month < 1 ||
    month > 12 ||
    day < 1 ||
    utcDate.getUTCFullYear() !== year ||
    utcDate.getUTCMonth() !== month - 1 ||
    utcDate.getUTCDate() !== day ||
    hour > 23 ||
    minute > 59 ||
    second > 59 ||
    offsetHour > 23 ||
    offsetMinute > 59 ||
    Number.isNaN(Date.parse(value))
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be an RFC 3339 time`,
    );
  }
  return value;
}
