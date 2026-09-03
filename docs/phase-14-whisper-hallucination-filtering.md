# Phase 14: Whisper Hallucination Filtering & Real Confidence Scoring

## Baseline

Trigger: the first genuine Environment C evidence this project has received - two screenshots from a
real Windows laptop running the Phase 13 installer live, showing a Live Transcript panel full of
`"[BLANK_AUDIO]"`, `"(speaking in foreign language)"`, `"[inaudible]"`, and `"[LAUGHTER]"`, every line
carrying the identical `(75%)` confidence badge, 6% input level, and zero Bible detections. Full
root-cause investigation in `docs/phase-14-audit.md`.

## What was actually wrong (verified, not assumed)

1. Every confidence badge was a hardcoded `0.75` literal (`ai/speech/src/whisper.rs`), not a real
   score - its own comment claiming "no per-token confidence exposed by this API" was itself false,
   confirmed against the vendored `whisper-rs-0.14.4` source: `full_n_tokens`/`full_get_token_prob`
   are real, implemented accessors this code simply never called.
2. The bracketed/parenthesized lines are whisper.cpp's own well-documented non-speech placeholder
   captions - literal training-data artifacts, not random noise - and nothing in this codebase
   recognized them as "not real spoken content" before displaying, persisting, and feeding them into
   Bible/Sermon/Music/Content detection exactly as if the congregation had said them.
3. The audio genuinely reached whisper.cpp; Phase 5.3's `SILENCE_RMS_THRESHOLD` (1%) worked correctly
   against the reported 6% level. The problem is downstream of the VAD gate, not in it.
4. "No Bible detection" is very likely explained by 1-3, not a fifth defect - none of the twelve
   reported transcript lines contained a real, cleanly transcribed Scripture reference for the
   detector to have missed.

## What was built

- **`core/ai/src/speech_engine.rs`**: new `SpeechEngine::last_feed_was_non_speech_placeholder`
  trait method (default `false`), mirroring `last_feed_was_silence`'s exact precedent.
- **`ai/speech/src/whisper.rs`**:
  - `NON_SPEECH_PLACEHOLDERS` (a documented set of whisper.cpp's own known non-speech captions) and
    `normalize_for_placeholder_match`/`is_non_speech_placeholder` (pure, fully tested functions).
  - `run_inference` now discards a segment whose entire decoded text is one of these placeholders,
    exactly like an empty-text result already was - never a partial match, so a segment mixing real
    words with a bracketed tag is kept in full.
  - Real per-segment confidence: averages whisper.cpp's own per-token decode probability
    (`full_n_tokens`/`full_get_token_prob`) across every token in the pass, replacing the hardcoded
    `0.75` (a documented, honest fallback only if no tokens were readable at all).
  - `last_feed_was_non_speech_placeholder` field + `SpeechEngine` impl.
- **`apps/desktop/src-tauri/src/state.rs`** / **`commands.rs`**: new
  `SpeechDiagnostics.non_speech_placeholders_skipped` / `SpeechRuntimeDiagnostics.nonSpeechPlaceholdersSkipped`
  counter, incremented in `handle_audio_chunk` exactly parallel to the existing
  `silent_windows_skipped` check.
- **Frontend**: `lib/format.ts` gains `describeAudioSignal` (extracted, tested pure function) - a
  quiet-but-real signal (above the 1% silence floor, below a documented 15% "healthy" floor) now
  reads "LOW SIGNAL" with a concrete suggestion, instead of the same bare "SIGNAL CAPTURED" text used
  at any level above silence; `LiveChurchBrain.tsx` uses it; `PilotDiagnosticsPanel.tsx` surfaces the
  new placeholder-discard counter alongside the existing silence counter.

## Explicitly deferred

Software gain normalization/AGC before feeding Whisper was seriously considered and **not**
implemented - see `docs/phase-14-audit.md`'s "Explicitly deferred" section for the concrete technical
reason (per-chunk gain would apply inconsistently across one buffered ~3s Whisper window, a real risk
of new artifacts this environment has no real audio to verify against).

## Testing boundary

Everything in this phase is either a pure function (`is_non_speech_placeholder`,
`normalize_for_placeholder_match`, `describeAudioSignal`) or thin diagnostics-counter wiring that
mirrors an already-established, already-untested-directly pattern (`silent_windows_skipped` itself
has no dedicated integration test either - only the pure logic beneath it is tested, per this
project's own standing convention). 4 new Rust tests in `ai/speech/src/whisper.rs`, using the exact
strings from the real pilot screenshots as test input. 7 new frontend tests in the new
`lib/format.test.ts`, including the exact 6% level from the real report.

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both feature configs.
- `cargo test --workspace`: 1023 passed, 0 failed (unchanged from Phase 13 - these new tests are
  gated behind the `whisper` Cargo feature).
- `cargo test -p cip-ai-speech --features whisper`: 19 passed, 0 failed (up from 15 - the 4 new
  tests).
- `cargo test --features whisper` (apps/desktop/src-tauri): 350 passed, 0 failed (unchanged from
  Phase 13's whisper-feature count).
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings, unchanged) /
  `npm run test -- --run` (294 passed, up from 287 - 7 new) / `npm run build`: all clean.

## Architectural safety

- Zero new Tauri commands, zero new events, zero new migrations - this phase is entirely inside the
  existing speech pipeline and its diagnostics.
- `is_non_speech_placeholder`'s whole-string-only matching means it can never discard a segment that
  contains any real words alongside a bracketed tag - the same "never eat real content" discipline
  `SILENCE_RMS_THRESHOLD` already established.
- The real confidence score only ever *replaces* a previously-fake constant; nothing downstream that
  already consumed `TranscriptSegment.confidence` needed to change, since the field's type and range
  are unchanged.
- `core/bible`, `core/sermon`, `core/music`, `core/service`, `core/presentation` (every domain
  contract crate) are entirely untouched - this phase lives only in the speech-recognition layer.

## Known limitations (honest, not deferred silently)

- **No audio gain normalization** - see "Explicitly deferred" above. A genuinely quiet microphone
  still produces a quiet, lower-confidence transcript; this phase makes that honestly visible (LOW
  SIGNAL, a real low confidence score, a visible placeholder-discard counter) rather than fixing the
  underlying audio level.
- **The non-speech placeholder list is a documented, not exhaustive, set** - whisper.cpp can in
  principle emit other bracketed captions this list does not yet include; new ones observed in a real
  session should be added the same way these were, from direct evidence.
- **Garbled-but-real-looking hallucinated phrases** (e.g. "Peace forever for beating the against
  fire.") are not caught by the placeholder filter, since they are not one of whisper.cpp's known
  fixed captions - the real confidence score now at least reports these honestly lower than clean
  speech, but does not remove them from the transcript. Removing them would require a confidence
  *threshold* decision (hide below X%), deliberately not made in this phase without further real
  operator feedback on what threshold is actually useful in practice.
- **This exact rebuilt artifact has NOT yet been installed or launched on real Windows hardware**,
  and this phase's own fixes have not yet been re-verified against a second real pilot session - see
  `physicalHardwareStatement` item 23 in the updated release manifest.

## Final gate

Environment A (build-time verification, full regression, direct binary symbol inspection): PASS.
Environment C (a second real pilot session on the same or similar hardware, confirming the Live
Transcript panel no longer shows whisper.cpp's own placeholder captions as if they were real speech,
and that the confidence badge now genuinely varies with transcription quality): not yet performed -
carried forward into `physicalHardwareStatement` per this project's standing discipline.
