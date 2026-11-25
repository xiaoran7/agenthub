import { isDeepStrictEqual } from "node:util";
import type postgres from "postgres";
import {
  AccountError,
  isNewerSubscriptionEvent,
  type AuditEvent,
  type AuditQuery,
  type AuditRecordInput,
  type BillingReference,
  type SubscriptionEvent,
  type SubscriptionEventResult,
  type SubscriptionRecord,
} from "../accounts.js";
import { type PushSubscription } from "../push.js";
import { type StoredPushSubscription } from "../store-types.js";
import {
  classifyPostgresConflict,
  isoTimestamp,
  normalizeAuditEvent,
  normalizeSubscription,
  normalizeSubscriptionEvent,
  postgresConstraint,
  type AuditEventRow,
} from "../postgres-helpers.js";
import {
  UUID_RE,
  insertAuditEvent,
  insertWorkspaceAuditIfVisible,
  postgresAccountView,
} from "./pg-shared.js";

export class PostgresBillingAuditPushRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  async applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult> {
    if (!UUID_RE.test(event.user_id)) {
      throw new AccountError("account_unavailable", "account unavailable", 404);
    }
    try {
      return await this.#sql.begin(async (sql) => {
        const subscriptions = await sql<
          Array<SubscriptionRecord & { user_status: string }>
        >`
          SELECT
            subscriptions.plan, subscriptions.status, subscriptions.provider,
            subscriptions.provider_customer_id,
            subscriptions.provider_subscription_id,
            subscriptions.current_period_end,
            subscriptions.last_event_at,
            subscriptions.last_event_id::text,
            users.status AS user_status
          FROM agent_pulse_subscriptions AS subscriptions
          INNER JOIN agent_pulse_users AS users
            ON users.user_id = subscriptions.user_id
          WHERE subscriptions.user_id = ${event.user_id}
          FOR UPDATE OF subscriptions
        `;
        const currentRow = subscriptions[0];
        if (currentRow === undefined || currentRow.user_status !== "active") {
          throw new AccountError(
            "account_unavailable",
            "account unavailable",
            404,
          );
        }
        const existing = await sql<
          Array<
            SubscriptionEvent & {
              applied: boolean;
              current_period_end: unknown | null;
            }
          >
        >`
          SELECT
            event_id::text, user_id::text, provider, provider_customer_id,
            provider_subscription_id, plan, status, current_period_end,
            occurred_at, applied
          FROM agent_pulse_subscription_events
          WHERE event_id = ${event.event_id}
        `;
        if (existing[0] !== undefined) {
          const stored = normalizeSubscriptionEvent(existing[0]);
          if (!isDeepStrictEqual(stored, event)) {
            throw new AccountError(
              "subscription_event_conflict",
              "event ID was reused with different data",
              409,
            );
          }
          const account = await postgresAccountView(sql, event.user_id);
          if (account === undefined) {
            throw new AccountError(
              "account_unavailable",
              "account unavailable",
              404,
            );
          }
          return { applied: existing[0].applied, duplicate: true, account };
        }
        const current = normalizeSubscription(currentRow);
        const applied = isNewerSubscriptionEvent(current, event);
        await sql`
          INSERT INTO agent_pulse_subscription_events (
            event_id, user_id, provider, provider_customer_id,
            provider_subscription_id, plan, status, current_period_end,
            occurred_at, applied
          ) VALUES (
            ${event.event_id}, ${event.user_id}, ${event.provider},
            ${event.provider_customer_id}, ${event.provider_subscription_id},
            ${event.plan}, ${event.status}, ${event.current_period_end ?? null},
            ${event.occurred_at}, ${applied}
          )
        `;
        if (applied) {
          await sql`
            UPDATE agent_pulse_subscriptions
            SET plan = ${event.plan}, status = ${event.status},
                provider = ${event.provider},
                provider_customer_id = ${event.provider_customer_id},
                provider_subscription_id = ${event.provider_subscription_id},
                current_period_end = ${event.current_period_end ?? null},
                last_event_at = ${event.occurred_at},
                last_event_id = ${event.event_id},
                updated_at = clock_timestamp()
            WHERE user_id = ${event.user_id}
          `;
          await insertAuditEvent(sql, {
            scope: "account",
            accountUserId: event.user_id,
            actorUserId: event.user_id,
            action: "subscription.updated",
            targetType: "subscription",
            targetId: event.event_id,
            metadata: {
              plan: event.plan,
              status: event.status,
              provider: event.provider,
            },
            createdAt: event.occurred_at,
          });
        }
        const account = await postgresAccountView(sql, event.user_id);
        if (account === undefined) {
          throw new AccountError(
            "account_unavailable",
            "account unavailable",
            404,
          );
        }
        return { applied, duplicate: false, account };
      });
    } catch (error) {
      if (
        typeof error === "object" &&
        error !== null &&
        "code" in error &&
        error.code === "23505"
      ) {
        const constraint = postgresConstraint(error);
        if (constraint === "agent_pulse_subscription_events_pkey") {
          const existing = await this.#sql<
            Array<
              SubscriptionEvent & {
                applied: boolean;
                current_period_end: unknown | null;
              }
            >
          >`
            SELECT
              event_id::text, user_id::text, provider, provider_customer_id,
              provider_subscription_id, plan, status, current_period_end,
              occurred_at, applied
            FROM agent_pulse_subscription_events
            WHERE event_id = ${event.event_id}
          `;
          if (
            existing[0] !== undefined &&
            isDeepStrictEqual(normalizeSubscriptionEvent(existing[0]), event)
          ) {
            const account = await postgresAccountView(this.#sql, event.user_id);
            if (account === undefined) {
              throw new AccountError(
                "account_unavailable",
                "account unavailable",
                404,
              );
            }
            return { applied: existing[0].applied, duplicate: true, account };
          }
          throw new AccountError(
            "subscription_event_conflict",
            "event ID was reused with different data",
            409,
          );
        }
        if (
          constraint === "agent_pulse_subscriptions_provider_subscription_idx"
        ) {
          throw new AccountError(
            "subscription_event_conflict",
            "provider subscription is already linked to another account",
            409,
          );
        }
      }
      throw error;
    }
  }

  async billingReference(
    userId: string,
  ): Promise<BillingReference | undefined> {
    if (!UUID_RE.test(userId)) return undefined;
    const rows = await this.#sql<
      Array<{
        provider: string | null;
        provider_customer_id: string | null;
        provider_subscription_id: string | null;
      }>
    >`
      SELECT provider, provider_customer_id, provider_subscription_id
      FROM agent_pulse_subscriptions
      WHERE user_id = ${userId}
      LIMIT 1
    `;
    const row = rows[0];
    if (
      row === undefined ||
      row.provider === null ||
      row.provider_customer_id === null
    ) {
      return undefined;
    }
    return {
      provider: row.provider,
      customer_id: row.provider_customer_id,
      ...(row.provider_subscription_id === null
        ? {}
        : { subscription_id: row.provider_subscription_id }),
    };
  }

  async listAuditEvents(input: AuditQuery): Promise<AuditEvent[]> {
    const cursor =
      input.beforeEventId === undefined
        ? undefined
        : (
            await this.#sql<Array<{ event_id: string; created_at: unknown }>>`
              SELECT event_id::text, created_at
              FROM agent_pulse_audit_events
              WHERE event_id = ${input.beforeEventId}
                AND (
                  (${input.scope} = 'account' AND scope = 'account'
                    AND account_user_id = ${input.actorUserId}) OR
                  (${input.scope} = 'workspace' AND scope = 'workspace'
                    AND workspace_owner_id = ${input.workspaceOwnerId}
                    AND EXISTS (
                      SELECT 1 FROM agent_pulse_workspace_members
                      WHERE workspace_owner_id = ${input.workspaceOwnerId}
                        AND user_id = ${input.actorUserId}
                    ))
                )
              LIMIT 1
            `
          )[0];
    if (input.beforeEventId !== undefined && cursor === undefined) return [];
    const cursorTime =
      cursor === undefined ? undefined : isoTimestamp(cursor.created_at);
    const rows =
      input.scope === "account"
        ? await this.#sql<AuditEventRow[]>`
            SELECT
              event_id::text, scope, actor_user_id::text,
              workspace_owner_id::text, action, target_type,
              target_id::text, metadata, created_at
            FROM agent_pulse_audit_events
            WHERE scope = 'account'
              AND account_user_id = ${input.actorUserId}
              AND (
                ${cursorTime ?? null}::timestamptz IS NULL OR
                (created_at, event_id) < (
                  ${cursorTime ?? null}::timestamptz,
                  ${cursor?.event_id ?? null}::uuid
                )
              )
            ORDER BY created_at DESC, event_id DESC
            LIMIT ${input.limit}
          `
        : await this.#sql<AuditEventRow[]>`
            SELECT
              events.event_id::text, events.scope,
              events.actor_user_id::text, events.workspace_owner_id::text,
              events.action, events.target_type, events.target_id::text,
              events.metadata, events.created_at
            FROM agent_pulse_audit_events AS events
            WHERE events.scope = 'workspace'
              AND events.workspace_owner_id = ${input.workspaceOwnerId}
              AND EXISTS (
                SELECT 1 FROM agent_pulse_workspace_members AS members
                WHERE members.workspace_owner_id = events.workspace_owner_id
                  AND members.user_id = ${input.actorUserId}
              )
              AND (
                ${cursorTime ?? null}::timestamptz IS NULL OR
                (events.created_at, events.event_id) < (
                  ${cursorTime ?? null}::timestamptz,
                  ${cursor?.event_id ?? null}::uuid
                )
              )
            ORDER BY events.created_at DESC, events.event_id DESC
            LIMIT ${input.limit}
          `;
    return rows.map(normalizeAuditEvent);
  }

  async recordAuditEvent(input: AuditRecordInput): Promise<boolean> {
    if (
      (input.scope === "account" &&
        (input.accountUserId === undefined ||
          input.workspaceOwnerId !== undefined ||
          !UUID_RE.test(input.accountUserId))) ||
      (input.scope === "workspace" &&
        (input.workspaceOwnerId === undefined ||
          input.accountUserId !== undefined ||
          !UUID_RE.test(input.workspaceOwnerId))) ||
      (input.actorUserId !== undefined && !UUID_RE.test(input.actorUserId)) ||
      (input.targetType === undefined) !== (input.targetId === undefined)
    ) {
      return false;
    }
    return this.#sql.begin(async (sql) => {
      if (input.scope === "account") {
        const users = await sql`
          SELECT 1 FROM agent_pulse_users
          WHERE user_id = ${input.accountUserId!}::uuid
          LIMIT 1
        `;
        if (users.length === 0) return false;
      } else {
        const owners = await sql`
          SELECT 1 FROM agent_pulse_users
          WHERE user_id = ${input.workspaceOwnerId!}::uuid
          LIMIT 1
        `;
        if (owners.length === 0) return false;
      }
      if (input.actorUserId !== undefined) {
        const actors = await sql`
          SELECT 1 FROM agent_pulse_users
          WHERE user_id = ${input.actorUserId}::uuid
          LIMIT 1
        `;
        if (actors.length === 0) return false;
      }
      await insertAuditEvent(sql, input);
      return true;
    });
  }

  async countActiveDevices(ownerId: string): Promise<number> {
    const rows = await this.#sql<Array<{ count: string | number }>>`
      SELECT count(*) AS count
      FROM agent_platform_devices
      WHERE owner_id = ${ownerId} AND revoked_at IS NULL
    `;
    return Number(rows[0]?.count ?? 0);
  }

  async savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
    subscriberUserId?: string,
  ): Promise<void> {
    // The endpoint identifies one browser installation. Re-subscribing under a
    // different user moves it: the browser now belongs to that user.
    await this.#sql`
      INSERT INTO agent_platform_push_subscriptions (
        endpoint,
        owner_id,
        subscriber_user_id,
        p256dh,
        auth
      )
      VALUES (
        ${subscription.endpoint},
        ${ownerId},
        ${subscriberUserId ?? null},
        ${subscription.p256dh},
        ${subscription.auth}
      )
      ON CONFLICT (endpoint) DO UPDATE SET
        owner_id = EXCLUDED.owner_id,
        subscriber_user_id = EXCLUDED.subscriber_user_id,
        p256dh = EXCLUDED.p256dh,
        auth = EXCLUDED.auth,
        updated_at = clock_timestamp()
    `;
  }

  async deletePushSubscription(
    ownerId: string,
    endpoint: string,
    subscriberUserId?: string,
  ): Promise<void> {
    if (subscriberUserId !== undefined) {
      await this.#sql`
        DELETE FROM agent_platform_push_subscriptions
        WHERE endpoint = ${endpoint}
          AND owner_id = ${ownerId}
          AND subscriber_user_id = ${subscriberUserId}
      `;
      return;
    }
    await this.#sql`
      DELETE FROM agent_platform_push_subscriptions
      WHERE endpoint = ${endpoint} AND owner_id = ${ownerId}
    `;
  }

  async deletePushSubscriptionsForSubscriber(
    ownerId: string,
    subscriberUserId: string,
  ): Promise<void> {
    await this.#sql`
      DELETE FROM agent_platform_push_subscriptions
      WHERE owner_id = ${ownerId}
        AND subscriber_user_id = ${subscriberUserId}
    `;
  }

  async listPushSubscriptions(
    ownerId: string,
    subscriberUserId?: string,
  ): Promise<StoredPushSubscription[]> {
    const rows =
      subscriberUserId !== undefined
        ? await this.#sql<
            Array<{
              endpoint: string;
              p256dh: string;
              auth: string;
              subscriber_user_id: string | null;
            }>
          >`
            SELECT endpoint, p256dh, auth, subscriber_user_id
            FROM agent_platform_push_subscriptions
            WHERE owner_id = ${ownerId}
              AND subscriber_user_id = ${subscriberUserId}
            ORDER BY endpoint
          `
        : await this.#sql<
            Array<{
              endpoint: string;
              p256dh: string;
              auth: string;
              subscriber_user_id: string | null;
            }>
          >`
            SELECT endpoint, p256dh, auth, subscriber_user_id
            FROM agent_platform_push_subscriptions
            WHERE owner_id = ${ownerId}
            ORDER BY endpoint
          `;
    return rows.map((row) => ({
      endpoint: row.endpoint,
      p256dh: row.p256dh,
      auth: row.auth,
      ...(row.subscriber_user_id === null
        ? {}
        : { subscriber_user_id: row.subscriber_user_id }),
    }));
  }

  async createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
    actorUserId?: string,
  ): Promise<void> {
    try {
      await this.#sql.begin(async (sql) => {
        await sql`
          DELETE FROM agent_platform_binding_codes
          WHERE claimed_at IS NULL
            AND expires_at <= clock_timestamp()
        `;
        await sql`
          INSERT INTO agent_platform_binding_codes (
            code_digest,
            owner_id,
            issued_by,
            expires_at
          )
          VALUES (${codeDigest}, ${ownerId}, ${actorUserId ?? null}, ${expiresAt})
        `;
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: ownerId,
          actorUserId,
          action: "device.binding_created",
          metadata: {},
        });
      });
    } catch (error) {
      throw classifyPostgresConflict(error);
    }
  }

  async countPendingBindingCodes(
    ownerId: string,
    nowIso: string,
  ): Promise<number> {
    const rows = await this.#sql<{ count: string }[]>`
      SELECT COUNT(*)::text AS count
      FROM agent_platform_binding_codes
      WHERE owner_id = ${ownerId}
        AND claimed_device_id IS NULL
        AND expires_at > ${nowIso}::timestamptz
    `;
    return Number(rows[0]?.count ?? "0");
  }
}
