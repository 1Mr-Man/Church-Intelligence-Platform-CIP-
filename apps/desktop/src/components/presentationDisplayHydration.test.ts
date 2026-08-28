import { describe, expect, it } from "vitest";
import { resolveHydratedPayload } from "./presentationDisplayHydration";
import type { PresentationDisplayState, PresentationItem, RenderedSlide } from "../domain";

const item: PresentationItem = {
  id: "item-1",
  serviceId: "service-1",
  content: { type: "scripture", reference: "JHN 3:16", translationId: "BSB", text: "For God so loved the world..." },
  status: "active",
  createdAt: "2026-01-01T00:00:00.000Z",
  sourceSuggestionId: null,
  template: "SCRIPTURE_DEFAULT",
};

const slide: RenderedSlide = {
  template: "SCRIPTURE_DEFAULT",
  heading: "JHN 3:16",
  bodyLines: ["For God so loved the world..."],
  footer: null,
};

describe("resolveHydratedPayload", () => {
  it("returns the item+slide pair when a real presentation is genuinely active", () => {
    const state: PresentationDisplayState = { windowOpen: true, activeItem: item, activeSlide: slide };
    expect(resolveHydratedPayload(state)).toEqual({ item, slide });
  });

  it("returns null when nothing is active", () => {
    const state: PresentationDisplayState = { windowOpen: true, activeItem: null, activeSlide: null };
    expect(resolveHydratedPayload(state)).toBeNull();
  });

  it("returns null (never fabricates a slide) if activeItem is present but activeSlide somehow is not", () => {
    const state: PresentationDisplayState = { windowOpen: true, activeItem: item, activeSlide: null };
    expect(resolveHydratedPayload(state)).toBeNull();
  });

  it("returns null if activeSlide is present but activeItem somehow is not", () => {
    const state: PresentationDisplayState = { windowOpen: true, activeItem: null, activeSlide: slide };
    expect(resolveHydratedPayload(state)).toBeNull();
  });
});
