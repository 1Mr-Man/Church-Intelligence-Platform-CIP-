/**
 * Phase 6.6 (Operator Ergonomics: onboarding). A dismissible, first-launch-
 * only overlay explaining the operator workflow - the last of Phase 6's
 * original 8 audit gaps to be scoped. Never blocks: it renders once,
 * closes on a single click, and never reappears unless the operator's
 * browser storage is cleared. See `lib/onboarding.ts` for the tested
 * "have we shown this before" logic this component wraps around real
 * `localStorage` access.
 */
import { useState } from "react";
import { ONBOARDING_SEEN_VALUE, ONBOARDING_STORAGE_KEY, shouldShowWalkthrough } from "../lib/onboarding";

function readStoredValue(): string | null {
  try {
    return window.localStorage.getItem(ONBOARDING_STORAGE_KEY);
  } catch {
    return null;
  }
}

function markSeen(): void {
  try {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, ONBOARDING_SEEN_VALUE);
  } catch {
    // Best-effort only - if storage is unavailable the walkthrough simply
    // shows again next launch. It never blocks anything either way.
  }
}

export function OnboardingWalkthrough() {
  const [visible, setVisible] = useState(() => shouldShowWalkthrough(readStoredValue()));

  if (!visible) return null;

  const dismiss = () => {
    markSeen();
    setVisible(false);
  };

  return (
    <div className="onboarding-overlay" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
      <div className="onboarding-modal">
        <h2 id="onboarding-title">Welcome to CIP</h2>
        <p>Here's the operator workflow, start to finish:</p>
        <ol className="onboarding-steps">
          <li>
            <strong>Start Service</strong> — begin a live service, or use Service Replay to try the whole workflow
            with a transcript instead of a live microphone.
          </li>
          <li>
            <strong>Needs Attention</strong> — as Scripture, sermon, and other findings come in, they land here for
            review, ordered by confidence.
          </li>
          <li>
            <strong>Approve or Reject</strong> — press A/R (or use the buttons) on the top item to confirm or dismiss
            it.
          </li>
          <li>
            <strong>Display</strong> — an approved Bible reference can be sent to the presentation screen with one
            click.
          </li>
        </ol>
        <p>
          Diagnostics Mode (top right) shows microphone, Whisper model, and display setup in full detail if
          something isn't working.
        </p>
        <button type="button" onClick={dismiss} autoFocus>
          Got it
        </button>
      </div>
    </div>
  );
}
