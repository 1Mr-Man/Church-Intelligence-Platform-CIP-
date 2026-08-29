# Phase 3.8.7 — Audit: Real Whisper Runtime Failure Investigation

## Trigger

The operator installed and launched the Phase 3.8.6.1 artifact (SHA-256
`b737f72b...`) on their real Windows machine. Result:

- ✅ No `libstdc++-6.dll` error - Phase 3.8.6.1's fix holds.
- ✅ Live Service UI opened, `Stereo Mix (Realtek(R) Audio)` selectable.
- ✅ Real audio capture confirmed: `SIGNAL CAPTURED`, input level 3%-6%.
- ❌ Transcription fails: `SPEECH ERROR - recorded, will clear on the
  next successful chunk.`

This closes Phase 3.8.6.1 (see its own doc's updated final gate) and
opens Phase 3.8.7. The operator's own instruction is explicit: **do not
guess again** - collect the exact underlying Whisper error from the real
Windows installation before proposing any fix.

## Audit question

Before writing any new code: does CIP already have the instrumentation
needed to answer "which exact stage failed", or does Phase 3.8.7 first
need to build that instrumentation?

## Finding: the required diagnostics already exist (built in Phase 3.8.6)

Mapping the operator's own requested field list (their message, section
1) onto what `get_pilot_diagnostics` (`apps/desktop/src-tauri/src/commands.rs:361`)
already returns:

| Operator asked for | Already exposed as | Source |
|---|---|---|
| Whisper feature enabled? | `speech.featureCompiled` | `SpeechRuntimeDiagnostics` |
| Speech engine initialized? | `speech.modelLoadAttempted` + `speech.engineReady` | same |
| Model path | `whisperModel.path` (or `.expectedPath` if missing) | `WhisperModelDiagnostic` |
| Model exists? | `whisperModel.status` (`missing` / `unreadable` / `present`) | same |
| Model size | `whisperModel.sizeBytes` (when `present`) | same |
| Model loading status | `speech.modelLoaded` | `SpeechRuntimeDiagnostics` |
| Last underlying speech error | `speech.modelLoadError` (load-time) and `speech.lastError` (runtime/inference-time) | same |
| Audio chunks received | `speech.chunksReceived` | same |
| Audio chunks resampled | `speech.lastResampledSampleCount` (last chunk only - see gap below) | same |
| Inference attempts | `speech.inferencesAttempted` | same |
| Successful inferences | `speech.inferencesSucceeded` | same |
| Failed inferences | derivable: `inferencesAttempted - inferencesSucceeded` (see gap below) | same |

All of this is already rendered in the running app's own UI - the
"Diagnostics" toggle in the Live Service screen's header renders
`PilotDiagnosticsPanel`, including a dedicated "Whisper diagnostics"
block with every field above
(`apps/desktop/src/components/workspace/PilotDiagnosticsPanel.tsx:92-123`).
**No new Rust or TypeScript code is required to collect this evidence -
it already exists in the artifact the operator has installed.**

## Two real, minor gaps found (not blocking, noted for completeness)

1. **`inferences_failed` has no dedicated counter.** It's arithmetically
   derivable from the two counters that do exist, so this doesn't block
   diagnosis, but a dedicated field would make the panel's own display
   clearer. Deferred - not needed to read the real error.
2. **`SpeechEngineError` has only three variants**
   (`core/ai/src/speech_engine.rs:39-45`): `NotInitialized`,
   `ModelNotFound(String)`, `TranscriptionFailed(String)`. Four distinct
   real failure points inside `WhisperSpeechEngine`
   (`ai/speech/src/whisper.rs`) - `WhisperContext::new_with_params`
   (context/library init), `ctx.create_state()`, `state.full()`
   (inference), `state.full_n_segments()` - all map to the same
   `TranscriptionFailed` variant (lines 66, 94, 104, 108). This is
   **not** the granular `MODEL_FILE_UNREADABLE` /
   `WHISPER_LIBRARY_INIT_FAILED` / `WHISPER_MODEL_LOAD_FAILED` /
   `WHISPER_CONTEXT_INIT_FAILED` taxonomy described in the operator's
   earlier Phase 3.8.6 spec. However: each call site passes
   `e.to_string()` from whisper-rs's own distinct `WhisperError` value,
   so the *text* surfaced in `speech.lastError`/`speech.modelLoadError`
   still differs per real failure - the Rust *type* is coarse, but the
   *message* is not. **Deliberately not fixed yet**: restructuring this
   enum now, before seeing which of these four call sites is actually
   failing, would be exactly the kind of guess the operator said to
   stop making. Once the real error text is in hand, if it turns out
   ambiguous, splitting this enum is a small, well-scoped follow-up.

## What this audit concludes

No code changes are needed to proceed with Phase 3.8.7's investigation.
The single blocking dependency is data that only exists on the
operator's own real Windows machine: the exact value of every field
above, read at the moment `SPEECH ERROR` is showing, plus the
application log covering that moment.

## Evidence-collection protocol (for the operator to run)

1. **Reproduce the error** exactly as before (select `Stereo Mix`,
   start listening, wait for `SPEECH ERROR` to appear).
2. **Open Diagnostics**: click the **Diagnostics** toggle at the top of
   the Live Service screen (next to **Operator**). This renders the
   Pilot Diagnostics panel live from the running process - no restart
   needed, no separate tool required.
3. **Copy the entire "Whisper diagnostics" block**, plus the "Whisper
   model" row directly above it (shows the exact configured path, and
   whether a file exists there / its size). Every field in the table
   above will be visible there.
4. **Retrieve the application log** at:
   `%APPDATA%\org.churchintelligence.cip\logs\Church Intelligence Platform.log`
   (this exact path is derived from this project's own logging setup -
   `tauri_plugin_log::TargetKind::LogDir` in
   `apps/desktop/src-tauri/src/lib.rs:129`, with the product name from
   `tauri.conf.json`, not a guess). Copy the lines from immediately
   before/after the `SPEECH ERROR` appeared - look for `target="cip::speech"`.
5. **Send both** (the diagnostics panel's text/screenshot and the log
   excerpt) back for the next phase's actual root-cause fix.

## One honest, evidence-based hypothesis (not a conclusion)

Phase 3.8.6's own `modelPackagingStatement` already established, from
this build environment's own confirmed-blocked network egress, that
**no Whisper model file has ever been bundled with any CIP installer**,
including this one. Unless the operator has separately placed a real
`ggml-tiny.en.bin` (or equivalent) at
`%APPDATA%\org.churchintelligence.cip\models\` on their machine, the
single most likely explanation for `SPEECH ERROR` is
`WhisperModelDiagnostic::Missing` / `SpeechEngineError::ModelNotFound` -
i.e., the whisper feature and code path are both genuinely working (as
this phase's audit confirms), but there is simply no model file to
load. This is stated as a hypothesis to check first, not a fix - the
Diagnostics panel's "Whisper model" row will confirm or rule this out
in seconds, and if a model file *is* present, the real error moves the
investigation to a different, more interesting failure mode (invalid
model content, context init failure, or an actual inference/threading
error).

## Final gate

| Item | Status |
|---|---|
| Diagnostic infrastructure audited | DONE - confirmed sufficient, no gaps block diagnosis |
| Real diagnostics collected from operator's machine | **PENDING** - the decisive next step |
| Real application log collected | **PENDING** |
| Exact failure category identified | **PENDING** - blocked on the above |
| Fix implemented | **NOT STARTED** - no guessing before real evidence, per the operator's explicit instruction |

Phase 3.8.7 does not proceed to a fix until the operator's diagnostics
and log evidence are in hand.
