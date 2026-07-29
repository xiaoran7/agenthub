import assert from "node:assert/strict";
import test from "node:test";

import { nextDialogFocusIndex } from "../src/lib/dialog-focus";

test("dialog focus wraps forward and backward", () => {
  assert.equal(nextDialogFocusIndex(0, 3, false), 1);
  assert.equal(nextDialogFocusIndex(2, 3, false), 0);
  assert.equal(nextDialogFocusIndex(0, 3, true), 2);
  assert.equal(nextDialogFocusIndex(2, 3, true), 1);
});

test("dialog focus starts at the direction-aware edge", () => {
  assert.equal(nextDialogFocusIndex(-1, 3, false), 0);
  assert.equal(nextDialogFocusIndex(-1, 3, true), 2);
  assert.equal(nextDialogFocusIndex(-1, 0, false), -1);
});
