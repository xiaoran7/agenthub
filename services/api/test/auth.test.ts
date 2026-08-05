import assert from "node:assert/strict";
import test from "node:test";

import { createGatewayApp } from "../src/app.js";
import { BearerTokenAuthenticator } from "../src/auth.js";

const ACCESS_TOKEN = "test-access-token-32-bytes-long!!";

test("gateway factory denies protected routes when authentication is omitted", async () => {
  const app = createGatewayApp();
  const response = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "spoofed-owner" },
  });

  assert.equal(response.statusCode, 401);
  assert.equal(response.json().error, "authentication_required");
  await app.close();
});

test("bearer authentication rejects spoofed development identities", async () => {
  const app = createGatewayApp({
    authenticator: new BearerTokenAuthenticator("owner-1", ACCESS_TOKEN),
  });

  for (const headers of [
    undefined,
    { "x-user-id": "owner-1" },
    { authorization: "Bearer wrong-access-token-32-bytes!!" },
    { authorization: `bearer ${ACCESS_TOKEN}` },
    { authorization: `Bearer ${ACCESS_TOKEN} trailing` },
  ]) {
    const response = await app.inject({
      method: "GET",
      url: "/v1/devices",
      ...(headers === undefined ? {} : { headers }),
    });
    assert.equal(response.statusCode, 401);
    assert.equal(response.json().error, "authentication_required");
    assert.equal(
      response.headers["www-authenticate"],
      'Bearer realm="agent-pulse"',
    );
  }

  const accepted = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: {
      authorization: `Bearer ${ACCESS_TOKEN}`,
      "x-user-id": "spoofed-owner",
    },
  });
  assert.equal(accepted.statusCode, 200);
  assert.deepEqual(accepted.json(), { devices: [] });
  await app.close();
});

test("bearer authentication validates deployment configuration", () => {
  assert.throws(
    () => new BearerTokenAuthenticator("", ACCESS_TOKEN),
    /AGENT_PLATFORM_OWNER_ID/,
  );
  assert.throws(
    () => new BearerTokenAuthenticator("owner-1", "short"),
    /AGENT_PLATFORM_ACCESS_TOKEN/,
  );
  assert.throws(
    () =>
      new BearerTokenAuthenticator(
        "owner-1",
        "test access token with whitespace and enough bytes",
      ),
    /AGENT_PLATFORM_ACCESS_TOKEN/,
  );
});
