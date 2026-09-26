/**
 * Agent CLIs report a failed login as ordinary assistant text rather than as a
 * protocol error, so the "Not logged in · Please run /login" line arrives in the
 * message stream and would otherwise be rendered as a real reply. Detect it so
 * the turn can be surfaced as an error instead.
 */
const AUTH_FAILURE_PATTERNS = [
  /not logged in/i,
  /please run \/login/i,
  /authentication_failed/i,
  /\blogin (?:required|expired)\b/i,
  /请先登录/,
];

export function isAuthFailure(text: string | undefined | null): boolean {
  if (!text) return false;
  const trimmed = text.trim();
  if (trimmed === "") return false;
  return AUTH_FAILURE_PATTERNS.some((pattern) => pattern.test(trimmed));
}

/** The exact fix command to show alongside the error. */
export const AUTH_FIX_HINT =
  "该 Agent CLI 未登录或凭据已失效。请在本机终端完成登录后重试（Claude Code 执行 /login，Codex 执行 codex login）。";
