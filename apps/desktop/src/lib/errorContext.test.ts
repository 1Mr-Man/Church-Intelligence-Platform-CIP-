import { describe, expect, it } from "vitest";
import { humanizeBusyKey } from "./errorContext";

describe("humanizeBusyKey", () => {
  it("strips a trailing UUID and capitalizes the action", () => {
    expect(humanizeBusyKey("approve-00000000-0000-0000-0000-000000000001")).toBe("Approve");
  });

  it("turns hyphens into spaces for a multi-word key with a trailing UUID", () => {
    expect(humanizeBusyKey("cross-domain-dismiss-00000000-0000-0000-0000-000000000002")).toBe(
      "Cross domain dismiss",
    );
  });

  it("capitalizes a plain hyphenated key with no id suffix", () => {
    expect(humanizeBusyKey("start-service")).toBe("Start service");
  });

  it("capitalizes a single-word key unchanged otherwise", () => {
    expect(humanizeBusyKey("search")).toBe("Search");
  });

  it("leaves a non-UUID suffix (e.g. a Bible reference) untouched", () => {
    expect(humanizeBusyKey("search-preview-ROM 8:28")).toBe("Search preview ROM 8:28");
  });

  it("returns an empty string unchanged rather than throwing", () => {
    expect(humanizeBusyKey("")).toBe("");
  });
});
