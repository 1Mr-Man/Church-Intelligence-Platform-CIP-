# Phase 5.3 — Audio Pipeline Hardening: Voice Activity Detection

## Baseline

Phase 5.1 (Post-Service Observability Report) and Phase 5.2 (Temporal
Confirmation) closed the first two slices of the operator's "Reliability
& Trust" Phase 5 theme. This phase closes the third slice, named
directly from the operator's own roadmap item: *"Audio pipeline
hardening (noise suppression/AEC/VAD)."*

Auditing `WhisperSpeechEngine::run_inference` (`ai/speech/src/whisper.rs`)
found every fully-buffered ~3s audio window is unconditionally handed to
whisper.cpp's `state.full()`, whether or not it contains any actual
speech. A window of near-silence (a pause between sentences, dead air
before the service starts, a muted mic) still runs a full, expensive
inference pass - and Whisper is documented to hallucinate plausible-looking
text on silent or near-silent input rather than reliably returning empty
output. This plausibly explains some of the "poor/repetitive transcript"
symptoms only partially addressed by Phase 3.8.7.3's backpressure fix,
and wastes real inference time (up to ~15s per call on the real Windows
hardware measured in `docs/phase-3-8-7-7-audit.md`) on audio that carries
no signal at all.

## Why this phase exists

The roadmap item names three distinct technologies. This phase audits all
three and scopes to the smallest one that is both genuinely useful and
honestly buildable/testable in this container:

- **Noise suppression** (e.g. RNNoise-style spectral denoising) would
  require a new native dependency and non-trivial DSP code with no way to
  validate its actual audio-quality effect without real microphone
  recordings and a human listening to the result - not attemptable here
  with any honesty.
- **Acoustic echo cancellation (AEC)** only matters when the capture
  device's own output (e.g. speaker playback) leaks back into its input -
  a real-hardware-only failure mode with no way to construct a
  meaningful automated test for it in this container at all.
- **Voice activity detection (VAD)**, scoped to RMS-energy thresholding,
  is pure arithmetic on the sample buffer already in memory: zero new
  dependencies, a deterministic and fully unit-testable classification
  function, and a real, honest reliability improvement (fewer wasted
  inferences, fewer silence-hallucinated transcripts) available today.

VAD alone is this phase's scope. Noise suppression and AEC remain
explicitly deferred (see Known limitations/Deferred work below) - this is
a judgment call made the same way Phase 3.8.7.3's own scope was narrowed,
not a claim that the other two techniques are unnecessary.

## Architecture decisions

- **RMS-energy thresholding, not a learned or spectral VAD model**: the
  simplest classifier that can honestly be built and tested without new
  dependencies or real audio fixtures. `rms_level(samples: &[i16]) -> f32`
  computes the root-mean-square of the buffer (each sample normalized to
  `[-1.0, 1.0]`); `is_silence` compares it against a fixed threshold.
- **`SILENCE_RMS_THRESHOLD = 0.01`, deliberately conservative, not
  empirically calibrated**: set near the digital noise floor rather than
  tuned to filter quiet speech. Silently dropping real soft speech is a
  far worse failure than occasionally spending one inference on true
  silence - the same honesty discipline already applied to
  `MIN_PARAPHRASE_SCORE`/`MIN_SEMANTIC_SIMILARITY`/
  `CONFIRMATION_SCORE_BONUS`. No real-hardware microphone recording has
  ever been run through this threshold; that remains an Environment C
  gate (see below).
- **Gated inside `run_inference`, after timestamp bookkeeping, before
  whisper.cpp is ever called**: `duration_ms`/`start_ms` are computed
  first (needed for `elapsed_ms`/segment-timestamp continuity regardless
  of outcome); the VAD check then either returns `Ok(vec![])` immediately
  - having still advanced `elapsed_ms` and cleared the buffer exactly as
  the non-silent path does - or falls through to the existing
  `audio_f32`/`state.full()` path unchanged. A silent window is bookkept
  identically to an inferred one; only the expensive inference call
  itself is skipped, so no downstream segment's `start_ms`/`end_ms` is
  ever distorted by this phase.
- **A new `SpeechEngine::last_feed_was_silence()` trait method, following
  the exact pattern Phase 3.8.7.3 established for
  `last_feed_triggered_inference()`**: defaults to `false`; only
  `WhisperSpeechEngine` (the one engine with an internal buffer and a VAD
  gate) overrides it. Deliberately distinct from
  `last_feed_triggered_inference() == false`, which is also true for a
  window that simply hasn't finished buffering yet - a caller needs both
  facts to be honestly countable as separate events.
- **A new process-lifetime diagnostics counter,
  `SpeechDiagnostics::silent_windows_skipped`**, following the same
  pattern as `chunks_skipped_engine_not_ready`/`inferences_attempted`:
  incremented in `commands.rs`'s speech-worker loop whenever
  `speech.last_feed_was_silence()` reports `true`, independent of (not
  nested inside) the existing `triggered_inference` branch, since the two
  conditions are mutually exclusive per-call but structurally separate
  checks.

## What was built

- **`core/ai::speech_engine::SpeechEngine`**: new default-`false` trait
  method `last_feed_was_silence(&self) -> bool`.
- **`ai/speech::whisper`**:
  - `SILENCE_RMS_THRESHOLD: f32 = 0.01` (heavily documented as a policy
    choice, not a calibrated value).
  - `fn rms_level(samples: &[i16]) -> f32` - pure, `self`-free, so
    directly unit-testable without any `WhisperContext`.
  - `fn is_silence(samples: &[i16]) -> bool`.
  - `WhisperSpeechEngine` gained a `last_feed_was_silence: bool` field
    (initialized `false` in `load()`), set by `run_inference()` on every
    full-buffer decision (`true` on the silent path, `false` on the real
    inference path), and left unchanged by the not-yet-full-buffer path
    in `feed_audio()` (matching `last_feed_triggered_inference`'s own
    existing semantics there).
  - `feed_audio()` simplified: `run_inference()` is now solely
    responsible for setting both flags after a full buffer is reached,
    rather than `feed_audio()` pre-setting one of them.
  - Trait impl: `last_feed_was_silence(&self) -> bool { self.last_feed_was_silence }`.
  - 4 new unit tests: `silence_has_near_zero_rms`,
    `a_full_scale_tone_is_never_classified_as_silence`,
    `low_level_room_noise_just_above_the_floor_is_not_silence`,
    `rms_level_of_an_empty_buffer_is_zero_not_a_panic`.
- **`apps/desktop/src-tauri/src/state.rs`**: `SpeechDiagnostics` gained
  `pub silent_windows_skipped: u64`.
- **`apps/desktop/src-tauri/src/commands.rs`**:
  - Speech-worker loop increments `silent_windows_skipped` whenever
    `speech.last_feed_was_silence()` is `true`, checked independently of
    the existing `triggered_inference` block (both read state after the
    same `feed_audio` call, but classify mutually exclusive outcomes).
  - `SpeechRuntimeDiagnostics` (the Tauri-command-facing mirror used by
    `get_pilot_diagnostics`) gained `pub silent_windows_skipped: u64`,
    populated from the same-named `SpeechDiagnostics` field.
  - The existing `pilot_diagnostics_serializes_camel_case` test fixture
    updated with the new field.
- **Frontend**: `config/appConfig.ts`'s `SpeechRuntimeDiagnostics`
  mirror gained `silentWindowsSkipped: number`;
  `PilotDiagnosticsPanel.tsx`'s "Inferences" line now shows
  `(N windows skipped - classified as silence)` whenever the counter is
  nonzero, alongside the existing "chunks skipped - engine not ready"
  note.

## Full regression result

`cargo fmt --check`: clean. `cargo clippy --workspace --all-targets --
-D warnings`: clean under both default features and
`--features whisper,semantic-search`. `cargo test --workspace`
(single-threaded, routing around the pre-existing, unrelated
`config.rs` env-var test-parallelism flake documented since Phase 5.1):
every crate green under both feature configurations - `cip-ai-speech`
went from 7 to 11 passing tests (the 4 new VAD unit tests), every other
crate's count unchanged. Frontend: `npm run typecheck` clean, `npm run
lint` shows only the four pre-existing `set-state-in-effect`/
`only-export-components` warnings already present before this phase (all
in files this phase did not touch), `npm run test` 220/220 passing
(unchanged - this phase added no new frontend test-observable behavior
beyond the diagnostics counter display, which has no dedicated frontend
test), `npm run build` succeeds.

## Windows rebuild

See `pilot-evidence/5.3/windows/installer-contents-verification.json`
for full direct binary proof. No new native dependency was introduced -
`rms_level`/`is_silence` are pure Rust arithmetic on the same `i16`
buffer type already in scope.

## Architectural safety diff

- Zero changes to any existing command's signature.
- Zero changes to segment timestamp math (`start_ms`/`end_ms`) - the
  silent-window path advances `elapsed_ms` and clears the buffer exactly
  as the inferred-window path already did; only the `state.full()` call
  itself is skipped.
- Zero changes to `ScriptedSpeechEngine`/`NullSpeechEngine` behavior -
  both inherit the trait's default `false` for `last_feed_was_silence()`,
  unchanged.
- Zero changes to any existing diagnostics field's meaning -
  `silent_windows_skipped` is additive, never double-counted against
  `inferences_attempted` or `chunks_skipped_engine_not_ready` (the three
  are mutually exclusive per `feed_audio` call: not-yet-full-buffer,
  full-buffer-but-silent, or full-buffer-and-inferred).
- The one real behavior change: a full ~3s window whose RMS falls below
  `SILENCE_RMS_THRESHOLD` no longer reaches whisper.cpp at all, and
  therefore can never produce a hallucinated transcript segment for that
  window. This is strictly a subtraction of previously-always-run work,
  never an addition of new inference behavior.

## Environment A / B / C

- **Environment A** (this container): PASSED, fully green, as detailed
  above - including 4 new unit tests directly exercising the
  classification math (`rms_level`/`is_silence`) against synthetic
  silent, full-scale-tone, and near-threshold buffers.
- **Environment B** (Xvfb GUI reproduction): unavailable in this
  session's container, a pre-existing, already-documented limitation
  since Phase 3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, real microphone audio):
  NOT YET VERIFIED, and cannot be verified in this container -
  `WhisperSpeechEngine::load()` always fails without a real ggml/gguf
  model file (a standing limitation since Phase 1.2), and even with a
  real model, the VAD gate's actual behavior against real room
  tone/silence/quiet speech has never been exercised on real hardware.
  The decisive pending gate is the operator's own real-hardware test:
  run a live or replayed service with genuine pauses between sentences
  and confirm (a) the transcript no longer shows hallucinated text during
  silent gaps, and (b) `silent_windows_skipped` in the diagnostics panel
  rises during those gaps without ever rising during actual quiet speech.

## Known limitations

- **`SILENCE_RMS_THRESHOLD` (0.01) is a documented policy choice, not
  empirically calibrated against any real microphone recording** -
  consistent with every other confidence/detection threshold in this
  codebase. It has never been exercised against real audio in a live
  sanctuary environment; a real room's noise floor, a specific
  microphone's gain staging, or background HVAC/organ hum could all shift
  what "silence" means in practice.
- **Noise suppression and AEC remain entirely unaddressed** - this phase
  only removes wasted inference on silence; it does nothing about a noisy
  but non-silent room, or acoustic feedback from speaker output leaking
  into the capture device. Both are explicitly deferred (see below).
- **VAD is not adaptive** - a fixed threshold cannot account for a room
  whose ambient noise floor sits meaningfully above or below the chosen
  constant; a persistently noisy room could see effectively no windows
  ever classified as silent, while an unusually quiet one could
  (incorrectly, if a pastor speaks very softly) classify real speech as
  silence.
- **No visibility into which of two adjacent windows'
  `last_feed_was_silence()` value is more likely to matter** - a single
  window straddling the boundary between an ongoing sentence and a pause
  is classified purely by that window's own RMS, with no lookback/lookahead
  smoothing.
- **Every limitation already documented for Phase 3.8.6/3.8.7.x's
  Whisper engine, Phase 4.3/4.4's detection fallbacks, and Phase 5.1/5.2's
  diagnostics/confirmation still applies unchanged** - this phase adds a
  further pipeline refinement, it does not revisit or resolve any of
  them.

## Deferred work

- Empirical calibration of `SILENCE_RMS_THRESHOLD` against real
  microphone recordings from an actual sanctuary, once real operator
  feedback exists.
- Noise suppression (e.g. spectral denoising) - a substantially larger
  effort requiring a new native dependency, not attempted this phase.
- Acoustic echo cancellation - inherently a real-hardware feature with no
  meaningful way to build or validate it without a real capture/playback
  device pair.
- Adaptive/rolling-noise-floor VAD, rather than a single fixed threshold.
- Real-hardware Environment C verification against genuine live or
  replayed audio with real silent gaps.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, real microphone audio, both outside this container's reach).
This phase is a real, verifiable, fully-tested, purely subtractive
reliability refinement - it never adds new inference behavior, never
distorts existing segment timestamps, and never touches any engine other
than the one (`WhisperSpeechEngine`) that actually buffers audio.
