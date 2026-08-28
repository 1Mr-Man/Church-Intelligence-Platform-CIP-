# Phase 3.8.5 — Real Windows Audio Capture & Live Speech Pipeline

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `bd8b135` (Phase 3.8.4, "fix white Presentation Display
  via async Windows commands")
- Working tree at start: clean

Full audit in `docs/phase-3-8-5-audit.md`, written before any code
changed, answering all 14 questions the operator's spec posed and citing
exact line numbers for the code defect it suspected.

## Why this phase exists

The operator's real Windows report: CIP launches normally, Service Replay
and manual Bible detection both work, and CIP correctly enumerates real
input devices (Microphone - Iriun Webcam; Stereo Mix - Realtek(R) Audio;
Microphone Array - Intel Smart Sound; Line In) in a device-selection UI.
But CIP reports **"SPEECH UNAVAILABLE - manual operation remains
available"**, and when the operator tried to connect CIP to audio/system
sound, **no live transcript or intelligence was produced at all** - not a
degraded/partial result, nothing.

The operator's spec named a specific suspected defect and required it be
verified by direct tracing, not assumed: *"The current `start_listening`
implementation checks `speech_engine.is_ready()` BEFORE
`audio_engine.start(...)`. If SpeechEngine is not initialized, it returns
`SpeechEngineError::NotInitialized` and never starts the audio engine."*

## Root cause

Confirmed exactly as suspected, by direct code tracing (full detail in
`docs/phase-3-8-5-audit.md` section C). Before this phase,
`start_listening` (`apps/desktop/src-tauri/src/commands.rs`) read:

```rust
if !state
    .speech_engine
    .lock()
    .expect("speech_engine mutex poisoned")
    .is_ready()
{
    return Err(log_and_return(AppError::SpeechEngine(
        cip_core_ai::SpeechEngineError::NotInitialized,
    )));
}
```

...positioned *before* the call to `state.audio_engine....start(&resolved_device_id, sink)`
later in the same function. Since no Windows artifact built in this
project's history has ever included the `whisper` feature (confirmed via
`create_speech_engine` in `lib.rs` and every prior phase's own build
logs, e.g. `pilot-evidence/3.8.4/xvfb/*.log`: `"built without the
whisper feature; live transcription is unavailable"`), this gate has
evaluated to "blocked" on **every** real Windows test performed in this
project to date - regardless of which real device the operator selected,
audio capture could never even attempt to start.

A second, independent gate existed at the frontend layer
(`apps/desktop/src/components/LiveChurchBrain.tsx` line 760): the Start
Listening button's `disabled` condition included
`status.speechStatus === "unavailable"`, so the button was inert even if
an operator somehow bypassed the backend gate.

## Why this is the wrong architecture (and how the audit proved it)

The audit did not just find the gate - it proved, from the project's own
existing documentation and code, that the gate contradicts an
architecture this codebase already committed to:

- `docs/live-speech.md` documents `get_live_status`'s `audioStatus` and
  `speechStatus` as **"four independent signals - deliberately never
  collapsed into one 'is everything OK' boolean."** Audio and speech were
  always meant to be independently observable.
- `handle_audio_chunk` (the sink every captured `AudioChunk` reaches)
  **already** handles `speech.feed_audio()` returning `Err` gracefully -
  logging it, recording `speech_error` state, and dropping just that one
  chunk. This is an existing, already-exercised Phase 1.3 "failure
  recovery" pattern, not new logic this phase had to invent.
- The `SpeechEngine` trait's own test example (`core/ai/src/speech_engine.rs`)
  demonstrates a "not ready -> graceful per-call error" contract as the
  documented, intended behavior of `is_ready() == false`, not an
  exceptional case to guard against upstream.

In other words: the consumer-degradation logic this fix depends on
already existed and was already tested. The pre-flight gate in
`start_listening` was the one place in the whole pipeline that treated
"speech isn't ready" as a reason to refuse to even try audio, contradicting
every other layer of the same codebase.

Two collateral findings came out of the same trace:

1. The acoustic/music recognition worker (`spawn_acoustic_worker`) is
   spawned in the same function body, *after* the gate - so it was
   silently blocked by the same defect, even though it has nothing to do
   with speech.
2. `input_level` (a real RMS value, already computed inside the CPAL
   stream callback and exposed all the way through
   `AudioEngineStatus`/`LiveStatus`/the TypeScript domain mirrors) was
   never rendered anywhere in the UI - so even if capture worked, the
   operator had no way to see "signal is present" independent of
   transcription.

## Fix (smallest evidence-supported change)

1. **`apps/desktop/src-tauri/src/commands.rs`** - `start_listening`: removed
   the pre-flight `speech_engine.is_ready()` early-return. Audio capture
   now starts unconditionally, subject to the existing, unchanged
   device-resolution/`AudioEngineError` handling. Speech readiness is
   checked **after** a successful `audio_engine.start()`, purely to
   decide whether `AppEvent::SpeechStarted` is recorded/emitted -
   `AppEvent::AudioStarted` is always recorded/emitted once audio genuinely
   started, and `SpeechStarted` is never fabricated when no speech engine
   is ready. `stop_listening` mirrors the identical rule for
   `AppEvent::SpeechStopped`, so the timeline never records a "stopped"
   event for something that was never honestly recorded as "started".
   Command names, parameters, and return types are byte-identical to
   before.
2. **`apps/desktop/src/components/LiveChurchBrain.tsx`** - the Start
   Listening button's `disabled` condition no longer includes
   `status.speechStatus === "unavailable"`. The SPEECH UNAVAILABLE notice
   text was reworded to clarify that live transcription will not run, but
   audio capture (input level, acoustic/music recognition) and manual
   operation both remain available.
3. Surfaced the existing `status.audio.inputLevel` value (no new
   audio-analysis mechanism - the same RMS computation `CpalAudioEngine`
   already performs) in the Audio & Speech panel while listening, so the
   operator can see **NO SIGNAL** vs **SIGNAL CAPTURED** independently of
   whether transcription is active, per the operator's explicit
   requirement.

No new Tauri command, no new event, no second audio engine, no second
speech engine, no hardcoded device, no network dependency, no automatic
model download - none of these were touched, matching every preservation
requirement in the operator's spec.

## Full regression result

Rust workspace (default features): **all suites passed, 0 failed** across
every crate (`cip_desktop_lib` 221, `cip_integration_tests` 75,
`cip_core_bible` 232, `cip_core_intelligence` 58, and all remaining
crates - see `pilot-evidence/3.8.5/automated/regression.json`). `cargo
fmt --check`, `clippy --all-targets -- -D warnings`: clean. Whisper
feature: 7 passed, 0 failed. Windows-target cross-compile check
(`cargo check --target x86_64-pc-windows-gnu`): clean; the full
`tauri build --target x86_64-pc-windows-gnu` (a stronger check - it
fully links and packages) also succeeded. Frontend: **210 passed, 0
failed** (unchanged pass count - no new tests were added this phase; the
removed disabled-condition and reordered backend event logic are not
independently unit-testable without hardware or GUI reproduction, per
this project's established "no `tauri::test` harness" convention).
`typecheck`, `build`: clean. `lint`: 0 errors, 4 pre-existing warnings
(unrelated files, unchanged from the Phase 3.8.4 baseline).

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src/components/LiveChurchBrain.tsx
FILES CREATED: docs/phase-3-8-5-audit.md,
  docs/phase-3-8-5-live-audio-pipeline.md,
  pilot-evidence/3.8.5/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
BIBLE DATABASE CHANGED: NO
INTELLIGENCE ENGINES CHANGED: NO
SERVICE REPLAY CONTRACT CHANGED: NO
TRANSCRIPT CONTRACT CHANGED: NO
TAURI COMMANDS RENAMED/REMOVED: NONE
TAURI COMMANDS ADDED: NONE
EXISTING COMMAND SIGNATURES CHANGED: NONE (start_listening and
  stop_listening's names/parameters/return types are identical; only the
  internal ordering of audio-start vs. speech-readiness-check changed,
  and only which of the already-existing events fire, never their shape)
EVENT CONTRACTS CHANGED: NONE (AudioStarted/SpeechStarted/AudioStopped/
  SpeechStopped payload shapes unchanged; confirmed via empty
  `git diff bd8b135 -- apps/desktop/src-tauri/src/events.rs apps/desktop/src/events/`)
PRESENTATION LIFECYCLE: unchanged (this phase does not touch presentation.rs,
  presentation_display.rs, or the renderer)
PERSISTENCE: unchanged
OFFLINE ARCHITECTURE: preserved (no HTTP client crate added; no network
  dependency added; no automatic model download added; confirmed via
  `cargo tree` unchanged and a manual re-read of every changed line)
NETWORK CAPABILITIES: NONE ADDED (confirmed via empty
  `git diff bd8b135 -- apps/desktop/src-tauri/capabilities/ apps/desktop/src-tauri/tauri.conf.json`)
AUDIOENGINE / SPEECHENGINE TRAITS: UNCHANGED - CpalAudioEngine,
  WhisperSpeechEngine, NullSpeechEngine, ScriptedSpeechEngine all
  untouched; no second audio engine, no second speech engine, no second
  audio meter (the existing RMS `input_level` mechanism is reused as-is)
DEVICE CONTRACT: UNCHANGED - device enumeration remains fully dynamic
  (list_audio_devices unmodified); no device is hardcoded; the device-id-
  by-name contract is unchanged
```

## Windows artifact

Rebuilt this phase - see `pilot-evidence/3.8.5/windows/` for the checksum
and `release/windows/release-manifest.json` for full provenance. SHA-256:
`70ea056359a1dbc738b9b84bd7f2e8bbe6d6a0088bb858aeac47cddaad04b813`.

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above.
- **Environment B (Xvfb)**: **NOT AVAILABLE THIS SESSION.** Under Xvfb,
  the CIP main window is created (a real native window titled "Church
  Intelligence Platform" appears), but its WebKitGTK webview never
  navigates past a generic "Could not connect to localhost: Connection
  refused" error page. Before concluding this was a genuine environment
  limitation and not a Phase 3.8.5 regression, the identical launch was
  repeated against a release binary built from the **unmodified** Phase
  3.8.4 baseline commit (`bd8b135`, via `git stash` of this phase's two
  source edits) - the exact same failure reproduced byte-for-byte,
  proving this is a property of this session's container, not of this
  phase's code. `XDG_RUNTIME_DIR`, a real session D-Bus daemon, and
  `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS` were all tried without
  success. Full investigation:
  `pilot-evidence/3.8.5/xvfb/environment-b-not-available.json`.
- **Environment C (real Windows/audio hardware)**: **NOT VERIFIED.** No
  physical Windows machine and no real audio hardware are accessible in
  this container. See `pilot-evidence/3.8.5/hardware/hardware-status.json`
  and the final gate below.

## Known limitations

- The fix is proven correct by direct code tracing against this
  codebase's own documented architecture and by a full-green automated
  regression suite - it has not been exercised end-to-end with real
  Windows audio hardware.
- This build was never compiled with the `whisper` feature (as has been
  true of every Windows artifact in this project's history), so
  `speechStatus` will still report `"unavailable"` on this build. This
  phase's fix makes audio capture (input level, acoustic/music
  recognition) work **without** transcription on such a build - live
  transcription itself still requires a real Whisper model at the
  configured path (`docs/live-speech.md`), which this phase does not add
  or download (per the operator's explicit "DO NOT download a model
  automatically" instruction).
- Environment B (Xvfb) verification of the operator-visible behavior
  (button no longer disabled; input-level text; the start-listening
  attempt reaching `audio_engine.start()`) could not be performed this
  phase due to a session/container limitation unrelated to this phase's
  code (see above). The real Windows re-test remains the only path to
  genuine confirmation.

## Deferred work

Real Windows re-test with real audio hardware (the hard blocker for
PASS, per the operator's own instruction); installing a real local
Whisper model on a test machine to exercise the transcript/Bible-
detection/Sermon-Intelligence chain end-to-end; the full aspirational UX
redesign (still deliberately out of scope, unchanged from prior phases).

## Final gate

Per the operator's own instruction: *"Do not mark the live-audio pipeline
PASS unless real Windows audio has actually produced evidence."* That
real Windows/real-audio test has not occurred in this session. All 12
final-gate fields (WINDOWS_DEVICE_ENUMERATION, AUDIO_DEVICE_SELECTION,
AUDIOENGINE_START, CPAL_STREAM, INPUT_LEVEL, AUDIO_CHUNK_FLOW,
WHISPER_INITIALIZATION, SPEECH_FEED, TRANSCRIPT, BIBLE_DETECTION,
SERMON_INTELLIGENCE, OFFLINE_OPERATION) are recorded as `NOT_VERIFIED` in
`pilot-evidence/3.8.5/windows/windows-offline-checklist.json`.

**Phase 3.8.5: NOT PASS - Environment C not verified.** Phase 3.9 is not
started automatically.
