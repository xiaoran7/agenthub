export const subscriptionPlans = ["free", "pro", "team"] as const;
export const subscriptionStatuses = [
  "trialing",
  "active",
  "past_due",
  "canceled",
  "expired",
] as const;

export type SubscriptionPlan = (typeof subscriptionPlans)[number];
export type SubscriptionStatus = (typeof subscriptionStatuses)[number];

export interface AccountUser {
  user_id: string;
  email: string;
  display_name: string;
  password_hash: string;
  status: "active" | "suspended" | "deleted";
  oauth_provider?: string;
  oauth_sub?: string;
  email_verified_at?: string;
  /** The user's own picture, as a data URL. */
  avatar_data_url?: string;
  created_at: string;
}

export interface SubscriptionRecord {
  plan: SubscriptionPlan;
  status: SubscriptionStatus;
  provider?: string;
  provider_customer_id?: string;
  provider_subscription_id?: string;
  current_period_end?: string;
  last_event_at?: string;
  last_event_id?: string;
}

export interface WebSessionView {
  session_id: string;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  current: boolean;
}

export type WorkspaceRole = "owner" | "admin" | "member";
export type InviteRole = Exclude<WorkspaceRole, "owner">;

export interface SessionIdentity {
  actor_user_id: string;
  workspace_owner_id: string;
  role: WorkspaceRole;
}

export interface WorkspaceSummary {
  workspace_owner_id: string;
  name: string;
  role: WorkspaceRole;
  current: boolean;
}

export interface WorkspaceMemberView {
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
  members: WorkspaceMemberView[];
  invitations: TeamInvitationView[];
  member_limit: number;
}

export interface SubscriptionEvent {
  event_id: string;
  user_id: string;
  provider: string;
  provider_customer_id: string;
  provider_subscription_id: string;
  plan: SubscriptionPlan;
  status: SubscriptionStatus;
  current_period_end?: string;
  occurred_at: string;
}

export interface SubscriptionEventResult {
  applied: boolean;
  duplicate: boolean;
  account: AccountView;
}

export interface BillingReference {
  provider: string;
  customer_id: string;
  subscription_id?: string;
}

export interface LoginAttemptResult {
  allowed: boolean;
  retry_after_seconds: number;
}

export interface PasswordChangeResult {
  updated: boolean;
  revoked_sessions: number;
}

export const auditActions = [
  "account.created",
  "auth.session_created",
  "auth.session_revoked",
  "auth.other_sessions_revoked",
  "auth.email_verification_requested",
  "auth.email_verified",
  "auth.password_reset_requested",
  "auth.password_reset_completed",
  "auth.password_changed",
  "workspace.switched",
  "account.avatar_updated",
  "team.invitation_created",
  "team.invitation_revoked",
  "team.invitation_accepted",
  "team.member_role_updated",
  "team.member_removed",
  "subscription.updated",
  "device.binding_created",
  "device.registered",
  "device.reactivated",
  "device.revoked",
  "task.created",
  "task.canceled",
  "task.claimed",
  "task.status_updated",
] as const;

export type AuditAction = (typeof auditActions)[number];
export type AuditScope = "account" | "workspace";
export type AuditTargetType =
  | "account"
  | "web_session"
  | "workspace"
  | "team_invitation"
  | "team_member"
  | "subscription"
  | "device"
  | "remote_task";
export type AuditMetadataValue = string | number | boolean;

export interface AuditEvent {
  event_id: string;
  scope: AuditScope;
  actor_user_id?: string;
  workspace_owner_id?: string;
  action: AuditAction;
  target_type?: AuditTargetType;
  target_id?: string;
  metadata: Record<string, AuditMetadataValue>;
  created_at: string;
}

export interface AuditQuery {
  scope: AuditScope;
  actorUserId: string;
  workspaceOwnerId: string;
  beforeEventId?: string;
  limit: number;
}

export interface AuditRecordInput {
  scope: AuditScope;
  accountUserId?: string | undefined;
  workspaceOwnerId?: string | undefined;
  actorUserId?: string | undefined;
  action: AuditAction;
  targetType?: AuditTargetType | undefined;
  targetId?: string | undefined;
  metadata: Record<string, AuditMetadataValue>;
  createdAt?: string | undefined;
}

export interface Entitlements {
  max_devices: number;
  max_concurrent_tasks: number;
  history_days: number;
  remote_tasks: boolean;
  team_members: number;
}

export interface AccountView {
  user: Omit<AccountUser, "password_hash">;
  subscription: Pick<
    SubscriptionRecord,
    "plan" | "status" | "current_period_end"
  >;
  entitlements: Entitlements;
}

export interface AccountStore {
  close?(): void | Promise<void>;
  hasAnyAdmin?(): Promise<boolean>;
  createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
    singleAdmin?: boolean;
  }): Promise<AccountView>;
  findAccountByEmail(email: string): Promise<AccountUser | undefined>;
  findAccountByOAuth?(
    provider: string,
    sub: string,
  ): Promise<AccountUser | undefined>;
  upsertOAuthAccount?(input: {
    provider: string;
    sub: string;
    email: string;
    displayName: string;
    emailVerified?: boolean;
    now?: string;
  }): Promise<AccountView>;
  accountView(userId: string): Promise<AccountView | undefined>;
  /** Replace or clear the user's own picture. */
  setAvatar(
    userId: string,
    avatarDataUrl: string | undefined,
    updatedAt?: string,
  ): Promise<boolean>;
  createWebSession(input: {
    sessionId: string;
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    reason?: "registration" | "login";
  }): Promise<void>;
  findWebSession(
    digest: string,
    now: string,
  ): Promise<SessionIdentity | undefined>;
  deleteWebSession(digest: string): Promise<void>;
  listWebSessions(
    userId: string,
    currentDigest: string,
    now: string,
  ): Promise<WebSessionView[]>;
  revokeWebSession(userId: string, sessionId: string): Promise<boolean>;
  revokeOtherWebSessions(
    userId: string,
    currentDigest: string,
  ): Promise<number>;
  issueEmailVerification(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult>;
  consumeEmailVerification(
    digest: string,
    now: string,
  ): Promise<AccountView | undefined>;
  deletePendingEmailVerification(digest: string): Promise<void>;
  issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult>;
  consumePasswordReset(input: {
    digest: string;
    passwordHash: string;
    now: string;
  }): Promise<PasswordChangeResult>;
  deletePendingPasswordReset(digest: string): Promise<void>;
  changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<PasswordChangeResult>;
  consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult>;
  clearLoginAttempts(scopeDigest: string): Promise<void>;
  listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]>;
  workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined>;
  issueTeamInvitation(input: {
    invitationId: string;
    digest: string;
    workspaceOwnerId: string;
    invitedUserId: string;
    invitedEmail: string;
    role: InviteRole;
    invitedBy: string;
    createdAt: string;
    expiresAt: string;
    memberLimit: number;
  }): Promise<void>;
  consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined>;
  revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean>;
  updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean>;
  switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined>;
  applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult>;
  billingReference(userId: string): Promise<BillingReference | undefined>;
  recordAuditEvent(input: AuditRecordInput): Promise<boolean>;
  listAuditEvents(input: AuditQuery): Promise<AuditEvent[]>;
  setMemberRemovedHook?(
    hook: (
      workspaceOwnerId: string,
      memberUserId: string,
    ) => void | Promise<void>,
  ): void;
}
