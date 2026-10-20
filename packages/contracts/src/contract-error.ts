/**
 * The error every contract violation raises.
 *
 * It lives in its own module so a contract parser can raise it without
 * importing `index.ts`, which would make the two modules circular.
 */
export class ContractError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ContractError";
    this.code = code;
  }
}
