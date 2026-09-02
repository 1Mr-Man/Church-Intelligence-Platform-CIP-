/**
 * Phase 6.6 (Operator Ergonomics: onboarding). `App.tsx` had zero
 * first-run affordance - every launch, first-ever or the thousandth,
 * dropped straight into the live workspace with no explanation of the
 * Start Service -> Needs Attention queue -> Approve/Reject -> Display
 * workflow. `shouldShowWalkthrough` is the one piece of logic worth unit
 * testing on its own: whether a given stored value means "already seen."
 * The actual `localStorage` read/write stays in the component, wrapped in
 * try/catch (this project has no DOM testing environment, and a walkthrough
 * that fails to persist should just show again next launch - never block
 * anything).
 */

export const ONBOARDING_STORAGE_KEY = "cip-onboarding-walkthrough-seen-v1";

export const ONBOARDING_SEEN_VALUE = "seen";

export function shouldShowWalkthrough(storedValue: string | null): boolean {
  return storedValue !== ONBOARDING_SEEN_VALUE;
}
