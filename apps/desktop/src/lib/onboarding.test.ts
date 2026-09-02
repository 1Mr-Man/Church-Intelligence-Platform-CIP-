import { describe, expect, it } from "vitest";
import { shouldShowWalkthrough } from "./onboarding";

describe("shouldShowWalkthrough", () => {
  it("shows the walkthrough when nothing has been stored yet", () => {
    expect(shouldShowWalkthrough(null)).toBe(true);
  });

  it("shows the walkthrough for an empty stored value", () => {
    expect(shouldShowWalkthrough("")).toBe(true);
  });

  it("shows the walkthrough for any value other than the exact seen marker", () => {
    expect(shouldShowWalkthrough("true")).toBe(true);
  });

  it("does not show the walkthrough once the seen marker is stored", () => {
    expect(shouldShowWalkthrough("seen")).toBe(false);
  });
});
