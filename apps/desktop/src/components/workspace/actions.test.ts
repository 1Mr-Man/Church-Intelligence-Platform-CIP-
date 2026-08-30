import { describe, expect, it } from "vitest";
import { actionsFor } from "./actions";

describe("actionsFor", () => {
  it("offers display/reject for the bible domain (a Suggestion) - display replaces approve so a live reference reaches the screen in one click", () => {
    expect(actionsFor("bible")).toEqual(["display", "reject"]);
  });

  it("offers accept/reject for music and sermon findings", () => {
    expect(actionsFor("music")).toEqual(["accept", "reject"]);
    expect(actionsFor("sermon")).toEqual(["accept", "reject"]);
  });

  it("offers only acknowledge for the service domain - never a reject action that does not exist", () => {
    expect(actionsFor("service")).toEqual(["acknowledge"]);
  });

  it("offers accept/reject for content candidates", () => {
    expect(actionsFor("content")).toEqual(["accept", "reject"]);
  });

  it("offers review/dismiss for correlations", () => {
    expect(actionsFor("correlation")).toEqual(["review", "dismiss"]);
  });

  it("returns no actions for an unrecognized domain, never guessing", () => {
    expect(actionsFor("unknown")).toEqual([]);
  });
});
