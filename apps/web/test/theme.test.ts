import assert from "node:assert/strict";
import test from "node:test";

import {
  getStoredTheme,
  resolveTheme,
  THEME_STORAGE_KEY,
} from "../src/lib/theme";

test("resolveTheme returns explicit light or dark directly", () => {
  assert.equal(resolveTheme("light"), "light");
  assert.equal(resolveTheme("dark"), "dark");
});

test("resolveTheme falls back to dark in non-browser environment for system", () => {
  assert.equal(resolveTheme("system"), "dark");
});

test("getStoredTheme safely returns system when window is undefined", () => {
  assert.equal(getStoredTheme(), "system");
});

test("THEME_STORAGE_KEY is consistent", () => {
  assert.equal(THEME_STORAGE_KEY, "agenthub_theme");
});
