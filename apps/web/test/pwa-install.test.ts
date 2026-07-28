import assert from "node:assert/strict";
import test from "node:test";

import { installFallbackState } from "../src/lib/pwa-install";

test("installed display mode wins over browser fallback", () => {
  assert.equal(installFallbackState("Mozilla/5.0 (iPhone)", true), "installed");
});

test("iOS gets manual add-to-home-screen guidance", () => {
  assert.equal(
    installFallbackState(
      "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)",
      false,
    ),
    "manual",
  );
});

test("other browsers wait for beforeinstallprompt", () => {
  assert.equal(
    installFallbackState("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", false),
    "unavailable",
  );
});
