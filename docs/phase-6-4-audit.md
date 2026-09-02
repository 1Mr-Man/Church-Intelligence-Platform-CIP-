# Phase 6.4 — Audit: Buried Audio/Speech Error Visibility

## Baseline

Phase 6.3 closed feed search. This audit opens gap #4 from Phase 6's own
audit: "buried audio/speech error visibility"
(`docs/phase-6-1-operator-ergonomics-shortcuts.md`).

## What exists today

Two independent error sources exist per engine:

- **Audio**: `AppState::audio_error` (a raw `Option<String>`, set from
  `AudioEngineError` on a synchronous `start_listening` failure -
  `commands.rs`) *and* `AudioEngineStatus::stream_error` (an async
  mid-capture failure the backend's own stream-error callback reports -
  already part of `LiveStatus.audio`, already mirrored in TypeScript as
  `AudioEngineStatus.streamError`). `get_live_status`'s `AudioStatusKind`
  enum already ORs both together to decide `Error` - but only the boolean
  presence, never either string.
- **Speech**: `AppState::speech_error` (a raw `Option<String>`, set from
  `SpeechEngineError` on a failed `feed_audio`/not-ready check) drives
  `SpeechStatusKind::Error` the same way - presence only, string
  discarded.

Operator Mode (`LiveChurchBrain.tsx`) already renders a notice when
either status is `Error` - but the text is two hardcoded, generic
strings ("AUDIO ERROR — retry below.", "SPEECH ERROR — recorded, will
clear on the next successful chunk.") that never change no matter what
actually went wrong. An operator sees *that* something is wrong, never
*what*.

Diagnostics Mode's Pilot Diagnostics panel does render real text - but
only for speech (`diagnostics.speech.lastError`,
`PilotDiagnosticsPanel.tsx:182`). `diagnostics.audio.streamError` is
already present in the exact same payload (`PilotDiagnostics.audio` is
literally `AudioEngineStatus`, so the field arrives on every poll) but
nothing in the panel renders it - a smaller, parallel gap even in the
mode that's supposed to have full detail.

`SystemStatusStrip` (both modes) shows only a red "Error" badge, no
text - correct for a compact strip, not a text-detail surface.

Real example error text (all plain, operator-readable, not raw Rust
debug output): `"no audio device available"`, `"device not found: {0}"`,
`"audio backend error: {0}"` (`AudioEngineError`); `"speech engine not
initialized"`, `"model not found: {0}"`, `"transcription failed: {0}"`
(`SpeechEngineError`).

## Design (no fork - proceeding directly)

The real text already exists, is already computed by the exact same
condition already driving the `Error` enum, and is already
operator-readable - there is no case for keeping it hidden. This is not
a genuine architectural choice the way Phase 6.2's confirm/undo split
was; it proceeds straight to implementation:

- `LiveStatus` (Rust + TS mirror) gains two additive fields,
  `audio_error_text`/`speech_error_text`, computed with the *exact same*
  source-of-truth logic already deciding `audio_status`/`speech_status`
  (`audio.stream_error` preferred, falling back to `state.audio_error`
  for audio; `state.speech_error` directly for speech) - no new state,
  no new command, no duplicated condition.
- Operator Mode's existing two notices interpolate the real text instead
  of staying generic, with the original generic phrasing kept as a
  defensive fallback if the text is ever absent.
- `PilotDiagnosticsPanel.tsx` gains one line rendering
  `diagnostics.audio.streamError` (already in the payload today),
  mirroring speech's own "Last error: ..." line - closing the smaller,
  parallel Diagnostics Mode gap at the same time, since it's the same
  underlying "an operator can't see why this failed" problem.
