import assert from "node:assert/strict";
import test from "node:test";

import { MAX_AVATAR_BYTES, parseAvatarPayload } from "../src/account.js";
import { ContractError } from "../src/contract-error.js";

/** A one-pixel PNG, the smallest thing a browser can decode. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

function dataUrl(mediaType: string, body: Buffer = PNG): string {
  return `data:${mediaType};base64,${body.toString("base64")}`;
}

test("a well-formed avatar data URL is accepted", () => {
  const url = dataUrl("image/png");
  assert.deepEqual(parseAvatarPayload({ data_url: url }), {
    data_url: url,
  });
});

test("an absent avatar clears the picture instead of failing", () => {
  assert.equal(parseAvatarPayload(null), undefined);
  assert.equal(parseAvatarPayload(undefined), undefined);
});

test("an unknown field in the avatar payload is rejected", () => {
  assert.throws(
    () => parseAvatarPayload({ data_url: dataUrl("image/png"), path: "/tmp" }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "unknown_field",
  );
});

test("an avatar that is not an image format is rejected", () => {
  for (const mediaType of ["image/svg+xml", "image/bmp", "text/plain"]) {
    assert.throws(
      () => parseAvatarPayload({ data_url: dataUrl(mediaType) }),
      (error: unknown) =>
        error instanceof ContractError && error.code === "invalid_avatar",
    );
  }
});

test("an avatar that is not a base64 data URL is rejected", () => {
  for (const url of [
    "https://example.com/avatar.png",
    "data:image/png,not-base64",
    "data:text/plain;base64,aGk=",
    "data:image/png;base64,",
  ]) {
    assert.throws(
      () => parseAvatarPayload({ data_url: url }),
      (error: unknown) =>
        error instanceof ContractError &&
        (error.code === "invalid_avatar" || error.code === "invalid_field"),
    );
  }
});

test("an avatar over the size cap is rejected", () => {
  const oversized = dataUrl("image/png", Buffer.alloc(MAX_AVATAR_BYTES));
  assert.throws(
    () => parseAvatarPayload({ data_url: oversized }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "avatar_too_large",
  );
});
