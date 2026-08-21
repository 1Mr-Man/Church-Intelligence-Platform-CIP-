/**
 * These tests exist primarily as compile-time proof that the Phase 1
 * domain contracts have the shape this file (and therefore any real
 * consumer) expects - if a field is renamed or removed on either side of
 * the Rust/TS mirror, this file fails to type-check before it fails at
 * runtime.
 */
import { describe, expect, it } from "vitest";
import type { BibleTranslation, ScriptureReference } from "./bible";
import type { ConfidenceResult } from "./confidence";
import type { Suggestion } from "./ai";
import type { PresentationItem } from "./presentation";
import type { ServiceSession } from "./service";

describe("domain contracts", () => {
  it("constructs a ScriptureReference and a matching BibleTranslation", () => {
    const reference: ScriptureReference = {
      translationId: "KJV",
      book: "ROM",
      chapter: 8,
      verseStart: 28,
      verseEnd: null,
    };
    const translation: BibleTranslation = {
      id: "KJV",
      name: "King James Version",
      abbreviation: "KJV",
      language: "en",
      isLocal: true,
    };
    expect(reference.translationId).toBe(translation.id);
  });

  it("constructs a pending Suggestion carrying a ConfidenceResult", () => {
    const confidence: ConfidenceResult = { score: 0.92, level: "high", source: "heuristic", reason: null };
    const suggestion: Suggestion = {
      id: "00000000-0000-0000-0000-000000000001",
      kind: { type: "scripture", reference: "ROM 8:28" },
      status: "pending",
      confidence,
      createdAt: new Date().toISOString(),
    };
    expect(suggestion.status).toBe("pending");
  });

  it("constructs a ServiceSession and a PresentationItem scoped to it", () => {
    const session: ServiceSession = {
      id: "00000000-0000-0000-0000-000000000002",
      title: "Sunday Morning",
      status: "started",
      startedAt: new Date().toISOString(),
      endedAt: null,
    };
    const item: PresentationItem = {
      id: "00000000-0000-0000-0000-000000000003",
      serviceId: session.id,
      content: { type: "scripture", reference: "ROM 8:28", translationId: "KJV", text: "..." },
      status: "prepared",
      createdAt: new Date().toISOString(),
    };
    expect(item.serviceId).toBe(session.id);
  });
});
