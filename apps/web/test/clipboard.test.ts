import assert from "node:assert/strict";
import test from "node:test";
import { copyTextToClipboard } from "../src/lib/clipboard";

test("copyTextToClipboard returns false on empty text", async () => {
  assert.equal(await copyTextToClipboard(""), false);
});

test("copyTextToClipboard uses navigator.clipboard.writeText when available", async (t) => {
  const originalNavigator = Object.getOwnPropertyDescriptor(
    globalThis,
    "navigator",
  );
  t.after(() => {
    if (originalNavigator) {
      Object.defineProperty(globalThis, "navigator", originalNavigator);
    } else {
      Reflect.deleteProperty(globalThis, "navigator");
    }
  });

  let copied = "";
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      clipboard: {
        writeText: async (text: string) => {
          copied = text;
        },
      },
    },
  });

  assert.equal(await copyTextToClipboard("hello-world"), true);
  assert.equal(copied, "hello-world");
});

test("copyTextToClipboard falls back to execCommand and honors its boolean result", async (t) => {
  const originalNavigator = Object.getOwnPropertyDescriptor(
    globalThis,
    "navigator",
  );
  const originalDocument = Object.getOwnPropertyDescriptor(
    globalThis,
    "document",
  );
  t.after(() => {
    if (originalNavigator) {
      Object.defineProperty(globalThis, "navigator", originalNavigator);
    } else {
      Reflect.deleteProperty(globalThis, "navigator");
    }
    if (originalDocument) {
      Object.defineProperty(globalThis, "document", originalDocument);
    } else {
      Reflect.deleteProperty(globalThis, "document");
    }
  });

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      clipboard: {
        writeText: async () => {
          throw new Error("denied");
        },
      },
    },
  });

  let execResult = true;
  let removed = false;
  const fakeBody = {
    appendChild(node: { parentNode: unknown }) {
      node.parentNode = fakeBody;
    },
    removeChild(node: { parentNode: unknown }) {
      node.parentNode = null;
      removed = true;
    },
  };
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      body: fakeBody,
      createElement: () => ({
        value: "",
        style: {},
        parentNode: null,
        setAttribute() {},
        focus() {},
        select() {},
        setSelectionRange() {},
      }),
      execCommand: () => execResult,
    },
  });

  assert.equal(await copyTextToClipboard("fallback-ok"), true);
  assert.equal(removed, true);

  execResult = false;
  removed = false;
  assert.equal(await copyTextToClipboard("fallback-fail"), false);
  assert.equal(removed, true);
});
