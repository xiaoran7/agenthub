import assert from "node:assert/strict";
import test from "node:test";

import { sessionNeighborIndex } from "../src/lib/session-navigation";

test("detail navigation stays inside the filtered session list", () => {
  assert.equal(sessionNeighborIndex(1, 3, -1), 0);
  assert.equal(sessionNeighborIndex(1, 3, 1), 2);
  assert.equal(sessionNeighborIndex(0, 3, -1), undefined);
  assert.equal(sessionNeighborIndex(2, 3, 1), undefined);
});

test("detail navigation rejects a stale selection", () => {
  assert.equal(sessionNeighborIndex(-1, 3, 1), undefined);
  assert.equal(sessionNeighborIndex(3, 3, -1), undefined);
  assert.equal(sessionNeighborIndex(0, 0, 1), undefined);
});
