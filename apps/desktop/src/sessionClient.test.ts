import { describe, expect, it } from "vitest";

import { SESSION_COMMANDS } from "./sessionClient";

describe("desktop session command contract", () => {
  it("uses the Tauri handler names registered by the Rust desktop", () => {
    expect(SESSION_COMMANDS).toEqual({
      launch: "launch_new_session",
      send: "send_session_message",
      cancel: "cancel_session_turn",
      resolveApproval: "resolve_session_approval",
    });
  });

  it("never revives the removed send_session_turn alias", () => {
    expect(Object.values(SESSION_COMMANDS)).not.toContain("send_session_turn");
  });
});
