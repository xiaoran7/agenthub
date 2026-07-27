import assert from "node:assert/strict";
import test from "node:test";

import {
  CONFIG_STORAGE_KEY,
  DEFAULT_CONFIG,
  loadConfig,
  normalizeGatewayBase,
  normalizeUserId,
  saveConfig,
} from "../src/lib/config";

test("gateway base normalization mirrors the runner outbound policy", () => {
  assert.equal(normalizeGatewayBase(""), "");
  assert.equal(normalizeGatewayBase("  "), "");
  assert.equal(
    normalizeGatewayBase("http://127.0.0.1:3001/"),
    "http://127.0.0.1:3001",
  );
  assert.equal(
    normalizeGatewayBase("https://gateway.example.com/base/"),
    "https://gateway.example.com/base",
  );
  assert.throws(() => normalizeGatewayBase("http://192.168.1.5:3001"));
  assert.throws(() => normalizeGatewayBase("ftp://127.0.0.1"));
  assert.throws(() => normalizeGatewayBase("https://user:pw@example.com"));
  assert.throws(() => normalizeGatewayBase("https://example.com/?x=1"));
  assert.throws(() => normalizeGatewayBase("https://example.com/#frag"));
  assert.throws(() => normalizeGatewayBase("not a url"));
});

test("user IDs must stay within the gateway header bounds", () => {
  assert.equal(normalizeUserId("  local-user "), "local-user");
  assert.throws(() => normalizeUserId(""));
  assert.throws(() => normalizeUserId("x".repeat(129)));
});

test("config storage round-trips and shrugs off corruption", () => {
  const backing = new Map<string, string>();
  const storage = {
    getItem: (key: string) => backing.get(key) ?? null,
    setItem: (key: string, value: string) => void backing.set(key, value),
  };

  assert.deepEqual(loadConfig(undefined), DEFAULT_CONFIG);
  assert.deepEqual(loadConfig(storage), DEFAULT_CONFIG);

  saveConfig(storage, {
    gatewayBase: "http://127.0.0.1:3001",
    userId: "user-1",
  });
  assert.deepEqual(loadConfig(storage), {
    gatewayBase: "http://127.0.0.1:3001",
    userId: "user-1",
  });

  backing.set(CONFIG_STORAGE_KEY, "{not json");
  assert.deepEqual(loadConfig(storage), DEFAULT_CONFIG);

  backing.set(
    CONFIG_STORAGE_KEY,
    JSON.stringify({ gatewayBase: "http://192.168.1.5:1", userId: "u" }),
  );
  assert.deepEqual(loadConfig(storage), DEFAULT_CONFIG);
});
