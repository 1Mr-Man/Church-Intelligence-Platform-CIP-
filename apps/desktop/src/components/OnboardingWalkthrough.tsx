/** First-run orientation for a consumer-safe install. */
import { useState } from "react";
import { ONBOARDING_SEEN_VALUE, ONBOARDING_STORAGE_KEY, shouldShowWalkthrough } from "../lib/onboarding";

function readStoredValue(): string | null {
  try { return window.localStorage.getItem(ONBOARDING_STORAGE_KEY); } catch { return null; }
}

function markSeen(): void {
  try { window.localStorage.setItem(ONBOARDING_STORAGE_KEY, ONBOARDING_SEEN_VALUE); } catch { /* best effort */ }
}

export function OnboardingWalkthrough() {
  const [visible, setVisible] = useState(() => shouldShowWalkthrough(readStoredValue()));
  if (!visible) return null;

  const dismiss = () => { markSeen(); setVisible(false); };

  return (
    <div className="onboarding-overlay" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
      <div className="onboarding-modal">
        <h2 id="onboarding-title">Welcome to CIP</h2>
        <p>Before the first live service, confirm these five things:</p>
        <ol className="onboarding-steps">
          <li><strong>Audio input</strong> — choose the microphone or mixer feed and make a short test recording.</li>
          <li><strong>Bible data</strong> — enable at least one licensed translation. Explicit references are resolved first; paraphrase matches require contextual evidence.</li>
          <li><strong>Speech model</strong> — verify that Whisper is loaded in Diagnostics before going live. If it is unavailable, use Service Replay or fix the setup before presenting.</li>
          <li><strong>Output</strong> — open the presentation display, then optionally configure OBS, vMix, or NDI. Nothing is shown publicly until an operator approves it.</li>
          <li><strong>Review and recover</strong> — findings appear in Needs Attention. Approve or reject them, and use Diagnostics if a device, model, database, or display disconnects.</li>
        </ol>
        <p>CIP keeps service data local by default and can continue offline when its bundled models and Bible data are available. Open Diagnostics Mode for the complete readiness check.</p>
        <button type="button" onClick={dismiss} autoFocus>Got it</button>
      </div>
    </div>
  );
}
