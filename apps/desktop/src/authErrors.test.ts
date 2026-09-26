import { describe, expect, it } from "vitest";
import { isAuthFailure } from "./authErrors";

describe("isAuthFailure", () => {
  it("detects the login prompt CLIs emit as assistant text", () => {
    expect(isAuthFailure("Not logged in · Please run /login")).toBe(true);
    expect(isAuthFailure("Not logged in. Please run /login")).toBe(true);
    expect(isAuthFailure("authentication_failed")).toBe(true);
    expect(isAuthFailure("Please run /login")).toBe(true);
  });

  it("leaves ordinary replies alone", () => {
    expect(isAuthFailure("已登录并完成改造，可以继续。")).toBe(false);
    expect(isAuthFailure("I updated the login page")).toBe(false);
    expect(isAuthFailure("")).toBe(false);
    expect(isAuthFailure(undefined)).toBe(false);
    expect(isAuthFailure(null)).toBe(false);
  });
});
