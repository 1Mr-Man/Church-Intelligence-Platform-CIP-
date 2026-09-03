/**
 * Phase 16 (operator ergonomics): whether the Live Transcript panel should
 * start collapsed. Collapsing it never pauses anything it displays from -
 * Bible/Sermon/Service/Music detection all run on the backend regardless of
 * whether this panel is rendered, so this is a pure display preference, not
 * a pipeline control. Mirrors `onboarding.ts`'s own precedent: the
 * storage-value semantics are the one piece of logic worth unit testing on
 * its own; the actual `localStorage` read/write stays in the component,
 * wrapped in try/catch (this project has no DOM testing environment, and a
 * preference that fails to persist should just default to expanded - never
 * block anything).
 */

export const LIVE_TRANSCRIPT_COLLAPSED_STORAGE_KEY = "cip-live-transcript-collapsed-v1";

export const LIVE_TRANSCRIPT_COLLAPSED_VALUE = "collapsed";

export function isLiveTranscriptCollapsed(storedValue: string | null): boolean {
  return storedValue === LIVE_TRANSCRIPT_COLLAPSED_VALUE;
}
