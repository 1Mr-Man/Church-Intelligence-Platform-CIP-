/**
 * Phase 6.2 (Operator Ergonomics: Display confirmation/undo) - a small,
 * generic "arm, then fire on the next matching click" guard for an action
 * whose first click should not fire it outright. Deliberately generic
 * (keyed by an arbitrary string, not "display" specifically) so any future
 * action needing the same two-click guard can reuse it without a new
 * mechanism - today only the Needs Attention queue's Bible "display"
 * action uses it, since it is the one unified action that immediately
 * projects content to a real, live screen with no way to intercept it
 * beforehand.
 *
 * Pure and DOM-free by design (this project has no DOM testing
 * environment configured), mirroring `resolveUnifiedShortcutAction`/
 * `shortcutLegend` from Phase 6.1: the caller owns all React state and
 * timers, this function only decides "arm" vs "fire" from that state.
 */
export const CONFIRM_WINDOW_MS = 4000;

export interface PendingConfirm {
  key: string;
  armedAt: number;
}

export type ConfirmDecision = { kind: "arm"; pending: PendingConfirm } | { kind: "fire" };

/**
 * A click on `key` fires immediately only if the same `key` was already
 * armed by a strictly preceding click, within `CONFIRM_WINDOW_MS`. Every
 * other case (nothing armed yet, a different key armed, or the window
 * elapsed) arms `key` fresh instead of firing - including a second click
 * on the same key that arrives exactly as, or after, the window closes.
 */
export function decideConfirmClick(key: string, pending: PendingConfirm | null, now: number): ConfirmDecision {
  if (pending && pending.key === key && now - pending.armedAt < CONFIRM_WINDOW_MS) {
    return { kind: "fire" };
  }
  return { kind: "arm", pending: { key, armedAt: now } };
}
