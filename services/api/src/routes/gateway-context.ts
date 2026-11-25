import type { FastifyInstance } from "fastify";

import type { AccountStore } from "../accounts.js";
import type { OwnerAuthenticator } from "../auth.js";
import type { BillingEventVerifier, BillingProvider } from "../billing.js";
import type { EmailVerificationSender } from "../email.js";
import type { ChangeBroker } from "../events.js";
import type { RunnerPresence } from "../presence.js";
import type { PushSender } from "../push.js";
import type {
  RemoteControlHooks,
  RemoteRouteOptions,
} from "../remote-routes.js";
import type { RemoteContentStore } from "../remote-store.js";
import type {
  GatewayStore,
  OneTimeBindingCodes,
  SessionStatusChange,
} from "../store.js";

export interface GatewayPushOptions {
  /** VAPID application server key handed to the browser at subscribe time. */
  publicKey: string;
  sender: PushSender;
}

export interface GatewayAppOptions {
  store?: GatewayStore;
  bindingCodes?: OneTimeBindingCodes;
  broker?: ChangeBroker;
  push?: GatewayPushOptions;
  authenticator?: OwnerAuthenticator;
  accounts?: AccountStore;
  secureCookies?: boolean;
  emailVerification?: {
    baseUrl: string;
    sender: EmailVerificationSender;
  };
  billingVerifier?: BillingEventVerifier;
  billingProvider?: BillingProvider;
  singleAdmin?: boolean;
  remoteContent?: RemoteContentStore;
  /** Live Runner presence shared with the v1 device list. */
  presence?: RunnerPresence;
  remoteRoutes?: RemoteRouteOptions;
  loginRateLimitSecret?: Buffer;
  allowedPushHosts?: readonly string[];
  trustProxy?: boolean | number | string[];
  bindingCodeFactory?: () => string;
  now?: () => Date;
  logger?: boolean;
}

export interface GatewayRouteContext {
  readonly app: FastifyInstance;
  readonly options: GatewayAppOptions;
  readonly store: GatewayStore;
  readonly bindingCodes: OneTimeBindingCodes | undefined;
  readonly now: () => Date;
  readonly bindingCodeFactory: () => string;
  readonly broker: ChangeBroker;
  readonly authenticator: OwnerAuthenticator;
  readonly loginRateLimitSecret: string | Buffer;
  readonly presence: RunnerPresence;
  readonly secureCookies: boolean;
  readonly singleAdmin: boolean;
  readonly invalidateAllStreamAuth: () => void;
  readonly notifyWaitingSessions: (
    ownerId: string,
    changes: readonly SessionStatusChange[],
  ) => Promise<void>;
  readonly getRemoteHooks: () => RemoteControlHooks | undefined;
}
