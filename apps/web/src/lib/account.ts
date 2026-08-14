export type SubscriptionPlan = "free" | "pro" | "team";
export type SubscriptionStatus =
  "trialing" | "active" | "past_due" | "canceled" | "expired";
export type WorkspaceRole = "owner" | "admin" | "member";
export type InviteRole = "admin" | "member";

export interface WorkspaceSummary {
  workspace_owner_id: string;
  name: string;
  role: WorkspaceRole;
  current: boolean;
}

export interface AccountView {
  user: {
    user_id: string;
    email: string;
    display_name: string;
    status: "active" | "suspended" | "deleted";
    email_verified_at?: string;
    created_at: string;
  };
  subscription: {
    plan: SubscriptionPlan;
    status: SubscriptionStatus;
    current_period_end?: string;
  };
  entitlements: {
    max_devices: number;
    max_concurrent_tasks: number;
    history_days: number;
    remote_tasks: boolean;
    team_members: number;
  };
  workspaces?: WorkspaceSummary[];
  workspace?: WorkspaceSummary;
  workspace_subscription?: AccountView["subscription"];
  workspace_entitlements?: AccountView["entitlements"];
}

export interface TeamMemberView {
  user_id: string;
  email: string;
  display_name: string;
  role: WorkspaceRole;
  joined_at: string;
}

export interface TeamInvitationView {
  invitation_id: string;
  email: string;
  role: InviteRole;
  created_at: string;
  expires_at: string;
}

export interface TeamWorkspaceView {
  workspace: WorkspaceSummary;
  members: TeamMemberView[];
  invitations: TeamInvitationView[];
  member_limit: number;
}

export interface WebSessionView {
  session_id: string;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  current: boolean;
}

export class AccountApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(status: number, code: string) {
    super(`account request failed: ${status} ${code}`);
    this.name = "AccountApiError";
    this.status = status;
    this.code = code;
  }
}

export async function currentAccount(): Promise<AccountView | undefined> {
  const response = await fetch("/v1/auth/me", {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (response.status === 401) return undefined;
  return accountResponse(response);
}

export async function registerAccount(input: {
  email: string;
  password: string;
  display_name: string;
}): Promise<AccountView> {
  return accountRequest("/v1/auth/register", input);
}

export async function loginAccount(input: {
  email: string;
  password: string;
}): Promise<AccountView> {
  return accountRequest("/v1/auth/login", input);
}

export async function requestPasswordReset(email: string): Promise<void> {
  await accountRequest<{ accepted: boolean }>("/v1/auth/password-resets", {
    email,
  });
}

export async function confirmPasswordReset(input: {
  token: string;
  password: string;
}): Promise<void> {
  await accountRequest<{ changed: boolean }>(
    "/v1/auth/password-resets/confirm",
    input,
  );
}

export async function changePassword(input: {
  current_password: string;
  new_password: string;
}): Promise<number> {
  const result = await accountRequest<{ changed: boolean; revoked: number }>(
    "/v1/auth/password",
    input,
  );
  return result.revoked;
}

export async function logoutAccount(): Promise<void> {
  const response = await fetch("/v1/auth/logout", {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!response.ok) throw await accountError(response);
}

export async function requestEmailVerification(): Promise<{
  accepted: boolean;
  verified: boolean;
}> {
  return accountRequest("/v1/auth/email-verifications", {});
}

export async function confirmEmailVerification(
  token: string,
): Promise<AccountView> {
  return accountRequest("/v1/auth/email-verifications/confirm", { token });
}

export async function fetchWebSessions(): Promise<WebSessionView[]> {
  const response = await fetch("/v1/auth/sessions", {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  const body = (await response.json()) as { sessions: WebSessionView[] };
  return body.sessions;
}

export async function revokeWebSession(sessionId: string): Promise<void> {
  const response = await fetch(
    `/v1/auth/sessions/${encodeURIComponent(sessionId)}`,
    {
      method: "DELETE",
      cache: "no-store",
      credentials: "same-origin",
    },
  );
  if (!response.ok) throw await accountError(response);
}

export async function revokeOtherWebSessions(): Promise<number> {
  const response = await fetch("/v1/auth/sessions", {
    method: "DELETE",
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  const body = (await response.json()) as { revoked: number };
  return body.revoked;
}

export async function fetchTeamWorkspace(): Promise<TeamWorkspaceView> {
  const response = await fetch("/v1/team", {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as TeamWorkspaceView;
}

export async function createTeamInvitation(input: {
  email: string;
  role: InviteRole;
}): Promise<{
  invitation_id: string;
  invitation_url: string;
  email: string;
  role: InviteRole;
  expires_at: string;
}> {
  return accountRequest("/v1/team/invitations", input);
}

export async function acceptTeamInvitation(token: string): Promise<void> {
  await accountRequest<{ accepted: boolean }>("/v1/team/invitations/accept", {
    token,
  });
}

export async function revokeTeamInvitation(
  invitationId: string,
): Promise<void> {
  await accountMutation(
    `/v1/team/invitations/${encodeURIComponent(invitationId)}`,
    "DELETE",
  );
}

export async function updateTeamMember(
  userId: string,
  role: InviteRole,
): Promise<void> {
  await accountMutation(
    `/v1/team/members/${encodeURIComponent(userId)}`,
    "PATCH",
    { role },
  );
}

export async function removeTeamMember(userId: string): Promise<void> {
  await accountMutation(
    `/v1/team/members/${encodeURIComponent(userId)}`,
    "DELETE",
  );
}

export async function switchWorkspace(
  workspaceOwnerId: string,
): Promise<WorkspaceSummary[]> {
  const response = await accountRequest<{ workspaces: WorkspaceSummary[] }>(
    "/v1/team/workspace",
    { workspace_owner_id: workspaceOwnerId },
  );
  return response.workspaces;
}

export function accountErrorMessage(error: unknown): string {
  if (!(error instanceof AccountApiError)) return "账户服务暂时不可用";
  const messages: Record<string, string> = {
    email_already_registered: "这个邮箱已经注册，请直接登录",
    invalid_credentials: "邮箱或密码不正确",
    invalid_email: "请输入有效的邮箱地址",
    invalid_password: "密码至少需要 10 个字符",
    invalid_current_password: "当前密码不正确",
    password_unchanged: "新密码不能与当前密码相同",
    invalid_password_reset: "重置链接无效或已经过期",
    invalid_display_name: "显示名称需要 1 到 80 个字符",
    accounts_unavailable: "服务器尚未启用账户服务",
    email_delivery_unavailable: "服务器尚未配置验证邮件服务",
    email_delivery_failed: "验证邮件发送失败，请稍后重试",
    email_verification_rate_limited: "验证邮件刚刚发送，请稍后再试",
    invalid_email_verification: "验证链接无效或已经过期",
    login_rate_limited: "登录尝试过多，请稍后再试",
    invalid_session_id: "登录会话标识无效",
    session_not_found: "登录会话不存在或已撤销",
    team_plan_required: "需要 Team 方案才能邀请成员",
    team_invitee_unavailable: "这个邮箱尚未注册 AgentPulse",
    team_member_exists: "这个用户已经在当前工作区",
    team_invitation_exists: "这个用户已有待接受邀请",
    team_member_limit_reached: "团队席位已用完",
    team_permission_denied: "当前角色不能执行这个团队操作",
    invalid_team_invitation: "团队邀请无效或已经过期",
    team_invitation_not_found: "团队邀请不存在或已撤销",
    workspace_not_found: "工作区不存在或你已不再是成员",
  };
  return messages[error.code] ?? "账户操作失败，请稍后重试";
}

async function accountRequest<T = AccountView>(
  url: string,
  body: object,
): Promise<T> {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as T;
}

async function accountMutation(
  url: string,
  method: "PATCH" | "DELETE",
  body?: object,
): Promise<void> {
  const response = await fetch(url, {
    method,
    ...(body === undefined
      ? {}
      : {
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        }),
    credentials: "same-origin",
    cache: "no-store",
  });
  if (!response.ok) throw await accountError(response);
}

async function accountResponse(response: Response): Promise<AccountView> {
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as AccountView;
}

async function accountError(response: Response): Promise<AccountApiError> {
  let code = "http_error";
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body.error === "string") code = body.error;
  } catch {
    // Keep the generic classification for non-JSON proxy failures.
  }
  return new AccountApiError(response.status, code);
}
