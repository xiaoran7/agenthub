import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import {
  AccountError,
  cloneAuditEvent,
  compareAuditEvents,
  normalizeEmail,
  roleOrder,
} from "./account-helpers.js";
import {
  entitlementsFor,
  isNewerSubscriptionEvent,
  subscriptionFromEvent,
} from "./account-auth.js";
import type {
  AuditEvent,
  AuditQuery,
  AuditRecordInput,
  BillingReference,
  InviteRole,
  SessionIdentity,
  SubscriptionEvent,
  SubscriptionEventResult,
  TeamInvitationView,
  TeamWorkspaceView,
  WorkspaceMemberView,
  WorkspaceRole,
  WorkspaceSummary,
} from "./account-types.js";
import type { MemoryAccountState } from "./memory-account-state.js";

export class MemoryAccountWorkspaceStore {
  readonly #state: MemoryAccountState;

  constructor(state: MemoryAccountState) {
    this.#state = state;
  }

  async listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]> {
    const result: WorkspaceSummary[] = [];
    for (const [ownerId, members] of this.#state.workspaceMembers) {
      const membership = members.get(userId);
      const owner = this.#state.users.get(ownerId);
      if (membership === undefined || owner?.status !== "active") continue;
      result.push({
        workspace_owner_id: ownerId,
        name:
          ownerId === userId ? "个人工作区" : `${owner.display_name} 的工作区`,
        role: membership.role,
        current: ownerId === currentOwnerId,
      });
    }
    return result.sort((left, right) => {
      if (left.workspace_owner_id === userId) return -1;
      if (right.workspace_owner_id === userId) return 1;
      return left.name.localeCompare(right.name);
    });
  }

  async workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined> {
    const members = this.#state.workspaceMembers.get(workspaceOwnerId);
    const actor = members?.get(actorUserId);
    const owner = this.#state.users.get(workspaceOwnerId);
    if (
      members === undefined ||
      actor === undefined ||
      owner?.status !== "active"
    ) {
      return undefined;
    }
    const canManage = actor.role === "owner" || actor.role === "admin";
    return {
      workspace: {
        workspace_owner_id: workspaceOwnerId,
        name:
          workspaceOwnerId === actorUserId
            ? "个人工作区"
            : `${owner.display_name} 的工作区`,
        role: actor.role,
        current: true,
      },
      members: [...members.entries()]
        .map(([userId, membership]) => {
          const user = this.#state.users.get(userId);
          return user === undefined
            ? undefined
            : {
                user_id: userId,
                email: user.email,
                display_name: user.display_name,
                role: membership.role,
                joined_at: membership.createdAt,
              };
        })
        .filter((member): member is WorkspaceMemberView => member !== undefined)
        .sort((left, right) => roleOrder(left.role) - roleOrder(right.role)),
      invitations: canManage
        ? [...this.#state.teamInvitations.values()]
            .filter(
              (invitation) =>
                invitation.workspaceOwnerId === workspaceOwnerId &&
                invitation.consumedAt === undefined &&
                invitation.revokedAt === undefined &&
                invitation.expiresAt > now,
            )
            .map((invitation) => ({
              invitation_id: invitation.invitationId,
              email: invitation.invitedEmail,
              role: invitation.role,
              created_at: invitation.createdAt,
              expires_at: invitation.expiresAt,
            }))
        : [],
      member_limit: memberLimit,
    };
  }

  async issueTeamInvitation(input: {
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
  }): Promise<void> {
    const members = this.#state.workspaceMembers.get(input.workspaceOwnerId);
    const actorRole = members?.get(input.invitedBy)?.role;
    if (
      members === undefined ||
      (actorRole !== "owner" && actorRole !== "admin") ||
      (actorRole === "admin" && input.role === "admin")
    ) {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
    if (members.has(input.invitedUserId)) {
      throw new AccountError(
        "team_member_exists",
        "user is already a member",
        409,
      );
    }
    const pending = [...this.#state.teamInvitations.values()].filter(
      (invitation) =>
        invitation.workspaceOwnerId === input.workspaceOwnerId &&
        invitation.consumedAt === undefined &&
        invitation.revokedAt === undefined &&
        invitation.expiresAt > input.createdAt,
    );
    if (
      pending.some(
        (invitation) => invitation.invitedUserId === input.invitedUserId,
      )
    ) {
      throw new AccountError(
        "team_invitation_exists",
        "invitation already exists",
        409,
      );
    }
    if (members.size + pending.length >= input.memberLimit) {
      throw new AccountError(
        "team_member_limit_reached",
        "team member limit reached",
        403,
      );
    }
    this.#state.teamInvitations.set(input.digest, {
      invitationId: input.invitationId,
      workspaceOwnerId: input.workspaceOwnerId,
      invitedUserId: input.invitedUserId,
      invitedEmail: input.invitedEmail,
      role: input.role,
      invitedBy: input.invitedBy,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#state.recordAudit({
      scope: "workspace",
      workspace_owner_id: input.workspaceOwnerId,
      actor_user_id: input.invitedBy,
      action: "team.invitation_created",
      target_type: "team_invitation",
      target_id: input.invitationId,
      metadata: {
        role: input.role,
        invited_user_id: input.invitedUserId,
      },
      created_at: input.createdAt,
    });
  }

  async consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined> {
    const invitation = this.#state.teamInvitations.get(input.digest);
    if (
      invitation === undefined ||
      invitation.invitedUserId !== input.userId ||
      invitation.consumedAt !== undefined ||
      invitation.revokedAt !== undefined ||
      invitation.expiresAt <= input.now
    ) {
      return undefined;
    }
    const members = this.#state.workspaceMembers.get(
      invitation.workspaceOwnerId,
    );
    const subscription = this.#state.subscriptions.get(
      invitation.workspaceOwnerId,
    );
    const memberLimit =
      subscription === undefined
        ? 0
        : entitlementsFor(subscription).team_members;
    if (members === undefined || members.size >= memberLimit) {
      throw new AccountError(
        "team_member_limit_reached",
        "team member limit reached",
        403,
      );
    }
    invitation.consumedAt = input.now;
    members.set(input.userId, {
      role: invitation.role,
      createdAt: input.now,
    });
    this.#state.recordAudit({
      scope: "workspace",
      workspace_owner_id: invitation.workspaceOwnerId,
      actor_user_id: input.userId,
      action: "team.invitation_accepted",
      target_type: "team_member",
      target_id: input.userId,
      metadata: {
        role: invitation.role,
        invitation_id: invitation.invitationId,
      },
      created_at: input.now,
    });
    return invitation.workspaceOwnerId;
  }

  async revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean> {
    const actorRole = this.#state.workspaceMembers
      .get(workspaceOwnerId)
      ?.get(actorUserId)?.role;
    for (const invitation of this.#state.teamInvitations.values()) {
      if (
        invitation.invitationId === invitationId &&
        invitation.workspaceOwnerId === workspaceOwnerId &&
        invitation.consumedAt === undefined &&
        invitation.revokedAt === undefined &&
        (actorRole === "owner" ||
          (actorRole === "admin" && invitation.role === "member"))
      ) {
        invitation.revokedAt = now;
        this.#state.recordAudit({
          scope: "workspace",
          workspace_owner_id: workspaceOwnerId,
          actor_user_id: actorUserId,
          action: "team.invitation_revoked",
          target_type: "team_invitation",
          target_id: invitationId,
          metadata: { role: invitation.role },
          created_at: now,
        });
        return true;
      }
    }
    return false;
  }

  async updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean> {
    const members = this.#state.workspaceMembers.get(input.workspaceOwnerId);
    const actorRole = members?.get(input.actorUserId)?.role;
    const target = members?.get(input.memberUserId);
    if (
      members === undefined ||
      target === undefined ||
      target.role === "owner" ||
      (actorRole !== "owner" && actorRole !== "admin") ||
      (actorRole === "admin" &&
        (target.role !== "member" || input.role === "admin"))
    ) {
      return false;
    }
    if (input.remove) {
      members.delete(input.memberUserId);
      for (const session of this.#state.sessions.values()) {
        if (
          session.userId === input.memberUserId &&
          session.workspaceOwnerId === input.workspaceOwnerId
        ) {
          session.workspaceOwnerId = input.memberUserId;
        }
      }
      await this.#state.onMemberRemoved?.(
        input.workspaceOwnerId,
        input.memberUserId,
      );
      this.#state.recordAudit({
        scope: "workspace",
        workspace_owner_id: input.workspaceOwnerId,
        actor_user_id: input.actorUserId,
        action: "team.member_removed",
        target_type: "team_member",
        target_id: input.memberUserId,
        metadata: { previous_role: target.role },
        created_at: input.updatedAt ?? new Date().toISOString(),
      });
    } else if (input.role !== undefined) {
      const previousRole = target.role;
      target.role = input.role;
      this.#state.recordAudit({
        scope: "workspace",
        workspace_owner_id: input.workspaceOwnerId,
        actor_user_id: input.actorUserId,
        action: "team.member_role_updated",
        target_type: "team_member",
        target_id: input.memberUserId,
        metadata: { previous_role: previousRole, role: input.role },
        created_at: input.updatedAt ?? new Date().toISOString(),
      });
    }
    return true;
  }

  async switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined> {
    const session = this.#state.sessions.get(input.digest);
    const owner = this.#state.users.get(input.workspaceOwnerId);
    const role = this.#state.workspaceMembers
      .get(input.workspaceOwnerId)
      ?.get(input.actorUserId)?.role;
    if (
      session?.userId !== input.actorUserId ||
      owner?.status !== "active" ||
      role === undefined
    ) {
      return undefined;
    }
    const previousWorkspaceOwnerId = session.workspaceOwnerId;
    session.workspaceOwnerId = input.workspaceOwnerId;
    if (previousWorkspaceOwnerId !== input.workspaceOwnerId) {
      this.#state.recordAudit({
        scope: "account",
        actor_user_id: input.actorUserId,
        action: "workspace.switched",
        target_type: "workspace",
        target_id: input.workspaceOwnerId,
        metadata: {},
        created_at: input.switchedAt ?? new Date().toISOString(),
      });
    }
    return {
      actor_user_id: input.actorUserId,
      workspace_owner_id: input.workspaceOwnerId,
      role,
    };
  }

  async applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult> {
    const existing = this.#state.subscriptionEvents.get(event.event_id);
    if (existing !== undefined) {
      if (!isDeepStrictEqual(existing.event, event)) {
        throw new AccountError(
          "subscription_event_conflict",
          "event ID was reused with different data",
          409,
        );
      }
      const account = await this.#state.accountView(event.user_id);
      if (account === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          404,
        );
      }
      return { applied: existing.applied, duplicate: true, account };
    }
    const user = this.#state.users.get(event.user_id);
    const subscription = this.#state.subscriptions.get(event.user_id);
    if (
      user === undefined ||
      subscription === undefined ||
      user.status !== "active"
    ) {
      throw new AccountError("account_unavailable", "account unavailable", 404);
    }
    const applied = isNewerSubscriptionEvent(subscription, event);
    if (applied) {
      for (const [otherUserId, otherSub] of this.#state.subscriptions) {
        if (
          otherUserId !== event.user_id &&
          otherSub.provider === event.provider &&
          otherSub.provider_subscription_id === event.provider_subscription_id
        ) {
          throw new AccountError(
            "subscription_event_conflict",
            "provider subscription is already linked to another account",
            409,
          );
        }
      }
    }
    this.#state.subscriptionEvents.set(event.event_id, {
      event: { ...event },
      applied,
    });
    if (applied) {
      this.#state.subscriptions.set(
        event.user_id,
        subscriptionFromEvent(event),
      );
      this.#state.recordAudit({
        scope: "account",
        actor_user_id: event.user_id,
        action: "subscription.updated",
        target_type: "subscription",
        target_id: event.event_id,
        metadata: {
          plan: event.plan,
          status: event.status,
          provider: event.provider,
        },
        created_at: event.occurred_at,
      });
    }
    const account = await this.#state.accountView(event.user_id);
    if (account === undefined) throw new Error("updated account disappeared");
    return { applied, duplicate: false, account };
  }

  async billingReference(
    userId: string,
  ): Promise<BillingReference | undefined> {
    const subscription = this.#state.subscriptions.get(userId);
    if (
      subscription?.provider === undefined ||
      subscription.provider_customer_id === undefined
    ) {
      return undefined;
    }
    return {
      provider: subscription.provider,
      customer_id: subscription.provider_customer_id,
      ...(subscription.provider_subscription_id === undefined
        ? {}
        : { subscription_id: subscription.provider_subscription_id }),
    };
  }

  async listAuditEvents(input: AuditQuery): Promise<AuditEvent[]> {
    if (
      input.scope === "workspace" &&
      !this.#state.workspaceMembers
        .get(input.workspaceOwnerId)
        ?.has(input.actorUserId)
    ) {
      return [];
    }
    const visible = this.#state.auditEvents
      .filter((event) =>
        input.scope === "account"
          ? event.scope === "account" &&
            event.actor_user_id === input.actorUserId
          : event.scope === "workspace" &&
            event.workspace_owner_id === input.workspaceOwnerId,
      )
      .sort(compareAuditEvents);
    const cursorIndex =
      input.beforeEventId === undefined
        ? -1
        : visible.findIndex((event) => event.event_id === input.beforeEventId);
    if (input.beforeEventId !== undefined && cursorIndex < 0) return [];
    return visible
      .slice(
        cursorIndex < 0 ? 0 : cursorIndex + 1,
        cursorIndex < 0 ? input.limit : cursorIndex + 1 + input.limit,
      )
      .map(cloneAuditEvent);
  }

  async recordAuditEvent(input: AuditRecordInput): Promise<boolean> {
    if (
      (input.scope === "account" &&
        (input.accountUserId === undefined ||
          input.workspaceOwnerId !== undefined ||
          !this.#state.users.has(input.accountUserId))) ||
      (input.scope === "workspace" &&
        (input.workspaceOwnerId === undefined ||
          input.accountUserId !== undefined ||
          !this.#state.workspaceMembers.has(input.workspaceOwnerId))) ||
      (input.actorUserId !== undefined &&
        !this.#state.users.has(input.actorUserId)) ||
      (input.targetType === undefined) !== (input.targetId === undefined)
    ) {
      return false;
    }
    this.#state.recordAudit({
      scope: input.scope,
      ...(input.actorUserId === undefined
        ? {}
        : { actor_user_id: input.actorUserId }),
      ...(input.workspaceOwnerId === undefined
        ? {}
        : { workspace_owner_id: input.workspaceOwnerId }),
      action: input.action,
      ...(input.targetType === undefined
        ? {}
        : { target_type: input.targetType }),
      ...(input.targetId === undefined ? {} : { target_id: input.targetId }),
      metadata: input.metadata,
      created_at: input.createdAt ?? new Date().toISOString(),
    });
    return true;
  }
}
