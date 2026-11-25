import postgres from "postgres";
import type { AccountStore } from "./accounts.js";
import type { GatewayStore } from "./store.js";
import { migratePostgres } from "./postgres/pg-shared.js";
export { migratePostgres };
import { PostgresAccountAuthRepo } from "./postgres/pg-account-auth.js";
import { PostgresAccountLifecycleRepo } from "./postgres/pg-account-lifecycle.js";
import { PostgresWorkspacesRepo } from "./postgres/pg-workspaces.js";
import { PostgresBillingAuditPushRepo } from "./postgres/pg-billing-audit-push.js";
import { PostgresDeviceIngestRepo } from "./postgres/pg-device-ingest.js";
import { PostgresSyncReadRepo } from "./postgres/pg-sync-read.js";

export class PostgresGatewayStore implements GatewayStore, AccountStore {
  readonly #sql: postgres.Sql;
  readonly #accountAuth: PostgresAccountAuthRepo;
  readonly #accountLifecycle: PostgresAccountLifecycleRepo;
  readonly #workspaces: PostgresWorkspacesRepo;
  readonly #billingAuditPush: PostgresBillingAuditPushRepo;
  readonly #deviceIngest: PostgresDeviceIngestRepo;
  readonly #syncRead: PostgresSyncReadRepo;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
    this.#accountAuth = new PostgresAccountAuthRepo(sql);
    this.#accountLifecycle = new PostgresAccountLifecycleRepo(sql);
    this.#workspaces = new PostgresWorkspacesRepo(sql);
    this.#billingAuditPush = new PostgresBillingAuditPushRepo(sql);
    this.#deviceIngest = new PostgresDeviceIngestRepo(sql);
    this.#syncRead = new PostgresSyncReadRepo(sql);
  }

  static async connect(databaseUrl: string): Promise<PostgresGatewayStore> {
    const sql = postgres(databaseUrl, {
      max: 10,
      connect_timeout: 10,
      idle_timeout: 20,
      onnotice: () => {},
      connection: { application_name: "agent-pulse-api" },
    });
    try {
      await migratePostgres(sql);
      return new PostgresGatewayStore(sql);
    } catch (error) {
      await sql.end({ timeout: 1 });
      throw error;
    }
  }

  hasAnyAdmin(
    ...args: Parameters<PostgresAccountAuthRepo["hasAnyAdmin"]>
  ): ReturnType<PostgresAccountAuthRepo["hasAnyAdmin"]> {
    return this.#accountAuth.hasAnyAdmin(...args);
  }

  createAccount(
    ...args: Parameters<PostgresAccountAuthRepo["createAccount"]>
  ): ReturnType<PostgresAccountAuthRepo["createAccount"]> {
    return this.#accountAuth.createAccount(...args);
  }

  findAccountByEmail(
    ...args: Parameters<PostgresAccountAuthRepo["findAccountByEmail"]>
  ): ReturnType<PostgresAccountAuthRepo["findAccountByEmail"]> {
    return this.#accountAuth.findAccountByEmail(...args);
  }

  findAccountByOAuth(
    ...args: Parameters<PostgresAccountAuthRepo["findAccountByOAuth"]>
  ): ReturnType<PostgresAccountAuthRepo["findAccountByOAuth"]> {
    return this.#accountAuth.findAccountByOAuth(...args);
  }

  upsertOAuthAccount(
    ...args: Parameters<PostgresAccountAuthRepo["upsertOAuthAccount"]>
  ): ReturnType<PostgresAccountAuthRepo["upsertOAuthAccount"]> {
    return this.#accountAuth.upsertOAuthAccount(...args);
  }

  accountView(
    ...args: Parameters<PostgresAccountAuthRepo["accountView"]>
  ): ReturnType<PostgresAccountAuthRepo["accountView"]> {
    return this.#accountAuth.accountView(...args);
  }

  setAvatar(
    ...args: Parameters<PostgresAccountAuthRepo["setAvatar"]>
  ): ReturnType<PostgresAccountAuthRepo["setAvatar"]> {
    return this.#accountAuth.setAvatar(...args);
  }

  createWebSession(
    ...args: Parameters<PostgresAccountAuthRepo["createWebSession"]>
  ): ReturnType<PostgresAccountAuthRepo["createWebSession"]> {
    return this.#accountAuth.createWebSession(...args);
  }

  findWebSession(
    ...args: Parameters<PostgresAccountAuthRepo["findWebSession"]>
  ): ReturnType<PostgresAccountAuthRepo["findWebSession"]> {
    return this.#accountAuth.findWebSession(...args);
  }

  deleteWebSession(
    ...args: Parameters<PostgresAccountAuthRepo["deleteWebSession"]>
  ): ReturnType<PostgresAccountAuthRepo["deleteWebSession"]> {
    return this.#accountAuth.deleteWebSession(...args);
  }

  listWebSessions(
    ...args: Parameters<PostgresAccountAuthRepo["listWebSessions"]>
  ): ReturnType<PostgresAccountAuthRepo["listWebSessions"]> {
    return this.#accountAuth.listWebSessions(...args);
  }

  revokeWebSession(
    ...args: Parameters<PostgresAccountAuthRepo["revokeWebSession"]>
  ): ReturnType<PostgresAccountAuthRepo["revokeWebSession"]> {
    return this.#accountAuth.revokeWebSession(...args);
  }

  revokeOtherWebSessions(
    ...args: Parameters<PostgresAccountAuthRepo["revokeOtherWebSessions"]>
  ): ReturnType<PostgresAccountAuthRepo["revokeOtherWebSessions"]> {
    return this.#accountAuth.revokeOtherWebSessions(...args);
  }

  issueEmailVerification(
    ...args: Parameters<PostgresAccountLifecycleRepo["issueEmailVerification"]>
  ): ReturnType<PostgresAccountLifecycleRepo["issueEmailVerification"]> {
    return this.#accountLifecycle.issueEmailVerification(...args);
  }

  consumeEmailVerification(
    ...args: Parameters<
      PostgresAccountLifecycleRepo["consumeEmailVerification"]
    >
  ): ReturnType<PostgresAccountLifecycleRepo["consumeEmailVerification"]> {
    return this.#accountLifecycle.consumeEmailVerification(...args);
  }

  deletePendingEmailVerification(
    ...args: Parameters<
      PostgresAccountLifecycleRepo["deletePendingEmailVerification"]
    >
  ): ReturnType<
    PostgresAccountLifecycleRepo["deletePendingEmailVerification"]
  > {
    return this.#accountLifecycle.deletePendingEmailVerification(...args);
  }

  issuePasswordReset(
    ...args: Parameters<PostgresAccountLifecycleRepo["issuePasswordReset"]>
  ): ReturnType<PostgresAccountLifecycleRepo["issuePasswordReset"]> {
    return this.#accountLifecycle.issuePasswordReset(...args);
  }

  consumePasswordReset(
    ...args: Parameters<PostgresAccountLifecycleRepo["consumePasswordReset"]>
  ): ReturnType<PostgresAccountLifecycleRepo["consumePasswordReset"]> {
    return this.#accountLifecycle.consumePasswordReset(...args);
  }

  deletePendingPasswordReset(
    ...args: Parameters<
      PostgresAccountLifecycleRepo["deletePendingPasswordReset"]
    >
  ): ReturnType<PostgresAccountLifecycleRepo["deletePendingPasswordReset"]> {
    return this.#accountLifecycle.deletePendingPasswordReset(...args);
  }

  changePassword(
    ...args: Parameters<PostgresAccountLifecycleRepo["changePassword"]>
  ): ReturnType<PostgresAccountLifecycleRepo["changePassword"]> {
    return this.#accountLifecycle.changePassword(...args);
  }

  consumeLoginAttempt(
    ...args: Parameters<PostgresAccountLifecycleRepo["consumeLoginAttempt"]>
  ): ReturnType<PostgresAccountLifecycleRepo["consumeLoginAttempt"]> {
    return this.#accountLifecycle.consumeLoginAttempt(...args);
  }

  clearLoginAttempts(
    ...args: Parameters<PostgresAccountLifecycleRepo["clearLoginAttempts"]>
  ): ReturnType<PostgresAccountLifecycleRepo["clearLoginAttempts"]> {
    return this.#accountLifecycle.clearLoginAttempts(...args);
  }

  listWorkspaces(
    ...args: Parameters<PostgresWorkspacesRepo["listWorkspaces"]>
  ): ReturnType<PostgresWorkspacesRepo["listWorkspaces"]> {
    return this.#workspaces.listWorkspaces(...args);
  }

  workspaceView(
    ...args: Parameters<PostgresWorkspacesRepo["workspaceView"]>
  ): ReturnType<PostgresWorkspacesRepo["workspaceView"]> {
    return this.#workspaces.workspaceView(...args);
  }

  issueTeamInvitation(
    ...args: Parameters<PostgresWorkspacesRepo["issueTeamInvitation"]>
  ): ReturnType<PostgresWorkspacesRepo["issueTeamInvitation"]> {
    return this.#workspaces.issueTeamInvitation(...args);
  }

  consumeTeamInvitation(
    ...args: Parameters<PostgresWorkspacesRepo["consumeTeamInvitation"]>
  ): ReturnType<PostgresWorkspacesRepo["consumeTeamInvitation"]> {
    return this.#workspaces.consumeTeamInvitation(...args);
  }

  revokeTeamInvitation(
    ...args: Parameters<PostgresWorkspacesRepo["revokeTeamInvitation"]>
  ): ReturnType<PostgresWorkspacesRepo["revokeTeamInvitation"]> {
    return this.#workspaces.revokeTeamInvitation(...args);
  }

  updateWorkspaceMember(
    ...args: Parameters<PostgresWorkspacesRepo["updateWorkspaceMember"]>
  ): ReturnType<PostgresWorkspacesRepo["updateWorkspaceMember"]> {
    return this.#workspaces.updateWorkspaceMember(...args);
  }

  switchWebSessionWorkspace(
    ...args: Parameters<PostgresWorkspacesRepo["switchWebSessionWorkspace"]>
  ): ReturnType<PostgresWorkspacesRepo["switchWebSessionWorkspace"]> {
    return this.#workspaces.switchWebSessionWorkspace(...args);
  }

  applySubscriptionEvent(
    ...args: Parameters<PostgresBillingAuditPushRepo["applySubscriptionEvent"]>
  ): ReturnType<PostgresBillingAuditPushRepo["applySubscriptionEvent"]> {
    return this.#billingAuditPush.applySubscriptionEvent(...args);
  }

  billingReference(
    ...args: Parameters<PostgresBillingAuditPushRepo["billingReference"]>
  ): ReturnType<PostgresBillingAuditPushRepo["billingReference"]> {
    return this.#billingAuditPush.billingReference(...args);
  }

  listAuditEvents(
    ...args: Parameters<PostgresBillingAuditPushRepo["listAuditEvents"]>
  ): ReturnType<PostgresBillingAuditPushRepo["listAuditEvents"]> {
    return this.#billingAuditPush.listAuditEvents(...args);
  }

  recordAuditEvent(
    ...args: Parameters<PostgresBillingAuditPushRepo["recordAuditEvent"]>
  ): ReturnType<PostgresBillingAuditPushRepo["recordAuditEvent"]> {
    return this.#billingAuditPush.recordAuditEvent(...args);
  }

  countActiveDevices(
    ...args: Parameters<PostgresBillingAuditPushRepo["countActiveDevices"]>
  ): ReturnType<PostgresBillingAuditPushRepo["countActiveDevices"]> {
    return this.#billingAuditPush.countActiveDevices(...args);
  }

  savePushSubscription(
    ...args: Parameters<PostgresBillingAuditPushRepo["savePushSubscription"]>
  ): ReturnType<PostgresBillingAuditPushRepo["savePushSubscription"]> {
    return this.#billingAuditPush.savePushSubscription(...args);
  }

  deletePushSubscription(
    ...args: Parameters<PostgresBillingAuditPushRepo["deletePushSubscription"]>
  ): ReturnType<PostgresBillingAuditPushRepo["deletePushSubscription"]> {
    return this.#billingAuditPush.deletePushSubscription(...args);
  }

  deletePushSubscriptionsForSubscriber(
    ...args: Parameters<
      PostgresBillingAuditPushRepo["deletePushSubscriptionsForSubscriber"]
    >
  ): ReturnType<
    PostgresBillingAuditPushRepo["deletePushSubscriptionsForSubscriber"]
  > {
    return this.#billingAuditPush.deletePushSubscriptionsForSubscriber(...args);
  }

  listPushSubscriptions(
    ...args: Parameters<PostgresBillingAuditPushRepo["listPushSubscriptions"]>
  ): ReturnType<PostgresBillingAuditPushRepo["listPushSubscriptions"]> {
    return this.#billingAuditPush.listPushSubscriptions(...args);
  }

  createBindingCode(
    ...args: Parameters<PostgresBillingAuditPushRepo["createBindingCode"]>
  ): ReturnType<PostgresBillingAuditPushRepo["createBindingCode"]> {
    return this.#billingAuditPush.createBindingCode(...args);
  }

  countPendingBindingCodes(
    ...args: Parameters<
      PostgresBillingAuditPushRepo["countPendingBindingCodes"]
    >
  ): ReturnType<PostgresBillingAuditPushRepo["countPendingBindingCodes"]> {
    return this.#billingAuditPush.countPendingBindingCodes(...args);
  }

  registerDeviceWithBindingCode(
    ...args: Parameters<
      PostgresDeviceIngestRepo["registerDeviceWithBindingCode"]
    >
  ): ReturnType<PostgresDeviceIngestRepo["registerDeviceWithBindingCode"]> {
    return this.#deviceIngest.registerDeviceWithBindingCode(...args);
  }

  revokeDevice(
    ...args: Parameters<PostgresDeviceIngestRepo["revokeDevice"]>
  ): ReturnType<PostgresDeviceIngestRepo["revokeDevice"]> {
    return this.#deviceIngest.revokeDevice(...args);
  }

  registerDevice(
    ...args: Parameters<PostgresDeviceIngestRepo["registerDevice"]>
  ): ReturnType<PostgresDeviceIngestRepo["registerDevice"]> {
    return this.#deviceIngest.registerDevice(...args);
  }

  ingestBatch(
    ...args: Parameters<PostgresDeviceIngestRepo["ingestBatch"]>
  ): ReturnType<PostgresDeviceIngestRepo["ingestBatch"]> {
    return this.#deviceIngest.ingestBatch(...args);
  }

  ingestAuthBatch(
    ...args: Parameters<PostgresDeviceIngestRepo["ingestAuthBatch"]>
  ): ReturnType<PostgresDeviceIngestRepo["ingestAuthBatch"]> {
    return this.#deviceIngest.ingestAuthBatch(...args);
  }

  recordHeartbeat(
    ...args: Parameters<PostgresSyncReadRepo["recordHeartbeat"]>
  ): ReturnType<PostgresSyncReadRepo["recordHeartbeat"]> {
    return this.#syncRead.recordHeartbeat(...args);
  }

  getRegisteredDevice(
    ...args: Parameters<PostgresSyncReadRepo["getRegisteredDevice"]>
  ): ReturnType<PostgresSyncReadRepo["getRegisteredDevice"]> {
    return this.#syncRead.getRegisteredDevice(...args);
  }

  getRegisteredDevices(
    ...args: Parameters<PostgresSyncReadRepo["getRegisteredDevices"]>
  ): ReturnType<PostgresSyncReadRepo["getRegisteredDevices"]> {
    return this.#syncRead.getRegisteredDevices(...args);
  }

  listDevices(
    ...args: Parameters<PostgresSyncReadRepo["listDevices"]>
  ): ReturnType<PostgresSyncReadRepo["listDevices"]> {
    return this.#syncRead.listDevices(...args);
  }

  listSessions(
    ...args: Parameters<PostgresSyncReadRepo["listSessions"]>
  ): ReturnType<PostgresSyncReadRepo["listSessions"]> {
    return this.#syncRead.listSessions(...args);
  }

  listSessionEvents(
    ...args: Parameters<PostgresSyncReadRepo["listSessionEvents"]>
  ): ReturnType<PostgresSyncReadRepo["listSessionEvents"]> {
    return this.#syncRead.listSessionEvents(...args);
  }

  listAuth(
    ...args: Parameters<PostgresSyncReadRepo["listAuth"]>
  ): ReturnType<PostgresSyncReadRepo["listAuth"]> {
    return this.#syncRead.listAuth(...args);
  }

  pruneExpiredData(
    ...args: Parameters<PostgresSyncReadRepo["pruneExpiredData"]>
  ): ReturnType<PostgresSyncReadRepo["pruneExpiredData"]> {
    return this.#syncRead.pruneExpiredData(...args);
  }

  async close(): Promise<void> {
    await this.#sql.end({ timeout: 5 });
  }
}
