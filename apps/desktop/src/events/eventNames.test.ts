import { describe, expect, it } from "vitest";
import { AppEvents } from "./eventNames";

describe("AppEvents", () => {
  const names = Object.values(AppEvents);

  it("defines exactly the 42 events from the Phase 1/1.2/1.3/1.4/2.0-2.4 event architecture", () => {
    expect(names).toHaveLength(42);
  });

  it("has no duplicate event names", () => {
    expect(new Set(names).size).toBe(names.length);
  });

  it("uses SCREAMING_SNAKE_CASE for every event name, matching the Rust AppEvent enum", () => {
    for (const name of names) {
      expect(name).toBe(name.toUpperCase());
      expect(name).toMatch(/^[A-Z]+(_[A-Z]+)+$/);
    }
  });
});
