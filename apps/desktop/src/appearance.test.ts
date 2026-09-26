import { describe, expect, it } from "vitest";
import { readAppearance, saveAppearance } from "./appearance";

function storage(initial: string | null = null) {
  let value = initial;
  return {
    getItem: (_key: string) => value,
    setItem: (_key: string, next: string) => {
      value = next;
    },
  };
}

describe("local appearance preferences", () => {
  it("keeps the saved theme when server preferences contain no theme", () => {
    const store = storage();
    saveAppearance(
      { theme: "light", app_language: "auto", default_cwd: "" },
      store,
    );
    expect(readAppearance({}, store).theme).toBe("light");
  });

  it("uses saved appearance instead of a stale server theme", () => {
    const store = storage(JSON.stringify({ theme: "amber" }));
    expect(readAppearance({ theme: "dark" }, store).theme).toBe("amber");
  });

  it("validates persisted values and tolerates malformed storage", () => {
    expect(readAppearance({}, storage("{bad")).theme).toBe("light");
    expect(readAppearance({}, storage('{"theme":"unknown"}')).theme).toBe(
      "light",
    );
    expect(readAppearance({}, storage("null")).theme).toBe("light");
    expect(
      readAppearance(
        {},
        {
          getItem() {
            throw new Error("blocked");
          },
        },
      ).theme,
    ).toBe("light");
  });

  it("persists only local appearance, not backend configuration", () => {
    const store = storage();
    saveAppearance(
      { theme: "amber", app_language: "zh-CN", default_cwd: "" },
      store,
    );
    expect(JSON.parse(store.getItem("")!)).toEqual({
      theme: "amber",
      app_language: "zh-CN",
      default_cwd: "",
    });
  });
});
