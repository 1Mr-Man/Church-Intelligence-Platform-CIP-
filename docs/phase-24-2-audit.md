# Phase 24.2: Real Interim Transcripts

## Trigger

The operator asked what else this session could do; offered the
detection-latency-focused deferred items from prior audits, including
"overlapping/interim transcription windows - deferred since Phase 19/21
as needing real timing-sensitive audio to validate safely." Since that
label actually covers two separable techniques with very different risk
profiles, the operator was asked which one to pursue and chose **interim
(partial) transcripts** - the lower-risk half that, unlike overlapping
windows, does not touch window boundaries or need real audio timing to
validate.

## What was actually missing

`TranscriptSegment.is_final` and the `SpeechEngine::feed_audio` trait
contract have documented interim-segment support since Phase 1.2 -
`feed_audio`'s own doc comment already says it returns "zero or more
transcript segments produced so far (interim, final, or both)". The
real-time worker in `apps/desktop-src-tauri/src/commands.rs` already
branches on `segment.is_final`, emitting `TranscriptUpdated` and skipping
the Bible Intelligence Core entirely for a non-final segment - this has
been true since that command was first written, not added this phase.
The frontend's `onTranscriptUpdated` handler already had the line
`if (!segment.isFinal) return; // interim text is not added to the
permanent feed`.

So the entire architecture already anticipated this feature end to end.
The one and only actual gap: `WhisperSpeechEngine`, the sole real
`SpeechEngine` implementation, never produced an interim segment -
`run_inference` unconditionally set `is_final: true` and only ever ran
once per ~3-second window, at the end. Nothing downstream needed to
change; only the engine itself needed to actually decode something
early.

## Design decision: one bounded, real interim decode per window

Once a window has buffered at least `MIN_VAD_FLUSH_SAMPLES` (1.5s, the
same threshold Phase 21's VAD-triggered flush already uses) without
either flush condition firing, `WhisperSpeechEngine::feed_audio` now runs
one additional, genuine whisper.cpp `full()` pass over *only the audio
buffered so far* and emits the result as an interim (`is_final: false`)
`TranscriptSegment`. At most one interim decode is attempted per window
(`window_interim_decoded`), regardless of how many more `feed_audio`
calls arrive before the window actually closes - this bounds the
worst-case extra cost to exactly one additional `full()` pass per window
(2x, never unbounded polling), and an interim attempt and a genuine VAD
flush/hard-cap flush never compete for the same window in the same call
(the interim check is the `else` branch after both flush checks have
already failed).

This is never fabricated text: it is the exact same real inference call
the final decode uses (`decode_pass`, factored out of the old
`run_inference` so both paths share one implementation), just run early
on a shorter prefix of the same buffered audio. The interim segment
shares the same `id` its window's eventual final segment will carry
(`TranscriptSegment::id`'s own documented contract), assigned the moment
a new window starts buffering (`window_id`, set in `feed_audio` the
instant a previously-empty buffer receives audio).

### Why this needed no real-audio validation, unlike overlapping windows

Phase 21's own module docs explain why audio-overlapping windows were
deferred: two overlapping windows decode a shared slice of audio twice,
and reconciling which of the two (possibly different) transcriptions of
that shared audio is correct needs token-timestamp-based stitching this
container cannot validate without real, timing-sensitive microphone
audio. Interim decoding has no such problem: it never introduces a
second decode of the *same* audio competing with another - it decodes a
strict, non-overlapping *prefix* of the one window's audio, and that
prefix's decode is always fully superseded (never merged, never
reconciled) by the final decode of the complete window. There is nothing
to stitch. This is why the technique was buildable and testable in this
container while overlapping windows still is not.

### Why buffer/clock mutation ordering had to be preserved exactly

`run_inference`'s pre-existing code always consumed `self.buffer`/
`self.elapsed_ms` *before* the fallible `state.full()` call, never after
- so a genuine whisper.cpp decode error never leaves stale audio stuck in
the buffer, retried forever on every subsequent call. The refactor into a
shared `decode_pass` free function (taking `&WhisperContext` and
`&str` rather than `&self`/`&mut self`) preserves this exactly: `run_inference`
still clears the buffer and advances the clock immediately after building
the owned `audio_f32` copy, *before* calling `decode_pass`. `try_interim_decode`
(a new code path) has no such constraint to preserve, since it never
touches `self.buffer`/`self.elapsed_ms` at all - the window is still
accumulating audio toward the final decode, and the interim peek must not
disturb it.

## What changed

- `ai/speech/src/whisper.rs`: `WhisperSpeechEngine` gained `window_id:
  Option<Uuid>` and `window_interim_decoded: bool`. The old
  `run_inference` inference body was split into a shared `decode_pass`
  (the real whisper.cpp `full()` call + result classification, taking
  plain arguments rather than `&self`) and a new `try_interim_decode`.
  `feed_audio` allocates a window id on the transition from empty to
  non-empty buffer, and - after the existing hard-cap/VAD-flush checks
  both fail - calls `try_interim_decode` when
  `should_attempt_interim_decode` (a new, small, independently-tested
  pure gate mirroring `should_flush_early`'s own style) says the window
  has enough buffered audio and hasn't already had its attempt.
  `discard_buffered_audio` also resets the new window-tracking fields.
- No changes anywhere else in the Rust workspace - `commands.rs`'s
  real-time worker, `core/ai`'s `SpeechEngine` trait, and the frontend's
  event-name wiring were all already correct for this before this phase.
- `apps/desktop/src/components/LiveChurchBrain.tsx`: a new
  `interimTranscript: TranscriptSegment | null` state, set by
  `onTranscriptUpdated` on a non-final segment (previously just
  discarded) and cleared once that window's final segment arrives or the
  service ends. Rendered as a distinct, dimmed trailing line in the Live
  Transcript panel (`.live-brain__transcript-interim`) - no
  timestamp/confidence badge, since those belong to the settled segment
  that will replace it.
- `ServiceReplay.tsx` was **not** touched: it never runs the real
  `WhisperSpeechEngine` (replay/manual-entry always construct
  `is_final: true` segments directly), so there is no interim text for
  it to ever receive.

## Real-browser verification

Headless Chromium with Tauri's own official `@tauri-apps/api/mocks`
module (`mockIPC`/`mockWindows`, `shouldMockEvents: true`) - a more
faithful mock than this session's earlier hand-rolled
`__TAURI_INTERNALS__` stub, since it drives `listen()`'s real
`plugin:event|listen`/`plugin:event|emit` registration path rather than
a bespoke callback table. Simulated the exact sequence a real window
produces: an interim `TRANSCRIPT_UPDATED` event (`isFinal: false`,
id `seg-interim-1`, "Turn with me to Romans chapter eight") followed by
that same window's final event (same id, `isFinal: true`, the completed
sentence). Confirmed:

- After the interim event: the Live Transcript panel shows a new, dimmed
  line reading "Turn with me to Romans chapter eight…" - not added to the
  permanent list.
- After the final event: the interim line is gone (count 0), and the
  permanent transcript list now shows the completed sentence with a real
  timestamp and confidence badge, exactly like every other final segment
  already did.

## Why this doesn't disturb anything downstream

- Zero new Tauri commands, zero new events, zero new migrations, zero
  schema changes.
- The Bible Intelligence Core, suggestion pipeline, and every other
  domain engine are untouched - `commands.rs`'s existing
  `if !segment.is_final { emit; continue; }` guard already kept interim
  segments away from all of them before this phase; this phase only
  causes that guard to actually receive a non-final segment sometimes.
- `run_inference`'s final-decode behavior (window boundaries, VAD gating,
  silence handling, non-speech-placeholder filtering, confidence
  scoring, language detection) is bit-for-bit unchanged - `decode_pass`
  is the same logic, only relocated so `try_interim_decode` can reuse it.
- `PresentationCard`/`PresentationDisplay`/the Bible/Sermon/Music/Service
  intelligence panels are entirely untouched.

## Testing boundary

Consistent with this project's established convention (no
`tauri::test` harness exists; a real whisper.cpp decode needs a real
model file this container cannot obtain - see `whisper.rs`'s own module
docs): `decode_pass`/`try_interim_decode`/`run_inference` themselves are
not directly unit-tested (they need a real `WhisperContext`). The new
pure gate they depend on, `should_attempt_interim_decode`, **is**
directly unit-tested (4 new tests, mirroring `should_flush_early`'s own
existing test style): fires exactly at the threshold, not before, still
fires further past it, and never fires twice in the same window
regardless of how much more audio has accumulated. All 27 pre-existing
`ai/speech` tests remain unchanged and passing.

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D
warnings` clean in both default and `--features whisper` configs,
`cargo test --workspace` 365/365 in default config and 31/31 (27 + 4 new)
in `cip-ai-speech --features whisper` plus 365/365 for the full desktop
workspace with `--features whisper` (all unchanged except the 4 new
`ai/speech` tests). Frontend: `npm run typecheck` 0 errors, `npm run
lint` the same 4 pre-existing warnings (unchanged), `npm run test --
--run` 303/303 (unchanged - no new pure-logic helper was added that
isn't already exercised), `npm run build` clean, plus the real-browser
verification above.

## Known limitations (honest, not deferred silently)

- **A mid-buffer decode has less audio context than the eventual final
  decode of the complete window**, so an interim segment's text can
  legitimately differ from (and is sometimes less accurate than) the
  final segment that later replaces it. This is inherent to decoding a
  shorter prefix, not a defect - documented directly in `whisper.rs`'s
  own module docs so a future reader (or operator reading detection
  logs) doesn't mistake a since-corrected interim guess for a persistent
  accuracy regression.
- **Interim decoding roughly doubles worst-case per-window inference
  cost** (one interim pass plus the eventual final pass, versus just the
  final pass before this phase) for any window that runs at least 1.5s
  without a detected pause. Bounded (never more than 2x, never
  unbounded), but a real, measurable CPU cost on slower hardware -
  Phase 3.8.7.2/3.8.7.3's speech-worker decoupling and backpressure work
  is what keeps this from blocking audio capture even if a device is
  genuinely slow.
- **Audio-overlapping windows remain explicitly not implemented** - this
  phase closes only the interim-transcript half of the original deferred
  item; the operator's own originally-requested "overlapping windows +
  VAD" combination is still one specific piece short (VAD-triggered
  flush shipped in Phase 21, interim decoding shipped here, true window
  overlap still deferred - see this doc's own "Why this needed no
  real-audio validation" section for exactly why that piece is harder).
- **This exact change has not been confirmed on real Windows hardware
  running the actual compiled desktop app with a real Whisper model and
  real microphone audio** - verification used a mocked Tauri event
  bridge in headless Chromium, proving the frontend rendering and event
  plumbing are correct, not that whisper.cpp's own mid-sentence decode
  quality is good enough to be useful on a real, accented voice. The
  decisive test is a real operator watching the Live Transcript panel
  during an actual sentence and confirming the interim line shown while
  they are still speaking is legible and gets corrected/completed
  smoothly once they pause.

## Final gate

Environment A (typecheck/lint/test/build, full Rust regression in both
feature configs, plus real-browser-engine event-simulation verification
of the interim-to-final transition using Tauri's own official mocking
API): PASS. Environment C (a real operator confirming interim text is
useful, not distracting, during an actual live sentence on real
hardware): not yet performed.
