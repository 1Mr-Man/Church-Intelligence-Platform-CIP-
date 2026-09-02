/**
 * Phase 6.6 (Operator Ergonomics: onboarding). `SystemStatusStrip`
 * already surfaces Bible/Speech readiness in plain language in Operator
 * Mode, unconditionally - but an operator seeing "Speech: Optional — not
 * configured" has no in-Operator-Mode path to actually fix it; the only
 * control that does anything about it (install a Whisper model) lives in
 * Diagnostics Mode, a mode a first-time operator has no reason to know
 * exists. `computeSetupGaps` decides which of the two genuinely
 * operator-actionable setup items ("install a Bible dataset," "install a
 * Whisper model") are still outstanding, from data `LiveStatus` already
 * carries - no new command, no new fetch.
 *
 * Deliberately narrower than "everything SystemStatusStrip shows": a
 * speechStatus of `"error"` is a live, real-time failure already surfaced
 * loudly elsewhere (the Operator Mode notice, Phase 6.4's real error
 * text, the dismissible error banner) - repeating it here as a "setup"
 * item would be misleading noise, not a new fact. Only `"unavailable"`
 * (no model configured at all - the expected, unremarkable first-run
 * state) is a genuine setup gap.
 */

import type { SpeechStatusKind } from "../domain";

export interface SetupGap {
  id: "bible" | "speech";
  message: string;
}

export function computeSetupGaps(bibleInstalled: boolean, speechStatus: SpeechStatusKind): SetupGap[] {
  const gaps: SetupGap[] = [];
  if (!bibleInstalled) {
    gaps.push({
      id: "bible",
      message: "No Bible dataset installed - Scripture detection won't find any verses until one is.",
    });
  }
  if (speechStatus === "unavailable") {
    gaps.push({
      id: "speech",
      message: "Automatic transcription (Whisper) isn't set up yet - manual transcript entry still works.",
    });
  }
  return gaps;
}
