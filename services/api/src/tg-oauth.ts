import { createHash, randomBytes } from "node:crypto";

export interface TgOAuthConfig {
  issuerUrl?: string;
  clientId?: string;
  clientSecret?: string;
  redirectUri?: string;
  fetchFn?: typeof fetch;
}

export interface OidcDiscoveryMetadata {
  issuer: string;
  authorization_endpoint: string;
  token_endpoint: string;
  userinfo_endpoint?: string;
  device_authorization_endpoint?: string;
  jwks_uri?: string;
  end_session_endpoint?: string;
}

export interface TgOAuthTokenResponse {
  access_token: string;
  token_type: string;
  expires_in?: number;
  refresh_token?: string;
  id_token?: string;
  scope?: string;
}

export interface TgOAuthUserInfo {
  sub: string;
  email?: string;
  name?: string;
  email_verified?: boolean;
}

export interface DeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string;
  expires_in: number;
  interval: number;
}

export class TgOAuthError extends Error {
  constructor(
    public readonly code: string,
    message: string,
    public readonly statusCode = 400,
    public readonly details?: unknown,
  ) {
    super(message);
    this.name = "TgOAuthError";
  }
}

export function generatePkcePair(): {
  codeVerifier: string;
  codeChallenge: string;
  codeChallengeMethod: "S256";
} {
  const codeVerifier = randomBytes(32).toString("base64url");
  const codeChallenge = createHash("sha256")
    .update(codeVerifier, "ascii")
    .digest("base64url");
  return {
    codeVerifier,
    codeChallenge,
    codeChallengeMethod: "S256",
  };
}

export class TgOAuthClient {
  readonly #issuerUrl: string;
  readonly #clientId: string;
  readonly #clientSecret: string | undefined;
  readonly #redirectUri: string;
  readonly #fetch: typeof fetch;
  #cachedDiscovery: OidcDiscoveryMetadata | undefined;

  constructor(config: TgOAuthConfig = {}) {
    this.#issuerUrl =
      config.issuerUrl?.trim() ||
      process.env.TG_OAUTH_ISSUER?.trim() ||
      "https://auth.tannerlab.cn";
    this.#clientId =
      config.clientId?.trim() ||
      process.env.TG_OAUTH_CLIENT_ID?.trim() ||
      "agent-pulse";
    this.#clientSecret =
      config.clientSecret?.trim() ||
      process.env.TG_OAUTH_CLIENT_SECRET?.trim() ||
      undefined;
    this.#redirectUri =
      config.redirectUri?.trim() ||
      process.env.TG_OAUTH_REDIRECT_URI?.trim() ||
      "http://localhost:3000/auth/callback/tg";
    this.#fetch = config.fetchFn ?? fetch;
  }

  get issuerUrl(): string {
    return this.#issuerUrl;
  }

  get clientId(): string {
    return this.#clientId;
  }

  get redirectUri(): string {
    return this.#redirectUri;
  }

  async getDiscoveryMetadata(): Promise<OidcDiscoveryMetadata> {
    if (this.#cachedDiscovery !== undefined) {
      return this.#cachedDiscovery;
    }
    const cleanIssuer = this.#issuerUrl.replace(/\/+$/, "");
    const discoveryUrl = `${cleanIssuer}/.well-known/openid-configuration`;
    try {
      const resp = await this.#fetch(discoveryUrl, {
        headers: { accept: "application/json" },
      });
      if (resp.ok) {
        const data = (await resp.json()) as OidcDiscoveryMetadata;
        this.#cachedDiscovery = data;
        return data;
      }
    } catch {
      // Fallback below
    }

    // Default fallback endpoints based on standard tg-oauth router
    const fallback: OidcDiscoveryMetadata = {
      issuer: cleanIssuer,
      authorization_endpoint: `${cleanIssuer}/authorize`,
      token_endpoint: `${cleanIssuer}/token`,
      userinfo_endpoint: `${cleanIssuer}/userinfo`,
      device_authorization_endpoint: `${cleanIssuer}/device/code`,
      jwks_uri: `${cleanIssuer}/.well-known/jwks.json`,
      end_session_endpoint: `${cleanIssuer}/logout`,
    };
    this.#cachedDiscovery = fallback;
    return fallback;
  }

  async buildAuthorizeUrl(params: {
    state: string;
    codeChallenge: string;
    redirectUri?: string;
    scope?: string;
    nonce?: string;
  }): Promise<string> {
    const meta = await this.getDiscoveryMetadata();
    const url = new URL(meta.authorization_endpoint);
    url.searchParams.set("response_type", "code");
    url.searchParams.set("client_id", this.#clientId);
    url.searchParams.set(
      "redirect_uri",
      params.redirectUri ?? this.#redirectUri,
    );
    url.searchParams.set(
      "scope",
      params.scope ?? "openid profile email offline_access",
    );
    url.searchParams.set("state", params.state);
    url.searchParams.set("code_challenge", params.codeChallenge);
    url.searchParams.set("code_challenge_method", "S256");
    if (params.nonce) {
      url.searchParams.set("nonce", params.nonce);
    }
    return url.toString();
  }

  async exchangeCodeForTokens(params: {
    code: string;
    codeVerifier: string;
    redirectUri?: string;
  }): Promise<TgOAuthTokenResponse> {
    const meta = await this.getDiscoveryMetadata();
    const body = new URLSearchParams({
      grant_type: "authorization_code",
      code: params.code,
      redirect_uri: params.redirectUri ?? this.#redirectUri,
      code_verifier: params.codeVerifier,
      client_id: this.#clientId,
    });
    if (this.#clientSecret) {
      body.set("client_secret", this.#clientSecret);
    }

    const headers: Record<string, string> = {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
    };
    if (this.#clientSecret) {
      const basic = Buffer.from(
        `${this.#clientId}:${this.#clientSecret}`,
      ).toString("base64");
      headers.authorization = `Basic ${basic}`;
    }

    const resp = await this.#fetch(meta.token_endpoint, {
      method: "POST",
      headers,
      body: body.toString(),
    });

    const data = await resp.json().catch(() => null);
    if (!resp.ok) {
      const errorMsg =
        typeof data === "object" && data && "error_description" in data
          ? String((data as { error_description: string }).error_description)
          : typeof data === "object" && data && "error" in data
            ? String((data as { error: string }).error)
            : `Token request failed with status ${resp.status}`;
      throw new TgOAuthError(
        "token_exchange_failed",
        errorMsg,
        resp.status,
        data,
      );
    }

    return data as TgOAuthTokenResponse;
  }

  async fetchUserInfo(accessToken: string): Promise<TgOAuthUserInfo> {
    const meta = await this.getDiscoveryMetadata();
    const endpoint =
      meta.userinfo_endpoint ??
      `${this.#issuerUrl.replace(/\/+$/, "")}/userinfo`;

    const resp = await this.#fetch(endpoint, {
      headers: {
        authorization: `Bearer ${accessToken}`,
        accept: "application/json",
      },
    });

    if (!resp.ok) {
      const err = await resp.text().catch(() => "");
      throw new TgOAuthError(
        "userinfo_failed",
        `Failed to fetch userinfo: ${resp.status}`,
        resp.status,
        err,
      );
    }

    const data = (await resp.json()) as TgOAuthUserInfo;
    if (!data.sub) {
      throw new TgOAuthError(
        "invalid_userinfo",
        "UserInfo response missing subject identifier",
        400,
      );
    }
    return data;
  }

  async startDeviceFlow(
    params: { scope?: string } = {},
  ): Promise<DeviceCodeResponse> {
    const meta = await this.getDiscoveryMetadata();
    const endpoint =
      meta.device_authorization_endpoint ??
      `${this.#issuerUrl.replace(/\/+$/, "")}/device/code`;

    const body = new URLSearchParams({
      client_id: this.#clientId,
      scope: params.scope ?? "openid profile email offline_access",
    });

    const resp = await this.#fetch(endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded",
        accept: "application/json",
      },
      body: body.toString(),
    });

    const data = await resp.json().catch(() => null);
    if (!resp.ok) {
      throw new TgOAuthError(
        "device_code_failed",
        `Device code initiation failed with status ${resp.status}`,
        resp.status,
        data,
      );
    }

    return data as DeviceCodeResponse;
  }

  async pollDeviceToken(
    deviceCode: string,
  ): Promise<
    | { status: "success"; tokens: TgOAuthTokenResponse }
    | { status: "pending"; slowDown?: boolean }
  > {
    const meta = await this.getDiscoveryMetadata();
    const body = new URLSearchParams({
      grant_type: "urn:ietf:params:oauth:grant-type:device_code",
      device_code: deviceCode,
      client_id: this.#clientId,
    });
    if (this.#clientSecret) {
      body.set("client_secret", this.#clientSecret);
    }

    const resp = await this.#fetch(meta.token_endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded",
        accept: "application/json",
      },
      body: body.toString(),
    });

    const data = (await resp.json().catch(() => null)) as Record<
      string,
      unknown
    > | null;
    if (resp.ok && data && "access_token" in data) {
      return {
        status: "success",
        tokens: data as unknown as TgOAuthTokenResponse,
      };
    }

    const error = (data?.error as string | undefined) ?? "";
    if (error === "authorization_pending") {
      return { status: "pending", slowDown: false };
    }
    if (error === "slow_down") {
      return { status: "pending", slowDown: true };
    }

    throw new TgOAuthError(
      error || "device_token_poll_failed",
      (data?.error_description as string) ||
        `Device token poll failed: ${error}`,
      resp.status,
      data,
    );
  }
}
