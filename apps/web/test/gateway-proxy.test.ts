import assert from "node:assert/strict";
import test from "node:test";

import {
  gatewayHeaders,
  gatewayOrigin,
  proxyGatewayRequest,
} from "../src/server/gateway-proxy";
import * as gatewayRoute from "../app/v1/[...path]/route";

const ACCESS_TOKEN = "test-access-token-32-bytes-long!!";

test("same-origin route exposes every supported mutation method", () => {
  assert.equal(typeof gatewayRoute.GET, "function");
  assert.equal(typeof gatewayRoute.POST, "function");
  assert.equal(typeof gatewayRoute.PATCH, "function");
  assert.equal(typeof gatewayRoute.DELETE, "function");
});

test("gateway origin rejects credential leaks and insecure remote transport", () => {
  assert.equal(gatewayOrigin(undefined), "http://127.0.0.1:3001");
  assert.equal(
    gatewayOrigin("https://gateway.example.com/"),
    "https://gateway.example.com",
  );
  assert.throws(() => gatewayOrigin("http://192.168.1.2:3001"));
  assert.throws(() => gatewayOrigin("https://user:secret@example.com"));
  assert.throws(() => gatewayOrigin("https://example.com/base"));
  assert.throws(() => gatewayOrigin("https://example.com?token=secret"));
});

test("server access token overrides browser supplied identity and credentials", () => {
  const incoming = new Headers({
    accept: "application/json",
    authorization: "Bearer browser-controlled-token",
    "content-type": "application/json",
    "x-user-id": "spoofed-owner",
  });
  const headers = gatewayHeaders(incoming, ACCESS_TOKEN);

  assert.equal(headers.get("authorization"), `Bearer ${ACCESS_TOKEN}`);
  assert.equal(headers.get("x-user-id"), null);
  assert.equal(headers.get("accept"), "application/json");
  assert.equal(headers.get("content-type"), "application/json");
});

test("development proxy forwards session cookie and explicit local identity", () => {
  const incoming = new Headers({
    authorization: "Bearer browser-controlled-token",
    "x-user-id": "local-user",
    cookie: "analytics=private; agent_pulse_session=opaque",
  });
  const headers = gatewayHeaders(incoming, undefined);

  assert.equal(headers.get("authorization"), null);
  assert.equal(headers.get("x-user-id"), "local-user");
  assert.equal(headers.get("cookie"), "agent_pulse_session=opaque");
});

test("proxy rejects malformed server access tokens", () => {
  assert.throws(() => gatewayHeaders(new Headers(), "short"));
  assert.throws(() =>
    gatewayHeaders(
      new Headers(),
      "test access token with whitespace and enough bytes",
    ),
  );
});

test("same-origin proxy injects the server credential without leaking browser headers", async () => {
  let forwardedUrl: string | undefined;
  let forwardedInit: RequestInit | undefined;
  const response = await proxyGatewayRequest(
    new Request(
      "https://panel.example/v1/sessions/events?agent_session_id=a%2Fb",
      {
        headers: {
          authorization: "Bearer attacker-token",
          cookie: "analytics=private; __Host-agent_pulse_session=opaque",
          "x-user-id": "spoofed-owner",
        },
      },
    ),
    ["sessions", "events"],
    {
      origin: "https://gateway.example",
      accessToken: ACCESS_TOKEN,
      fetch: async (input, init) => {
        forwardedUrl = String(input);
        forwardedInit = init;
        return Response.json({ events: [] });
      },
    },
  );

  assert.equal(
    forwardedUrl,
    "https://gateway.example/v1/sessions/events?agent_session_id=a%2Fb",
  );
  const headers = new Headers(forwardedInit?.headers);
  assert.equal(headers.get("authorization"), `Bearer ${ACCESS_TOKEN}`);
  assert.equal(headers.get("x-user-id"), null);
  assert.equal(headers.get("cookie"), "__Host-agent_pulse_session=opaque");
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { events: [] });
});

test("proxy returns an upstream session cookie to the browser", async () => {
  const response = await proxyGatewayRequest(
    new Request("https://panel.example/v1/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email: "user@example.com", password: "password" }),
    }),
    ["auth", "login"],
    {
      origin: "https://gateway.example",
      accessToken: undefined,
      fetch: async () =>
        new Response(JSON.stringify({ user: {} }), {
          headers: {
            "content-type": "application/json",
            "set-cookie":
              "__Host-agent_pulse_session=opaque; Path=/; HttpOnly; Secure",
          },
        }),
    },
  );
  assert.match(
    response.headers.get("set-cookie") ?? "",
    /__Host-agent_pulse_session=opaque/,
  );
});
