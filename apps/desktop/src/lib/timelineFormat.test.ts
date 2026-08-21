import { describe, expect, it } from "vitest";
import { describeTimelineEntry } from "./timelineFormat";
import type { TimelineEntry } from "../domain";

function entry(eventName: string, payload: Record<string, unknown> | null = null): TimelineEntry {
  return {
    id: "00000000-0000-0000-0000-000000000001",
    serviceId: "00000000-0000-0000-0000-000000000002",
    eventName,
    category: "app",
    payload,
    createdAt: new Date().toISOString(),
  };
}

describe("describeTimelineEntry", () => {
  it("describes a service starting with its title", () => {
    expect(describeTimelineEntry(entry("SERVICE_STARTED", { title: "Sunday Morning" }))).toBe(
      "Service started - Sunday Morning",
    );
  });

  it("describes a suggestion created with its reference and confidence", () => {
    const description = describeTimelineEntry(
      entry("SUGGESTION_CREATED", { kind: { reference: "ROM 8:28" }, confidence: 0.98 }),
    );
    expect(description).toBe("ROM 8:28 suggested - confidence 98%");
  });

  it("describes a suggestion approval", () => {
    expect(describeTimelineEntry(entry("SUGGESTION_APPROVED", { kind: { reference: "ROM 8:28" } }))).toBe(
      "ROM 8:28 approved",
    );
  });

  it("describes an ambiguous reference resolution", () => {
    expect(describeTimelineEntry(entry("SCRIPTURE_AMBIGUOUS_RESOLVED", { selected: "JHN 3:16" }))).toBe(
      "Ambiguous reference resolved to JHN 3:16",
    );
  });

  it("describes a context correction", () => {
    expect(describeTimelineEntry(entry("SCRIPTURE_CONTEXT_CORRECTED", { corrected: "ROM 8" }))).toBe(
      "Context corrected to ROM 8",
    );
  });

  it("falls back to the raw event name for an unrecognized event", () => {
    expect(describeTimelineEntry(entry("SOMETHING_NEW"))).toBe("SOMETHING_NEW");
  });

  it("handles a missing payload gracefully", () => {
    expect(describeTimelineEntry(entry("SERVICE_PAUSED", null))).toBe("Service paused");
  });
});
