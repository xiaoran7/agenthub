import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import test from "node:test";

import { createGatewayApp } from "../src/app.js";
import { SessionAuthenticator } from "../src/accounts.js";
import { SqliteAccountStore } from "../src/sqlite-account-store.js";
import { InMemoryGatewayStore } from "../src/store.js";

const TEST_DB = "./data/test-admin-setup.db";

test("admin first-run setup lifecycle and rejection of subsequent registrations", async (t) => {
  try {
    rmSync(TEST_DB, { force: true });
  } catch {}

  const accounts = new SqliteAccountStore(TEST_DB);
  const store = new InMemoryGatewayStore();
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
  });

  t.after(() => {
    try {
      rmSync(TEST_DB, { force: true });
    } catch {}
  });

  // 1. Initial status: needs_setup must be true
  const initialStatus = await app.inject({
    method: "GET",
    url: "/v1/auth/status",
  });
  assert.equal(initialStatus.statusCode, 200);
  assert.deepEqual(JSON.parse(initialStatus.body), { needs_setup: true });

  // 2. Setup the single admin account
  const setupResp = await app.inject({
    method: "POST",
    url: "/v1/auth/setup",
    payload: {
      email: "admin@selfhosted.local",
      password: "StrongAdminPassword123!",
      display_name: "SuperAdmin",
    },
  });
  assert.equal(setupResp.statusCode, 201);
  const setupAccount = JSON.parse(setupResp.body);
  assert.equal(setupAccount.user.email, "admin@selfhosted.local");
  assert.equal(setupAccount.user.display_name, "SuperAdmin");
  assert.ok(setupResp.headers["set-cookie"], "setup must issue a session cookie");

  // 3. Status after setup: needs_setup must be false
  const updatedStatus = await app.inject({
    method: "GET",
    url: "/v1/auth/status",
  });
  assert.equal(updatedStatus.statusCode, 200);
  assert.deepEqual(JSON.parse(updatedStatus.body), { needs_setup: false });

  // 4. Second setup attempt must be blocked with 409
  const secondSetup = await app.inject({
    method: "POST",
    url: "/v1/auth/setup",
    payload: {
      email: "intruder@evil.local",
      password: "IntruderPassword123!",
    },
  });
  assert.equal(secondSetup.statusCode, 409);
  assert.equal(JSON.parse(secondSetup.body).error, "admin_already_exists");

  // 5. Normal register endpoint must also be blocked with 409
  const secondRegister = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "intruder2@evil.local",
      password: "IntruderPassword123!",
    },
  });
  assert.equal(secondRegister.statusCode, 409);
  assert.equal(JSON.parse(secondRegister.body).error, "admin_already_exists");

  // 6. Admin can log in normally
  const loginResp = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload: {
      email: "admin@selfhosted.local",
      password: "StrongAdminPassword123!",
    },
  });
  assert.equal(loginResp.statusCode, 200);
  assert.ok(loginResp.headers["set-cookie"]);

  // 7. Restart simulation: Open same SQLite DB with new SqliteAccountStore instance
  const restartedAccounts = new SqliteAccountStore(TEST_DB);
  assert.equal(await restartedAccounts.hasAnyAdmin(), true);
  const adminUser = await restartedAccounts.findAccountByEmail("admin@selfhosted.local");
  assert.ok(adminUser);
  assert.equal(adminUser.display_name, "SuperAdmin");
});
