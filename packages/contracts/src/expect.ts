/**
 * Shared field-level checks for the contract parsers.
 *
 * They live apart from `index.ts` so a contract module can use them without
 * importing `index.ts`, which would make the two modules circular.
 */
import { ContractError } from "./contract-error.js";

export function expectObject(
  value: unknown,
  name: string,
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError("invalid_field", `${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

export function expectKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = [],
): void {
  for (const key of required) {
    if (!(key in value)) {
      throw new ContractError("missing_field", `missing field ${key}`);
    }
  }
  const allowed = new Set([...required, ...optional]);
  const unknown = Object.keys(value).find((key) => !allowed.has(key));
  if (unknown !== undefined) {
    throw new ContractError("unknown_field", `unknown field ${unknown}`);
  }
}
