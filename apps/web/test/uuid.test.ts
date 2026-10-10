import assert from "node:assert/strict";
import test from "node:test";

import { safeRandomUUID } from "../src/lib/uuid";

test("safeRandomUUID produces valid v4 UUIDs", () => {
  const uuid = safeRandomUUID();
  assert.equal(typeof uuid, "string");
  assert.match(
    uuid,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
});

test("safeRandomUUID generates unique identifiers", () => {
  const set = new Set<string>();
  for (let i = 0; i < 100; i++) {
    set.add(safeRandomUUID());
  }
  assert.equal(set.size, 100);
});
