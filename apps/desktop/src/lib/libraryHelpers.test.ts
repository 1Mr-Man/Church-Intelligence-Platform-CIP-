import { describe, expect, it } from "vitest";
import type { PresentationItem } from "../domain";
import { filterBooksByPrefix, parseVerseRange, presentationHeading, referenceFor } from "./libraryHelpers";

describe("referenceFor", () => {
  it("formats a single verse", () => {
    expect(referenceFor("ROM", 8, 28)).toBe("ROM 8:28");
  });

  it("formats a genuine range", () => {
    expect(referenceFor("ROM", 8, 28, 30)).toBe("ROM 8:28-30");
  });

  it("does not add a range when verseEnd equals verseStart", () => {
    expect(referenceFor("ROM", 8, 28, 28)).toBe("ROM 8:28");
  });

  it("does not add a range when verseEnd is null or undefined", () => {
    expect(referenceFor("JHN", 3, 16, null)).toBe("JHN 3:16");
    expect(referenceFor("JHN", 3, 16, undefined)).toBe("JHN 3:16");
  });
});

describe("parseVerseRange", () => {
  it("parses a valid ascending range", () => {
    expect(parseVerseRange("28", "30")).toEqual({ from: 28, to: 30 });
  });

  it("rejects an inverted range", () => {
    expect(parseVerseRange("30", "28")).toBeNull();
  });

  it("rejects non-numeric input", () => {
    expect(parseVerseRange("abc", "30")).toBeNull();
    expect(parseVerseRange("28", "")).toBeNull();
  });

  it("rejects a zero or negative verse number", () => {
    expect(parseVerseRange("0", "5")).toBeNull();
  });

  it("accepts a single-verse range (from === to)", () => {
    expect(parseVerseRange("28", "28")).toEqual({ from: 28, to: 28 });
  });
});

describe("presentationHeading", () => {
  const base = {
    id: "item-1",
    serviceId: "svc-1",
    status: "prepared" as const,
    createdAt: "2026-01-01T10:00:00Z",
    sourceSuggestionId: null,
    template: null,
  };

  it("returns the reference for scripture content", () => {
    const item: PresentationItem = {
      ...base,
      content: { type: "scripture", reference: "ROM 8:28", translationId: "BSB", text: "..." },
    };
    expect(presentationHeading(item)).toBe("ROM 8:28");
  });

  it("returns the title for text content", () => {
    const item: PresentationItem = {
      ...base,
      content: { type: "text", title: "Welcome", body: "..." },
    };
    expect(presentationHeading(item)).toBe("Welcome");
  });

  it("falls back to (untitled) for text content with no title, never guessing one", () => {
    const item: PresentationItem = {
      ...base,
      content: { type: "text", title: null, body: "..." },
    };
    expect(presentationHeading(item)).toBe("(untitled)");
  });
});

describe("filterBooksByPrefix", () => {
  const books = [
    { name: "Acts" },
    { name: "Amos" },
    { name: "Romans" },
    { name: "1 Samuel" },
    { name: "1 Kings" },
    { name: "John" },
  ];

  it("matches a single-letter prefix against the display name, case-insensitively", () => {
    expect(filterBooksByPrefix(books, "a").map((b) => b.name)).toEqual(["Acts", "Amos"]);
    expect(filterBooksByPrefix(books, "A").map((b) => b.name)).toEqual(["Acts", "Amos"]);
  });

  it("matches a numbered book by its leading digit", () => {
    expect(filterBooksByPrefix(books, "1").map((b) => b.name)).toEqual(["1 Samuel", "1 Kings"]);
  });

  it("trims whitespace from the prefix before matching", () => {
    expect(filterBooksByPrefix(books, "  ro  ").map((b) => b.name)).toEqual(["Romans"]);
  });

  it("returns every book unchanged for an empty or whitespace-only prefix", () => {
    expect(filterBooksByPrefix(books, "")).toEqual(books);
    expect(filterBooksByPrefix(books, "   ")).toEqual(books);
  });

  it("returns an empty list when nothing matches, never guessing a fallback", () => {
    expect(filterBooksByPrefix(books, "zzz")).toEqual([]);
  });

  it("does not match a prefix found only mid-name", () => {
    // "ohn" is inside "John" but is not a prefix of it.
    expect(filterBooksByPrefix(books, "ohn")).toEqual([]);
  });
});
