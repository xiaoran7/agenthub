import type postgres from "postgres";
import {
  AccountError,
  entitlementsFor,
  type InviteRole,
  type SessionIdentity,
  type SubscriptionRecord,
  type WorkspaceMemberView,
  type WorkspaceRole,
  type WorkspaceSummary,
  type TeamWorkspaceView,
} from "../accounts.js";
import {
  isoTimestamp,
  normalizeSubscription,
  postgresConstraint,
} from "../postgres-helpers.js";
import { UUID_RE, insertAuditEvent } from "./pg-shared.js";

export class PostgresWorkspacesRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  async listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]> {
    if (!UUID_RE.test(userId)) return [];
    const rows = await this.#sql<
      Array<{
        workspace_owner_id: string;
        owner_name: string;
        role: WorkspaceRole;
      }>
    >`
      SELECT
        members.workspace_owner_id::text,
        owners.display_name AS owner_name,
        members.role
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS owners
        ON owners.user_id = members.workspace_owner_id
      WHERE members.user_id = ${userId} AND owners.status = 'active'
      ORDER BY
        members.workspace_owner_id = ${userId} DESC,
        owners.display_name,
        members.workspace_owner_id
    `;
    return rows.map((row) => ({
      workspace_owner_id: row.workspace_owner_id,
      name:
        row.workspace_owner_id === userId
          ? "个人工作区"
          : `${row.owner_name} 的工作区`,
      role: row.role,
      current: row.workspace_owner_id === currentOwnerId,
    }));
  }

  async workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined> {
    if (!UUID_RE.test(actorUserId) || !UUID_RE.test(workspaceOwnerId)) {
      return undefined;
    }
    const memberships = await this.#sql<
      Array<{ role: WorkspaceRole; owner_name: string }>
    >`
      SELECT members.role, owners.display_name AS owner_name
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS owners
        ON owners.user_id = members.workspace_owner_id
      WHERE members.workspace_owner_id = ${workspaceOwnerId}
        AND members.user_id = ${actorUserId}
        AND owners.status = 'active'
      LIMIT 1
    `;
    const membership = memberships[0];
    if (membership === undefined) return undefined;
    const members = await this.#sql<
      Array<{
        user_id: string;
        email: string;
        display_name: string;
        role: WorkspaceRole;
        created_at: unknown;
      }>
    >`
      SELECT
        users.user_id::text,
        users.email,
        users.display_name,
        members.role,
        members.created_at
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS users ON users.user_id = members.user_id
      WHERE members.workspace_owner_id = ${workspaceOwnerId}
      ORDER BY
        CASE members.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,
        users.display_name,
        users.user_id
    `;
    const canManage =
      membership.role === "owner" || membership.role === "admin";
    const invitations = canManage
      ? await this.#sql<
          Array<{
            invitation_id: string;
            invited_email: string;
            role: InviteRole;
            created_at: unknown;
            expires_at: unknown;
          }>
        >`
          SELECT
            invitation_id::text,
            invited_email,
            role,
            created_at,
            expires_at
          FROM agent_pulse_team_invitations
          WHERE workspace_owner_id = ${workspaceOwnerId}
            AND consumed_at IS NULL
            AND revoked_at IS NULL
            AND expires_at > ${now}
          ORDER BY created_at DESC, invitation_id
        `
      : [];
    return {
      workspace: {
        workspace_owner_id: workspaceOwnerId,
        name:
          workspaceOwnerId === actorUserId
            ? "个人工作区"
            : `${membership.owner_name} 的工作区`,
        role: membership.role,
        current: true,
      },
      members: members.map((member): WorkspaceMemberView => ({
        user_id: member.user_id,
        email: member.email,
        display_name: member.display_name,
        role: member.role,
        joined_at: isoTimestamp(member.created_at),
      })),
      invitations: invitations.map((invitation) => ({
        invitation_id: invitation.invitation_id,
        email: invitation.invited_email,
        role: invitation.role,
        created_at: isoTimestamp(invitation.created_at),
        expires_at: isoTimestamp(invitation.expires_at),
      })),
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
    return this.#sql.begin(async (sql) => {
      await sql`
        SELECT user_id
        FROM agent_pulse_subscriptions
        WHERE user_id = ${input.workspaceOwnerId}
        FOR UPDATE
      `;
      const actors = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT role
        FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND user_id = ${input.invitedBy}
      `;
      const actorRole = actors[0]?.role;
      if (
        (actorRole !== "owner" && actorRole !== "admin") ||
        (actorRole === "admin" && input.role === "admin")
      ) {
        throw new AccountError(
          "team_permission_denied",
          "team permission denied",
          403,
        );
      }
      const counts = await sql<Array<{ count: string | number }>>`
        SELECT (
          SELECT count(*) FROM agent_pulse_workspace_members
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
        ) + (
          SELECT count(*) FROM agent_pulse_team_invitations
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND consumed_at IS NULL
            AND revoked_at IS NULL
            AND expires_at > ${input.createdAt}
        ) AS count
      `;
      if (Number(counts[0]?.count ?? 0) >= input.memberLimit) {
        throw new AccountError(
          "team_member_limit_reached",
          "team member limit reached",
          403,
        );
      }
      const existing = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT role FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND user_id = ${input.invitedUserId}
      `;
      if (existing[0] !== undefined) {
        throw new AccountError(
          "team_member_exists",
          "user is already a member",
          409,
        );
      }
      await sql`
        UPDATE agent_pulse_team_invitations
        SET revoked_at = ${input.createdAt}
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND invited_user_id = ${input.invitedUserId}
          AND consumed_at IS NULL
          AND revoked_at IS NULL
          AND expires_at <= ${input.createdAt}
      `;
      try {
        await sql`
          INSERT INTO agent_pulse_team_invitations (
            invitation_id, token_digest, workspace_owner_id, invited_user_id,
            invited_email, role, invited_by, created_at, expires_at
          ) VALUES (
            ${input.invitationId}, ${input.digest}, ${input.workspaceOwnerId},
            ${input.invitedUserId}, ${input.invitedEmail}, ${input.role},
            ${input.invitedBy}, ${input.createdAt}, ${input.expiresAt}
          )
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.invitedBy,
          action: "team.invitation_created",
          targetType: "team_invitation",
          targetId: input.invitationId,
          metadata: {
            role: input.role,
            invited_user_id: input.invitedUserId,
          },
          createdAt: input.createdAt,
        });
      } catch (error) {
        if (
          postgresConstraint(error) ===
          "agent_pulse_team_invitations_pending_idx"
        ) {
          throw new AccountError(
            "team_invitation_exists",
            "invitation already exists",
            409,
          );
        }
        throw error;
      }
    });
  }

  async consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined> {
    return this.#sql.begin(async (sql) => {
      const candidates = await sql<
        Array<{
          invitation_id: string;
          workspace_owner_id: string;
          role: InviteRole;
        }>
      >`
        SELECT invitation_id::text, workspace_owner_id::text, role
        FROM agent_pulse_team_invitations
        WHERE token_digest = ${input.digest}
          AND invited_user_id = ${input.userId}
          AND consumed_at IS NULL
          AND revoked_at IS NULL
          AND expires_at > ${input.now}
        FOR UPDATE
      `;
      const invitation = candidates[0];
      if (invitation === undefined) return undefined;
      const subscriptions = await sql<SubscriptionRecord[]>`
        SELECT plan, status, current_period_end
        FROM agent_pulse_subscriptions
        WHERE user_id = ${invitation.workspace_owner_id}
        FOR UPDATE
      `;
      const subscription = subscriptions[0];
      const memberLimit =
        subscription === undefined
          ? 0
          : entitlementsFor(normalizeSubscription(subscription)).team_members;
      const memberCounts = await sql<Array<{ count: string | number }>>`
        SELECT count(*) AS count
        FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${invitation.workspace_owner_id}
      `;
      if (Number(memberCounts[0]?.count ?? 0) >= memberLimit) {
        throw new AccountError(
          "team_member_limit_reached",
          "team member limit reached",
          403,
        );
      }
      await sql`
        UPDATE agent_pulse_team_invitations
        SET consumed_at = ${input.now}
        WHERE token_digest = ${input.digest}
      `;
      await sql`
        INSERT INTO agent_pulse_workspace_members (
          workspace_owner_id, user_id, role, created_at
        ) VALUES (
          ${invitation.workspace_owner_id}, ${input.userId},
          ${invitation.role}, ${input.now}
        )
        ON CONFLICT (workspace_owner_id, user_id) DO NOTHING
      `;
      await insertAuditEvent(sql, {
        scope: "workspace",
        workspaceOwnerId: invitation.workspace_owner_id,
        actorUserId: input.userId,
        action: "team.invitation_accepted",
        targetType: "team_member",
        targetId: input.userId,
        metadata: {
          role: invitation.role,
          invitation_id: invitation.invitation_id,
        },
        createdAt: input.now,
      });
      return invitation.workspace_owner_id;
    });
  }

  async revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{ invitation_id: string; role: InviteRole }>
      >`
        UPDATE agent_pulse_team_invitations AS invitations
        SET revoked_at = ${now}
        FROM agent_pulse_workspace_members AS actors
        WHERE invitations.invitation_id = ${invitationId}
          AND invitations.workspace_owner_id = ${workspaceOwnerId}
          AND invitations.consumed_at IS NULL
          AND invitations.revoked_at IS NULL
          AND actors.workspace_owner_id = invitations.workspace_owner_id
          AND actors.user_id = ${actorUserId}
          AND (
            actors.role = 'owner' OR
            (actors.role = 'admin' AND invitations.role = 'member')
          )
        RETURNING invitations.invitation_id::text, invitations.role
      `;
      const invitation = rows[0];
      if (invitation === undefined) return false;
      await insertAuditEvent(sql, {
        scope: "workspace",
        workspaceOwnerId,
        actorUserId,
        action: "team.invitation_revoked",
        targetType: "team_invitation",
        targetId: invitationId,
        metadata: { role: invitation.role },
        createdAt: now,
      });
      return true;
    });
  }

  async updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{ actor_role: WorkspaceRole; target_role: WorkspaceRole }>
      >`
        SELECT actors.role AS actor_role, targets.role AS target_role
        FROM agent_pulse_workspace_members AS actors
        INNER JOIN agent_pulse_workspace_members AS targets
          ON targets.workspace_owner_id = actors.workspace_owner_id
        WHERE actors.workspace_owner_id = ${input.workspaceOwnerId}
          AND actors.user_id = ${input.actorUserId}
          AND targets.user_id = ${input.memberUserId}
        FOR UPDATE OF targets
      `;
      const roles = rows[0];
      if (
        roles === undefined ||
        roles.target_role === "owner" ||
        (roles.actor_role !== "owner" &&
          (roles.actor_role !== "admin" ||
            roles.target_role !== "member" ||
            input.role === "admin"))
      ) {
        return false;
      }
      if (input.remove) {
        await sql`
          DELETE FROM agent_pulse_workspace_members
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND user_id = ${input.memberUserId}
        `;
        await sql`
          UPDATE agent_pulse_web_sessions
          SET workspace_owner_id = user_id
          WHERE user_id = ${input.memberUserId}
            AND workspace_owner_id = ${input.workspaceOwnerId}
        `;
        await sql`
          DELETE FROM agent_platform_push_subscriptions
          WHERE owner_id = ${input.workspaceOwnerId}
            AND subscriber_user_id = ${input.memberUserId}
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.actorUserId,
          action: "team.member_removed",
          targetType: "team_member",
          targetId: input.memberUserId,
          metadata: { previous_role: roles.target_role },
          ...(input.updatedAt === undefined
            ? {}
            : { createdAt: input.updatedAt }),
        });
      } else if (input.role !== undefined) {
        await sql`
          UPDATE agent_pulse_workspace_members
          SET role = ${input.role}
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND user_id = ${input.memberUserId}
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.actorUserId,
          action: "team.member_role_updated",
          targetType: "team_member",
          targetId: input.memberUserId,
          metadata: {
            previous_role: roles.target_role,
            role: input.role,
          },
          ...(input.updatedAt === undefined
            ? {}
            : { createdAt: input.updatedAt }),
        });
      }
      return true;
    });
  }

  async switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined> {
    if (
      !UUID_RE.test(input.actorUserId) ||
      !UUID_RE.test(input.workspaceOwnerId)
    ) {
      return undefined;
    }
    return this.#sql.begin(async (sql) => {
      const sessions = await sql<Array<{ workspace_owner_id: string }>>`
        SELECT workspace_owner_id::text
        FROM agent_pulse_web_sessions
        WHERE session_digest = ${input.digest}
          AND user_id = ${input.actorUserId}
        FOR UPDATE
      `;
      const previousWorkspaceOwnerId = sessions[0]?.workspace_owner_id;
      if (previousWorkspaceOwnerId === undefined) return undefined;
      const members = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT members.role
        FROM agent_pulse_workspace_members AS members
        INNER JOIN agent_pulse_users AS owners
          ON owners.user_id = members.workspace_owner_id
        WHERE members.workspace_owner_id = ${input.workspaceOwnerId}
          AND members.user_id = ${input.actorUserId}
          AND owners.status = 'active'
      `;
      const role = members[0]?.role;
      if (role === undefined) return undefined;
      await sql`
        UPDATE agent_pulse_web_sessions
        SET workspace_owner_id = ${input.workspaceOwnerId}
        WHERE session_digest = ${input.digest}
      `;
      if (previousWorkspaceOwnerId !== input.workspaceOwnerId) {
        await insertAuditEvent(sql, {
          scope: "account",
          accountUserId: input.actorUserId,
          actorUserId: input.actorUserId,
          action: "workspace.switched",
          targetType: "workspace",
          targetId: input.workspaceOwnerId,
          metadata: {},
          ...(input.switchedAt === undefined
            ? {}
            : { createdAt: input.switchedAt }),
        });
      }
      return {
        actor_user_id: input.actorUserId,
        workspace_owner_id: input.workspaceOwnerId,
        role,
      };
    });
  }
}
