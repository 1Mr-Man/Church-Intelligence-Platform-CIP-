/**
 * Phase 1.3 keyboard shortcut architecture (section 31). A = approve, R =
 * reject, E = edit, P = preview, S = focus search - all acting on the
 * first pending suggestion (the one most in need of a quick operator
 * decision). The one hard requirement: a shortcut must never fire while
 * the operator is typing - {@link shouldHandleShortcut} is the single
 * guard every handler checks first, kept as a pure, independently
 * testable function rather than inline logic scattered across handlers.
 */

const EDITABLE_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

/** Only the two properties actually read from a focused element -
 * deliberately not `HTMLElement` itself, so this guard has no dependency
 * on a browser DOM being present. In a real Tauri WebView, an actual
 * `HTMLElement` satisfies this shape naturally; in tests it's a plain
 * object, since this project has no DOM testing environment configured
 * (see `docs/live-service.md`'s testing section). */
export interface FocusedElementLike {
  tagName: string;
  isContentEditable: boolean;
}

/** A minimal shape covering only what the guard reads, so it can be unit
 * tested with a plain object instead of a real `KeyboardEvent`/DOM. */
export interface ShortcutEventLike {
  target: FocusedElementLike | null;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

export function shouldHandleShortcut(event: ShortcutEventLike): boolean {
  // A modifier held means the operator is invoking some other shortcut
  // (copy, browser devtools, ...) - never ours.
  if (event.ctrlKey || event.metaKey || event.altKey) return false;

  const target = event.target;
  if (target) {
    if (EDITABLE_TAGS.has(target.tagName)) return false;
    if (target.isContentEditable) return false;
  }
  return true;
}
