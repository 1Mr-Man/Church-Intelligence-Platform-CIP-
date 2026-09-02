/**
 * Plain-object tests, deliberately not DOM-based - this project has no
 * DOM testing environment configured (see the note in
 * `domain/contracts.test.ts`), and `shouldHandleShortcut` itself was
 * designed to depend only on a `{ tagName, isContentEditable }` shape
 * rather than a real `HTMLElement`/DOM global, precisely so it stays
 * testable without one.
 */
import { describe, expect, it } from "vitest";
import { resolveUnifiedShortcutAction, shouldHandleShortcut, type ShortcutEventLike } from "./keyboardShortcuts";

function eventFrom(
  target: ShortcutEventLike["target"],
  modifiers: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean }> = {},
): ShortcutEventLike {
  return { target, ctrlKey: false, metaKey: false, altKey: false, ...modifiers };
}

describe("shouldHandleShortcut", () => {
  it("fires for a plain keypress with no focused element", () => {
    expect(shouldHandleShortcut(eventFrom(null))).toBe(true);
  });

  it("fires when focus is on a non-editable element (e.g. a button)", () => {
    expect(shouldHandleShortcut(eventFrom({ tagName: "BUTTON", isContentEditable: false }))).toBe(true);
  });

  it("never fires while typing in an input", () => {
    expect(shouldHandleShortcut(eventFrom({ tagName: "INPUT", isContentEditable: false }))).toBe(false);
  });

  it("never fires while typing in a textarea", () => {
    expect(shouldHandleShortcut(eventFrom({ tagName: "TEXTAREA", isContentEditable: false }))).toBe(false);
  });

  it("never fires while a select is focused", () => {
    expect(shouldHandleShortcut(eventFrom({ tagName: "SELECT", isContentEditable: false }))).toBe(false);
  });

  it("never fires on a contentEditable element", () => {
    expect(shouldHandleShortcut(eventFrom({ tagName: "DIV", isContentEditable: true }))).toBe(false);
  });

  it("never fires when a modifier key is held, even outside an input", () => {
    expect(shouldHandleShortcut(eventFrom(null, { ctrlKey: true }))).toBe(false);
    expect(shouldHandleShortcut(eventFrom(null, { metaKey: true }))).toBe(false);
    expect(shouldHandleShortcut(eventFrom(null, { altKey: true }))).toBe(false);
  });
});

describe("resolveUnifiedShortcutAction", () => {
  it("maps A to the domain's primary action", () => {
    expect(resolveUnifiedShortcutAction("a", ["display", "reject"])).toBe("display");
    expect(resolveUnifiedShortcutAction("A", ["accept", "reject"])).toBe("accept");
  });

  it("maps R to the domain's secondary action", () => {
    expect(resolveUnifiedShortcutAction("r", ["display", "reject"])).toBe("reject");
    expect(resolveUnifiedShortcutAction("R", ["review", "dismiss"])).toBe("dismiss");
  });

  it("resolves R to null for a domain with only a primary action", () => {
    expect(resolveUnifiedShortcutAction("r", ["acknowledge"])).toBeNull();
  });

  it("resolves both keys to null when the domain has no actions", () => {
    expect(resolveUnifiedShortcutAction("a", [])).toBeNull();
    expect(resolveUnifiedShortcutAction("r", [])).toBeNull();
  });

  it("ignores any key other than A/R", () => {
    expect(resolveUnifiedShortcutAction("e", ["display", "reject"])).toBeNull();
    expect(resolveUnifiedShortcutAction("p", ["display", "reject"])).toBeNull();
    expect(resolveUnifiedShortcutAction("s", ["display", "reject"])).toBeNull();
  });
});
