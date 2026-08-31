import assert from "node:assert/strict";
import test from "node:test";

process.env.AGENT_PULSE_ENFORCE_ENTITLEMENTS = "true";

import { createGatewayApp } from "../src/app.js";
import { InMemoryAccountStore } from "../src/accounts.js";
import { generatePkcePair, TgOAuthClient } from "../src/tg-oauth.js";

function sessionCookie(response: { headers: Record<string, unknown> }): string {
  const value = response.headers["set-cookie"];
  if (typeof value !== "string") {
    assert.fail("response did not set a session cookie");
  }
  return value.split(";", 1)[0] ?? "";
}

test("generatePkcePair produces valid S256 verifier and challenge", () => {
  const pkce = generatePkcePair();
  assert.ok(pkce.codeVerifier.length >= 43);
  assert.ok(pkce.codeChallenge.length >= 43);
  assert.equal(pkce.codeChallengeMethod, "S256");
});

test("TgOAuthClient builds authorize URL with PKCE parameters", async () => {
  const client = new TgOAuthClient({
    issuerUrl: "https://auth.tannerlab.cn",
    clientId: "test-client",
    redirectUri: "https://agent.example.com/callback",
  });

  const pkce = generatePkcePair();
  const urlStr = await client.buildAuthorizeUrl({
    state: "test-state-123",
    codeChallenge: pkce.codeChallenge,
  });

  const parsed = new URL(urlStr);
  assert.equal(parsed.origin, "https://auth.tannerlab.cn");
  assert.equal(parsed.pathname, "/authorize");
  assert.equal(parsed.searchParams.get("client_id"), "test-client");
  assert.equal(parsed.searchParams.get("response_type"), "code");
  assert.equal(parsed.searchParams.get("code_challenge"), pkce.codeChallenge);
  assert.equal(parsed.searchParams.get("code_challenge_method"), "S256");
  assert.equal(parsed.searchParams.get("state"), "test-state-123");
});

test("GET/POST authorize-url endpoint returns valid PKCE bundle and URL", async () => {
  const app = createGatewayApp({
    accounts: new InMemoryAccountStore(),
    tgOAuthClient: new TgOAuthClient({
      issuerUrl: "https://auth.tannerlab.cn",
      clientId: "agent-pulse",
    }),
  });

  const response = await app.inject({
    method: "POST",
    url: "/v1/auth/oauth/tg/authorize-url",
    payload: {
      redirect_uri: "http://localhost:3000/auth/callback/tg",
    },
  });

  assert.equal(response.statusCode, 200);
  const body = response.json();
  assert.ok(typeof body.authorize_url === "string");
  assert.ok(typeof body.code_verifier === "string");
  assert.ok(typeof body.state === "string");
  assert.ok(body.authorize_url.includes("client_id=agent-pulse"));
  await app.close();
});

test("OAuth callback exchanges code, fetches userinfo, creates session, and links account", async () => {
  const mockFetch: typeof fetch = async (url, init) => {
    const urlStr = String(url);
    if (urlStr.endsWith("/.well-known/openid-configuration")) {
      return new Response(
        JSON.stringify({
          issuer: "https://auth.tannerlab.cn",
          authorization_endpoint: "https://auth.tannerlab.cn/authorize",
          token_endpoint: "https://auth.tannerlab.cn/token",
          userinfo_endpoint: "https://auth.tannerlab.cn/userinfo",
          device_authorization_endpoint:
            "https://auth.tannerlab.cn/device/code",
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    if (urlStr.endsWith("/token")) {
      return new Response(
        JSON.stringify({
          access_token: "mock_access_token_123",
          token_type: "Bearer",
          expires_in: 3600,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    if (urlStr.endsWith("/userinfo")) {
      return new Response(
        JSON.stringify({
          sub: "tg_user_9988",
          email: "tguser@tannerlab.cn",
          name: "Tg Developer",
          email_verified: true,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    return new Response("Not Found", { status: 404 });
  };

  const accounts = new InMemoryAccountStore();
  const mockClient = new TgOAuthClient({
    issuerUrl: "https://auth.tannerlab.cn",
    clientId: "agent-pulse",
    fetchFn: mockFetch,
  });

  const app = createGatewayApp({
    accounts,
    tgOAuthClient: mockClient,
  });

  // 1. First callback creates new OAuth account and sets session cookie
  const callbackResp = await app.inject({
    method: "POST",
    url: "/v1/auth/oauth/tg/callback",
    payload: {
      code: "valid_mock_code_123",
      code_verifier: "valid_mock_verifier_123",
      redirect_uri: "http://localhost:3000/auth/callback/tg",
    },
  });

  assert.equal(callbackResp.statusCode, 200);
  const account = callbackResp.json();
  assert.equal(account.user.email, "tguser@tannerlab.cn");
  assert.equal(account.user.display_name, "Tg Developer");
  assert.ok(callbackResp.headers["set-cookie"]);

  // 2. Validate session works with /v1/auth/me
  const meResp = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: sessionCookie(callbackResp) },
  });
  assert.equal(meResp.statusCode, 200);
  assert.equal(meResp.json().user.email, "tguser@tannerlab.cn");

  await app.close();
});

test("Device Flow initiates and polls until user completes authorization", async () => {
  let pollCount = 0;
  const mockFetch: typeof fetch = async (url, init) => {
    const urlStr = String(url);
    if (urlStr.endsWith("/device/code")) {
      return new Response(
        JSON.stringify({
          device_code: "dev_code_xyz",
          user_code: "TID-1234-5678",
          verification_uri: "https://auth.tannerlab.cn/device",
          verification_uri_complete:
            "https://auth.tannerlab.cn/device?code=TID-1234-5678",
          expires_in: 900,
          interval: 5,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    if (urlStr.endsWith("/token")) {
      pollCount++;
      if (pollCount === 1) {
        return new Response(
          JSON.stringify({ error: "authorization_pending" }),
          { status: 400, headers: { "content-type": "application/json" } },
        );
      }
      return new Response(
        JSON.stringify({
          access_token: "device_access_token_456",
          token_type: "Bearer",
          expires_in: 3600,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    if (urlStr.endsWith("/userinfo")) {
      return new Response(
        JSON.stringify({
          sub: "tg_device_user_1",
          email: "device_tester@tannerlab.cn",
          name: "Device Tester",
          email_verified: true,
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    }
    return new Response("Not Found", { status: 404 });
  };

  const accounts = new InMemoryAccountStore();
  const mockClient = new TgOAuthClient({
    issuerUrl: "https://auth.tannerlab.cn",
    clientId: "agent-pulse",
    fetchFn: mockFetch,
  });

  const app = createGatewayApp({
    accounts,
    tgOAuthClient: mockClient,
  });

  // 1. Start device flow
  const deviceStartResp = await app.inject({
    method: "POST",
    url: "/v1/auth/oauth/tg/device/code",
  });
  assert.equal(deviceStartResp.statusCode, 200);
  const deviceData = deviceStartResp.json();
  assert.equal(deviceData.user_code, "TID-1234-5678");

  // 2. Poll 1: pending
  const poll1 = await app.inject({
    method: "POST",
    url: "/v1/auth/oauth/tg/device/poll",
    payload: { device_code: deviceData.device_code },
  });
  assert.equal(poll1.statusCode, 200);
  assert.equal(poll1.json().status, "pending");

  // 3. Poll 2: success
  const poll2 = await app.inject({
    method: "POST",
    url: "/v1/auth/oauth/tg/device/poll",
    payload: { device_code: deviceData.device_code },
  });
  assert.equal(poll2.statusCode, 200);
  const poll2Data = poll2.json();
  assert.equal(poll2Data.status, "success");
  assert.equal(poll2Data.account.user.email, "device_tester@tannerlab.cn");

  await app.close();
});
