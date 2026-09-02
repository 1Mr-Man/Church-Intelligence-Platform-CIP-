import { describe, expect, it } from "vitest";
import { CONFIRM_WINDOW_MS, decideConfirmClick } from "./confirmGuard";

describe("decideConfirmClick", () => {
  it("arms on the first click for a key with nothing pending", () => {
    const decision = decideConfirmClick("bible-display-a", null, 1000);
    expect(decision).toEqual({ kind: "arm", pending: { key: "bible-display-a", armedAt: 1000 } });
  });

  it("fires on a second click for the same key within the confirm window", () => {
    const pending = { key: "bible-display-a", armedAt: 1000 };
    const decision = decideConfirmClick("bible-display-a", pending, 1000 + CONFIRM_WINDOW_MS - 1);
    expect(decision).toEqual({ kind: "fire" });
  });

  it("re-arms instead of firing once the confirm window has elapsed", () => {
    const pending = { key: "bible-display-a", armedAt: 1000 };
    const now = 1000 + CONFIRM_WINDOW_MS;
    const decision = decideConfirmClick("bible-display-a", pending, now);
    expect(decision).toEqual({ kind: "arm", pending: { key: "bible-display-a", armedAt: now } });
  });

  it("arms the new key instead of firing when a different key is currently armed", () => {
    const pending = { key: "bible-display-a", armedAt: 1000 };
    const decision = decideConfirmClick("bible-display-b", pending, 1500);
    expect(decision).toEqual({ kind: "arm", pending: { key: "bible-display-b", armedAt: 1500 } });
  });

  it("arms instead of firing when nothing is pending at all", () => {
    const decision = decideConfirmClick("bible-display-a", null, 5000);
    expect(decision.kind).toBe("arm");
  });
});
