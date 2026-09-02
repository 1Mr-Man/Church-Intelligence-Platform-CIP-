import { describe, expect, it } from "vitest";
import { computeSetupGaps } from "./setupGaps";

describe("computeSetupGaps", () => {
  it("reports no gaps when the Bible dataset is installed and speech is ready", () => {
    expect(computeSetupGaps(true, "ready")).toEqual([]);
  });

  it("reports the Bible gap when no dataset is installed", () => {
    const gaps = computeSetupGaps(false, "ready");
    expect(gaps).toEqual([
      { id: "bible", message: "No Bible dataset installed - Scripture detection won't find any verses until one is." },
    ]);
  });

  it("reports the speech gap when speech is unavailable", () => {
    const gaps = computeSetupGaps(true, "unavailable");
    expect(gaps).toEqual([
      { id: "speech", message: "Automatic transcription (Whisper) isn't set up yet - manual transcript entry still works." },
    ]);
  });

  it("reports both gaps, Bible first, when neither is ready", () => {
    const gaps = computeSetupGaps(false, "unavailable");
    expect(gaps.map((g) => g.id)).toEqual(["bible", "speech"]);
  });

  it("does not report a speech gap for a live error - that's already surfaced elsewhere", () => {
    expect(computeSetupGaps(true, "error")).toEqual([]);
  });
});
