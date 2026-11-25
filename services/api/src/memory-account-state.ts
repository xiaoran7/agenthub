import { randomUUID } from "node:crypto";

import { auditMetadata } from "./account-helpers.js";
import { accountView } from "./account-auth.js";
import type {
  AccountUser,
  AccountView,
  AuditEvent,
  InviteRole,
  SubscriptionEvent,
  SubscriptionRecord,
  WorkspaceRole,
} from "./account-types.js";

export class MemoryAccountState {
  readonly users = new Map<string, AccountUser>();
  readonly userIdsByEmail = new Map<string, string>();
  readonly subscriptions = new Map<string, SubscriptionRecord>();
  readonly sessions = new Map<
    string,
    {
      sessionId: string;
      userId: string;
      workspaceOwnerId: string;
      createdAt: string;
      lastSeenAt: string;
      expiresAt: string;
    }
  >();
  readonly emailVerifications = new Map<
    string,
    {
      userId: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
    }
  >();
  readonly loginLimits = new Map<
    string,
    { attemptCount: number; windowStartedAt: string; blockedUntil?: string }
  >();
  readonly passwordResets = new Map<
    string,
    {
      userId: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
    }
  >();
  readonly subscriptionEvents = new Map<
    string,
    { event: SubscriptionEvent; applied: boolean }
  >();
  readonly workspaceMembers = new Map<
    string,
    Map<string, { role: WorkspaceRole; createdAt: string }>
  >();
  readonly teamInvitations = new Map<
    string,
    {
      invitationId: string;
      workspaceOwnerId: string;
      invitedUserId: string;
      invitedEmail: string;
      role: InviteRole;
      invitedBy: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
      revokedAt?: string;
    }
  >();
  readonly auditEvents: AuditEvent[] = [];
  onMemberRemoved?: (
    workspaceOwnerId: string,
    memberUserId: string,
  ) => void | Promise<void>;

  recordAudit(event: Omit<AuditEvent, "event_id">): void {
    this.auditEvents.push({
      ...event,
      event_id: randomUUID(),
      metadata: auditMetadata(event.metadata),
    });
  }

  async accountView(userId: string): Promise<AccountView | undefined> {
    const user = this.users.get(userId);
    const subscription = this.subscriptions.get(userId);
    if (
      user === undefined ||
      subscription === undefined ||
      user.status !== "active"
    ) {
      return undefined;
    }
    return accountView(user, subscription);
  }
}
