import { describe, expect, it } from "vitest";
import { formatScriptureReference } from "./bible";
import type { ScriptureReference } from "./bible";

describe("formatScriptureReference", () => {
  it("formats a single verse the same way core/bible::ScriptureReference's Display impl does", () => {
    const reference: ScriptureReference = { translationId: "KJV", book: "ROM", chapter: 8, verseStart: 28, verseEnd: null };
    expect(formatScriptureReference(reference)).toBe("ROM 8:28");
  });

  it("formats a verse range when verseEnd differs from verseStart", () => {
    const reference: ScriptureReference = { translationId: "KJV", book: "ROM", chapter: 8, verseStart: 28, verseEnd: 30 };
    expect(formatScriptureReference(reference)).toBe("ROM 8:28-30");
  });

  it("treats an equal verseEnd/verseStart as a single verse, not a one-verse range", () => {
    const reference: ScriptureReference = { translationId: "KJV", book: "JHN", chapter: 3, verseStart: 16, verseEnd: 16 };
    expect(formatScriptureReference(reference)).toBe("JHN 3:16");
  });
});
