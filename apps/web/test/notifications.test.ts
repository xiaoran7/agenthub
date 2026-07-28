import assert from "node:assert/strict";
import { createECDH } from "node:crypto";
import test from "node:test";

import {
  decodeApplicationServerKey,
  parseNotificationPresentedAt,
} from "../src/lib/notifications";
import { parseSessionRef } from "../src/lib/types";

test("the application server key decodes to the gateway's raw point", () => {
  const key = createECDH("prime256v1");
  key.generateKeys();
  const publicKey = key.getPublicKey();

  const decoded = decodeApplicationServerKey(publicKey.toString("base64url"));
  assert.equal(decoded.length, 65, "browsers require the uncompressed point");
  assert.deepEqual(Buffer.from(decoded), publicKey);
});

test("base64url padding and alphabet are handled without a Buffer", () => {
  // 1, 2 and 3 byte inputs cover every padding case of the last quantum.
  for (const bytes of [[0xfb], [0xfb, 0xff], [0xfb, 0xff, 0xbe]]) {
    const source = Buffer.from(bytes);
    assert.deepEqual(
      Buffer.from(decodeApplicationServerKey(source.toString("base64url"))),
      source,
    );
  }
  assert.throws(
    () => decodeApplicationServerKey("has padding=="),
    /应用服务器公钥格式无效/,
  );
});

test("session references from a notification are validated before use", () => {
  const ref = {
    device_id: "dev_00112233445566778899aabbccddeeff",
    agent_type: "codex",
    agent_session_id: "contract-session",
  };
  assert.deepEqual(parseSessionRef(ref), ref);
  assert.deepEqual(
    parseSessionRef({ ...ref, extra: "ignored" }),
    ref,
    "extra fields are dropped, not a reason to discard the reference",
  );
  assert.equal(parseSessionRef(undefined), undefined);
  assert.equal(parseSessionRef("codex"), undefined);
  assert.equal(parseSessionRef({ ...ref, agent_type: "cursor" }), undefined);
  assert.equal(parseSessionRef({ ...ref, agent_session_id: "" }), undefined);
  assert.equal(parseSessionRef({ ...ref, device_id: 7 }), undefined);
  // Prototype keys must not pass as data.
  assert.equal(parseSessionRef({ ...ref, agent_type: "toString" }), undefined);
});

test("presentation receipts carry only a bounded browser timestamp", () => {
  assert.equal(
    parseNotificationPresentedAt({
      type: "notification-presented",
      presented_at_ms: 1_785_233_118_726,
    }),
    1_785_233_118_726,
  );
  assert.equal(parseNotificationPresentedAt(undefined), undefined);
  assert.equal(
    parseNotificationPresentedAt({
      type: "notification-presented",
      presented_at_ms: "1785233118726",
    }),
    undefined,
  );
  assert.equal(
    parseNotificationPresentedAt({
      type: "notification-presented",
      presented_at_ms: Number.POSITIVE_INFINITY,
    }),
    undefined,
  );
  assert.equal(
    parseNotificationPresentedAt({
      type: "select-session",
      presented_at_ms: 1_785_233_118_726,
    }),
    undefined,
  );
});
