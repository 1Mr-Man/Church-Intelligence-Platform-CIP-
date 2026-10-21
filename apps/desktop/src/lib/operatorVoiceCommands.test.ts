import { describe, expect, it } from "vitest";
import { parseOperatorVoiceCommand } from "./operatorVoiceCommands";

describe("parseOperatorVoiceCommand", () => {
  it("maps only available top-item actions", () => {
    expect(parseOperatorVoiceCommand("display the next suggestion", ["display", "reject"])).toMatchObject({
      kind: "top-item-action",
      action: "display",
    });
    expect(parseOperatorVoiceCommand("display the next suggestion", ["approve", "reject"])).toMatchObject({
      action: "approve",
    });
  });

  it("rejects ambiguous or unsupported commands", () => {
    expect(parseOperatorVoiceCommand("display everything", ["display"])).toBeNull();
    expect(parseOperatorVoiceCommand("reject the next item", ["display"])).toBeNull();
  });

  it("supports safe navigation commands", () => {
    expect(parseOperatorVoiceCommand("Open diagnostics mode", [])).toMatchObject({ kind: "open-diagnostics" });
    expect(parseOperatorVoiceCommand("focus search", [])).toMatchObject({ kind: "focus-search" });
  });
});
