import { ContractError } from "@agent-pulse/contracts";
import type {
  AuditEvent,
  AuditMetadataValue,
  WorkspaceRole,
} from "./account-types.js";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/u;

export class AccountError extends ContractError {
  readonly statusCode: number;

  constructor(code: string, message: string, statusCode: number) {
    super(code, message);
    this.name = "AccountError";
    this.statusCode = statusCode;
  }
}

export function normalizeEmail(value: unknown): string {
  if (typeof value !== "string") {
    throw new AccountError(
      "invalid_email",
      "邮箱地址格式无效，请输入正确的邮箱",
      400,
    );
  }
  const normalized = value.trim().toLowerCase();
  if (
    normalized.length < 3 ||
    normalized.length > 254 ||
    !EMAIL_PATTERN.test(normalized)
  ) {
    throw new AccountError(
      "invalid_email",
      "邮箱地址格式无效，请输入正确的邮箱",
      400,
    );
  }
  return normalized;
}

export function normalizeDisplayName(value: unknown, email: string): string {
  if (value === undefined || value === null) {
    const defaultName = email.slice(0, email.indexOf("@")).trim();
    return defaultName || "管理员";
  }
  if (typeof value !== "string") {
    throw new AccountError("invalid_display_name", "显示名称格式无效", 400);
  }
  const normalized = value.trim();
  if (normalized.length === 0) {
    const defaultName = email.slice(0, email.indexOf("@")).trim();
    return defaultName || "管理员";
  }
  if (normalized.length > 80 || /[\u0000-\u001f\u007f]/u.test(normalized)) {
    throw new AccountError(
      "invalid_display_name",
      "显示名称长度不能超过 80 个字符或包含控制字符",
      400,
    );
  }
  return normalized;
}

export function validatePassword(value: unknown): string {
  if (typeof value !== "string") {
    throw new AccountError("invalid_password", "密码格式无效", 400);
  }
  const bytes = Buffer.byteLength(value, "utf8");
  if (value.length < 10) {
    throw new AccountError(
      "invalid_password",
      "管理员密码长度至少需要 10 个字符",
      400,
    );
  }
  if (value.length > 128 || bytes > 512) {
    throw new AccountError(
      "invalid_password",
      "密码长度不能超过 128 个字符",
      400,
    );
  }
  return value;
}

export function roleOrder(role: WorkspaceRole): number {
  return role === "owner" ? 0 : role === "admin" ? 1 : 2;
}

const AUDIT_METADATA_KEYS = new Set([
  "reason",
  "plan",
  "status",
  "provider",
  "role",
  "previous_role",
  "invited_user_id",
  "invitation_id",
  "revoked_sessions",
  "agent_type",
  "task_status",
  "previous_status",
  "removed",
]);

export function auditMetadata(
  input: Record<string, AuditMetadataValue>,
): Record<string, AuditMetadataValue> {
  const output: Record<string, AuditMetadataValue> = {};
  for (const [key, value] of Object.entries(input)) {
    if (!AUDIT_METADATA_KEYS.has(key)) {
      throw new Error(`audit metadata key is not allowed: ${key}`);
    }
    if (
      (typeof value === "string" &&
        (value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value))) ||
      (typeof value === "number" && (!Number.isSafeInteger(value) || value < 0))
    ) {
      throw new Error(`audit metadata value is invalid: ${key}`);
    }
    output[key] = value;
  }
  return output;
}

export function compareAuditEvents(
  left: AuditEvent,
  right: AuditEvent,
): number {
  const timeOrder = right.created_at.localeCompare(left.created_at);
  return timeOrder === 0
    ? right.event_id.localeCompare(left.event_id)
    : timeOrder;
}

export function cloneAuditEvent(event: AuditEvent): AuditEvent {
  return { ...event, metadata: { ...event.metadata } };
}
