import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import postcss from "postcss";

const modern = readFileSync(new URL("../modern.css", import.meta.url), "utf8");
const workspace = readFileSync(
  new URL("../workspace.css", import.meta.url),
  "utf8",
);
const styles = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

const sheet = postcss.parse(`${styles}\n${workspace}\n${modern}`);
function declarations(selector: string) {
  const result: Record<string, string> = {};
  sheet.walkRules((rule) => {
    if (rule.selectors.includes(selector)) {
      rule.walkDecls((decl) => {
        result[decl.prop] = decl.value;
      });
    }
  });
  return result;
}
function luminance(hex: string) {
  const channels = hex
    .slice(1)
    .match(/../g)!
    .map((part) => {
      const value = parseInt(part, 16) / 255;
      return value <= 0.04045
        ? value / 12.92
        : ((value + 0.055) / 1.055) ** 2.4;
    });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

describe("conversation and home theme contract", () => {
  it.each(["dark", "light", "amber"])(
    "keeps text pairs readable in %s",
    (theme) => {
      const tokens = declarations(`[data-theme="${theme}"]`);
      for (const [fg, bg] of [
        ["text-primary", "bg-primary"],
        ["text-heading", "bg-card-header"],
        ["text-primary", "bg-code"],
        ["text-muted", "bg-code"],
        ["text-primary", "bg-selected"],
        ["text-secondary", "bg-sidebar"],
        ["text-muted", "bg-primary"],
        ["button-primary-text", "button-primary"],
        ["action-foreground", "action-background"],
        ["code-inline-text", "code-inline-background"],
        ["status-success", "status-success-background"],
        ["status-danger", "status-danger-background"],
        ["status-info", "status-info-background"],
      ]) {
        const a = luminance(tokens[`--${fg}`]);
        const b = luminance(tokens[`--${bg}`]);
        expect(
          (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
          `${theme}: ${fg}/${bg}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    },
  );

  it("makes component surfaces consume tokens rather than per-theme overrides", () => {
    for (const selector of [
      ".single-agent-badge",
      ".sidebar-brand strong",
      ".top-icon-btn",
      ".provider-title",
      ".profile-name-text",
      ".code-copy-btn",
      ".markdown-body td",
      ".markdown-body strong",
      ".settings-modal-window",
      ".remote-launch",
    ]) {
      expect(declarations(selector).color, selector).toMatch(/^var\(--/);
    }
    for (const selector of [
      ".settings-modal-window",
      ".settings-modal-sidebar",
      ".settings-modal-main",
      ".remote-launch",
      ".code-block-wrapper",
      ".code-block-header",
    ]) {
      expect(declarations(selector).background, selector).toMatch(/^var\(--/);
    }
    postcss.parse(modern).walkRules((rule) => {
      if (rule.selector.includes("data-theme")) {
        expect(rule.selector, rule.selector).not.toMatch(/\]\s+[^\s,]/);
        rule.walkDecls((decl) => {
          expect(decl.important).toBeFalsy();
        });
      }
    });
  });

  it("uses one code frame and an unframed home layout", () => {
    expect(declarations(".markdown-body .code-block-pre")).toMatchObject({
      border: "0",
      margin: "0",
      background: "transparent",
    });
    expect(declarations(".markdown-body pre code")).toMatchObject({
      border: "0",
      background: "transparent",
      color: "var(--text-primary)",
    });
    expect(declarations(".session-empty-content").background).toBeUndefined();
    expect(declarations(".session-empty-content").border).toBeUndefined();
    expect(modern).not.toContain(".session-empty-card");
  });
});
