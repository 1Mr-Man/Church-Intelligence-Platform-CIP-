# Phase 6.4 — Operator Ergonomics: Buried Audio/Speech Error Visibility

## Baseline

Phase 6.3 closed feed search. This phase closes gap #4 from Phase 6's own
audit: "buried audio/speech error visibility" - see
`docs/phase-6-4-audit.md` for the full breakdown.

## Audit

`get_live_status`'s `AudioStatusKind`/`SpeechStatusKind` enums already OR
together the real error sources (`AudioEngineStatus::stream_error` +
`AppState::audio_error` for audio; `AppState::speech_error` for speech)
to decide `Error` - but only the boolean presence, discarding the actual
string. Operator Mode's two error notices were hardcoded, generic text
that never changed regardless of the real failure. Diagnostics Mode
rendered real text for speech only (`diagnostics.speech.lastError`) -
`diagnostics.audio.streamError` was already present in every diagnostics
poll but nothing rendered it, a smaller parallel gap in the mode meant to
have full detail. Real error text is plain and operator-readable ("no
audio device available", "model not found: ...", "transcription
failed: ..."), not raw debug output - there was no case for keeping it
hidden.

## What was built

- **`LiveStatus`** (`commands.rs` + `domain/live.ts`): two new additive
  fields, `audio_error_text`/`speech_error_text`, computed with the exact
  same source-of-truth already deciding the enum (`audio.stream_error`
  preferred, falling back to `state.audio_error`, for audio;
  `state.speech_error` directly for speech) - no new state, no new
  command, no duplicated condition.
- **`error_text_if(is_error, text)`** (new, `commands.rs`): the one rule
  both fields share - never surface text unless the status actually
  resolved to `Error`, even if a stale, not-yet-cleared string is still
  sitting in state. Pure and directly testable without a locked mutex or
  running engine.
- **`LiveChurchBrain.tsx`**: Operator Mode's two existing notices now
  interpolate the real text (`AUDIO ERROR: {text} — retry below...`),
  falling back to the original generic phrasing if the text is ever
  absent.
- **`PilotDiagnosticsPanel.tsx`**: a new "Last error: ..." line for audio
  (`diagnostics.audio.streamError`), mirroring speech's own existing
  line - the data was already in every poll, this only renders it.

## Full regression result

Backend: `cargo fmt --check` clean. `cargo clippy --workspace
--all-targets -- -D warnings` clean under both default features and
`--features whisper,semantic-search`. `cargo test --workspace`
(single-threaded, routing around the pre-existing `config.rs`
test-parallelism flake documented since Phase 5.1) fully green under
both feature configurations - `cip-desktop` went from 313 to 316 passing
tests (3 new `error_text_if` cases). Frontend: `npm run typecheck`
clean, `npm run lint` same pre-existing warnings as before this phase
(no new ones), `npm run test` 242/242 passing (unchanged count - this
phase's frontend work is rendering/interpolation, not new pure logic),
`npm run build` succeeds.

## Windows rebuild

Both Rust and frontend files changed this phase - see
`pilot-evidence/6.4/windows/installer-contents-verification.json` for
the rebuild's direct binary verification (new symbols this time, not
just a bundle-hash change, since this is the first Phase 6.x slice to
touch Rust).

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - `LiveStatus` gains two additive, read-only fields
  derived entirely from state that already existed.
- `error_text_if`'s contract (`None` unless the status is actually
  `Error`) prevents a stale error string from ever appearing next to a
  status that says everything is fine.
- Operator Mode's generic phrasing remains as a fallback - the notices
  never disappear or break if the text is ever absent, they only gain
  detail when it's present.
- Diagnostics Mode's new line renders data that was already transmitted
  on every poll - no new IPC surface, no new field on the wire.

## Environment A / B / C

- **Environment A** (this container): PASSED - full backend and frontend
  regression green as detailed above, including 3 new unit tests for
  `error_text_if` covering the not-an-error/is-an-error/error-with-no-text
  cases.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: trigger a real audio or speech error (e.g. unplug the
  microphone, or point the Whisper model path at a nonexistent file) and
  confirm Operator Mode's notice shows the actual error text, not just
  "AUDIO ERROR"/"SPEECH ERROR".

## Known limitations

- **The real text is still an internal error message, not a
  operator-facing "what to do" instruction** - e.g. "no audio device
  available" tells the operator what's wrong, not necessarily the exact
  remedy. The existing "retry below"/"will clear on the next successful
  chunk" phrasing still carries the guidance; this phase only adds the
  diagnostic detail alongside it.
- **`audio_error_text`/`speech_error_text` reflect the same
  process-lifetime persistence their underlying state fields already
  had** (`state.audio_error`/`state.speech_error` are not time-limited or
  auto-expired) - a very old, long-cleared error never leaks through
  (guarded by `error_text_if`), but a currently-`Error` status shows
  whatever text is currently set, which is exactly the intended
  behavior, not a new limitation.
- **4 more ergonomics gaps from Phase 6's own audit remain unaddressed**
  after this phase (error-banner dismiss/context, onboarding, Diagnostics
  Mode density, unified-queue Edit support) - each a candidate for a
  future Phase 6.x slice.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The remaining Phase 6 ergonomics gaps from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase surfaces
diagnostic text that was already being computed and, in one case,
already being transmitted over IPC - it introduces no new backend
surface and changes no existing command's contract beyond two additive
fields.
