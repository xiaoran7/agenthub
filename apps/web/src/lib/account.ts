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
    oauth_provider?: string;
    oauth_sub?: string;
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

export type AuditScope = "account" | "workspace";
export type AuditAction =
  | "account.created"
  | "auth.session_created"
  | "auth.session_revoked"
  | "auth.other_sessions_revoked"
  | "auth.email_verification_requested"
  | "auth.email_verified"
  | "auth.password_reset_requested"
  | "auth.password_reset_completed"
  | "auth.password_changed"
  | "workspace.switched"
  | "team.invitation_created"
  | "team.invitation_revoked"
  | "team.invitation_accepted"
  | "team.member_role_updated"
  | "team.member_removed"
  | "subscription.updated"
  | "device.binding_created"
  | "device.registered"
  | "device.revoked"
  | "task.created"
  | "task.canceled"
  | "task.claimed"
  | "task.status_updated";

export interface AuditEvent {
  event_id: string;
  scope: AuditScope;
  actor_user_id?: string;
  workspace_owner_id?: string;
  action: AuditAction;
  target_type?:
    | "account"
    | "web_session"
    | "workspace"
    | "team_invitation"
    | "team_member"
    | "subscription"
    | "device"
    | "remote_task";
  target_id?: string;
  metadata: Record<string, string | number | boolean>;
  created_at: string;
}

export interface AuditPage {
  scope: AuditScope;
  events: AuditEvent[];
  next_before?: string;
}

export interface BillingStatus {
  available: boolean;
  provider?: string;
  checkout: boolean;
  portal: boolean;
  subscription: AccountView["subscription"];
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

export async function fetchAuditEvents(
  scope: AuditScope,
  before?: string,
): Promise<AuditPage> {
  const query = new URLSearchParams({ scope, limit: "30" });
  if (before !== undefined) query.set("before", before);
  const response = await fetch(`/v1/audit?${query.toString()}`, {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as AuditPage;
}

export async function fetchBillingStatus(): Promise<BillingStatus> {
  const response = await fetch("/v1/billing", {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as BillingStatus;
}

export async function createBillingCheckout(
  plan: "pro" | "team",
): Promise<string> {
  const body = await accountRequest<{ url: unknown }>("/v1/billing/checkout", {
    plan,
  });
  return billingRedirectUrl(body.url);
}

export async function openBillingPortal(): Promise<string> {
  const body = await accountRequest<{ url: unknown }>("/v1/billing/portal", {});
  return billingRedirectUrl(body.url);
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

export interface AuthStatusResponse {
  needs_setup: boolean;
}

export async function fetchAuthStatus(): Promise<AuthStatusResponse> {
  const response = await fetch("/v1/auth/status", {
    cache: "no-store",
    credentials: "same-origin",
  });
  if (!response.ok) throw await accountError(response);
  return (await response.json()) as AuthStatusResponse;
}

export async function setupAdmin(input: {
  email: string;
  password: string;
  display_name?: string;
}): Promise<AccountView> {
  return accountRequest<AccountView>("/v1/auth/setup", input);
}

export function accountErrorMessage(error: unknown): string {
  if (!(error instanceof AccountApiError)) return "账户服务暂时不可用";
  const messages: Record<string, string> = {
    admin_already_exists: "系统管理员已被初始化，请直接登录",
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
    token_exchange_failed: "OAuth Token 换取失败",
    userinfo_failed: "读取 OAuth 用户资料失败",
    oauth_unsupported: "OAuth 身份认证暂未启用",
    invalid_device_code: "设备授权码无效或已过期",
    team_plan_required: "需要 Team 方案才能邀请成员",
    team_invitee_unavailable: "这个邮箱尚未注册 AgentHub",
    team_member_exists: "这个用户已经在当前工作区",
    team_invitation_exists: "这个用户已有待接受邀请",
    team_member_limit_reached: "团队席位已用完",
    team_permission_denied: "当前角色不能执行这个团队操作",
    invalid_team_invitation: "团队邀请无效或已经过期",
    team_invitation_not_found: "团队邀请不存在或已撤销",
    workspace_not_found: "工作区不存在或你已不再是成员",
    invalid_audit_query: "审计记录查询参数无效",
    billing_unavailable: "订阅支付暂未配置",
    billing_provider_failed: "支付服务暂时不可用，请稍后重试",
    billing_portal_required: "请通过订阅管理入口调整当前方案",
    billing_customer_unavailable: "当前账户还没有可管理的订阅",
    billing_provider_mismatch: "当前订阅由其他支付服务管理",
    invalid_billing_plan: "订阅方案无效",
    invalid_billing_signature: "支付回调签名无效",
    invalid_billing_event: "支付回调内容无效",
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

function billingRedirectUrl(value: unknown): string {
  if (typeof value !== "string") throw new AccountApiError(502, "http_error");
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new AccountApiError(502, "http_error");
  }
  if (
    url.protocol !== "https:" ||
    (url.hostname !== "checkout.stripe.com" &&
      !url.hostname.endsWith(".checkout.stripe.com") &&
      url.hostname !== "billing.stripe.com" &&
      !url.hostname.endsWith(".billing.stripe.com"))
  ) {
    throw new AccountApiError(502, "http_error");
  }
  return url.toString();
}
