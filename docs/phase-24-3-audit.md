# Phase 24.3: True Dual-Tier Whisper

## Trigger

The operator picked the last of the deferred, latency/accuracy-focused
items this session had offered: "True dual-tier Whisper (fast detector
model + separate high-quality transcript model running concurrently) -
this is literally what you originally asked for as your model strategy,
and it's still not built. CIP only ever runs one Whisper model at a
time." A follow-up question ("what should happen to the quality model's
output once ready?") was answered: **also re-run Bible detection on the
corrected text**.

## What was actually missing

Confirmed by reading `apps/desktop/src-tauri/src/state.rs`/`lib.rs`:
`AppState` held exactly one `speech_engine: Mutex<Box<dyn SpeechEngine>>`,
constructed once at startup by `create_speech_engine` from
`config.whisper_model_path`. No second engine, no second model path, no
mechanism for a second, concurrently-running `WhisperContext` existed
anywhere in the codebase - this gap was explicitly flagged in
`docs/live-speech.md` and multiple release-manifest `knownLimitations`
entries across several prior phases.

## Design decision: a second, independent engine invoked only for one-shot re-decodes

Two questions had to be answered before writing any code:

### 1. How does the quality tier get the *same* audio the fast tier already decoded?

Rejected: running a second `WhisperSpeechEngine` on the same live audio
stream via its own `feed_audio`, then trying to correlate its windows
with the fast tier's by timing. Two independently-VAD-gated engines have
no guaranteed boundary alignment - this would be fragile and unverifiable
without real, timing-sensitive audio (the same reason overlapping windows
remain deferred - see `docs/phase-24-2-audit.md`).

Chosen: `WhisperSpeechEngine` already builds one exact, owned copy of the
raw i16 samples for every window it finalizes (`run_inference`, to hand to
`decode_pass`). Phase 24.3 retains that copy in a new
`last_final_window_audio: Option<Vec<i16>>` field instead of discarding
it, and adds two new `SpeechEngine` trait methods with harmless defaults:

- `take_last_final_window_audio(&mut self) -> Option<Vec<i16>>` - hands
  that exact audio to the caller, once, per finalized window.
- `transcribe_once(&self, audio: &[i16]) -> Result<Option<QualityTranscript>, SpeechEngineError>` -
  a stateless, isolated decode of `audio`, sharing `decode_pass` with the
  live buffering path (the same "never a second, fabricated inference
  path" discipline Phase 24.2 established), returning a small
  `QualityTranscript { text, confidence, language }` rather than a full
  `TranscriptSegment` - the caller already knows the timing/id, only the
  decode result is new information.

A caller (`commands::handle_audio_chunk`) takes the fast engine's
just-finalized window audio and hands it, via a bounded channel, to a
**second, genuinely separate** `WhisperSpeechEngine` instance (its own
`WhisperContext`, its own model file at
`AppConfig::whisper_quality_model_path`) running on its own dedicated
thread (`spawn_quality_worker`). The two engines never share state; the
quality engine's `transcribe_once` never touches the fast engine's buffer,
window, or clock.

### 2. What happens to the quality tier's output?

Rejected: mutating the original `TranscriptSegment`'s text in place. This
codebase already has an explicit, applied principle against it -
`pipeline.rs`'s own docs state "the transcript-and-detection record is
never edited after the fact" for `ScriptureDetection` - and doing so here
would erase what the operator actually saw live, with no record that a
correction ever happened.

Chosen: the quality tier's output becomes a **new**, ordinary final
`TranscriptSegment` (fresh id, fresh `transcript_sequence` slot, same
`start_ms`/`end_ms`/`speaker_id` as the segment it corrects), persisted
and routed through the *exact same* `finalize_bible_only`/
`finalize_and_route_segment` pipeline every other final segment already
uses. Per the operator's own answer, Bible detection **does** re-run on
this corrected text - and the existing 60-second suggestion-dedup window
(`persistence::has_recent_detection_for_reference`, same-reference +
same-category) already suppresses a redundant suggestion when the quality
tier only confirms what the fast tier found, or surfaces one when the
quality tier catches something the fast tier missed - **zero new dedup
logic was needed**. A small link table (`transcript_corrections`, new
migration 0019) records only the `original_segment_id` ->
`corrected_segment_id` relationship, so the operator-facing badge (below)
survives a restart even though this phase doesn't yet build a history/
replay view for it (an honest, documented gap - see Known Limitations).

## What changed

- `core/ai/src/speech_engine.rs`: new `QualityTranscript` struct; two new
  `SpeechEngine` trait methods (`transcribe_once`, defaulting to
  `Ok(None)`; `take_last_final_window_audio`, defaulting to `None`) - both
  no-ops for every existing engine except `WhisperSpeechEngine`.
- `ai/speech/src/whisper.rs`: `WhisperSpeechEngine` gained
  `last_final_window_audio: Option<Vec<i16>>`, populated in
  `run_inference` via `mem::replace` (preserving `self.buffer`'s
  pre-allocated capacity exactly like the `.clear()` it replaces) at the
  same point the owned `audio_f32` copy is built - before the fallible
  `decode_pass` call, matching the buffer/clock-before-fallible-decode
  invariant Phase 24.2 already established. `transcribe_once` and
  `take_last_final_window_audio` implemented; both a real, deliberate
  no-op with respect to `self.buffer`/`self.elapsed_ms`/`self.sequence`/
  `self.window_id`, so a second instance's live window is never disturbed
  by a call on it.
- `apps/desktop/src-tauri/src/config.rs`: `WHISPER_QUALITY_MODEL_FILENAME`
  (`"ggml-base.en.bin"`) and `whisper_quality_model_path` (env override
  `CIP_WHISPER_QUALITY_MODEL_PATH`), mirroring `whisper_model_path`
  exactly.
- `apps/desktop/src-tauri/src/state.rs`: `SpeechQualityDiagnostics`
  (mirrors `SpeechDiagnostics`, much smaller - job counters, not a
  per-chunk pipeline); `AppState.speech_quality_engine`/
  `speech_quality_ready`/`speech_quality_diagnostics`.
- `apps/desktop/src-tauri/src/lib.rs`: `create_quality_speech_engine`
  (mirrors `create_speech_engine`, skips the tiny-class-model accuracy
  warning - not applicable to an entirely optional second tier); wired
  into `setup`/`AppState::new`; registers `install_whisper_quality_model`.
- `apps/desktop/src-tauri/src/commands.rs`:
  - `QualityJob` (service id, original segment id, timing, speaker id,
    raw audio) and `spawn_quality_worker` (its own thread, reads jobs from
    a bounded `mpsc::sync_channel`, calls `transcribe_once`, builds and
    routes the corrected segment, persists the correction link, emits
    `AppEvent::TranscriptCorrected`).
  - `start_listening` spawns the quality channel/worker only when
    `state.speech_quality_ready` - an operator who never installs a
    quality model gets no channel, no worker, no per-window overhead at
    all, not a permanently-idle one.
  - `spawn_speech_worker`/`handle_audio_chunk` thread an
    `Option<mpsc::SyncSender<QualityJob>>` through (mirroring how
    `pending_ms`/`generation` are already threaded), and - only for a
    genuine final window - `try_send` (never blocking `send`) a job. A
    full channel (the quality worker still catching up - by design, a
    slower model) simply drops the newest job rather than ever stalling
    the fast tier or audio capture; `speech_quality_diagnostics.jobs_dropped_backlog`
    counts this honestly.
  - `install_whisper_quality_model` command mirrors `install_whisper_model`
    (real-load validation via the same `WhisperSpeechEngine::load`,
    atomic copy-then-rename install, next-launch-only activation).
  - `get_pilot_diagnostics`/`PilotDiagnostics` gained
    `whisper_quality_model`/`speech_quality` fields.
- `database/migrations/0019_transcript_corrections.sql` (new): the link
  table described above. `apps/desktop/src-tauri/src/persistence.rs`:
  `persist_transcript_correction`.
- `apps/desktop/src-tauri/src/events.rs`: `AppEvent::TranscriptCorrected`
  (`TRANSCRIPT_CORRECTED`).
- Frontend: `domain/ai.ts` gained `TranscriptCorrected`; `lib/liveEvents.ts`
  gained `onTranscriptCorrected`; `lib/commands.ts` gained
  `installWhisperQualityModel`; `config/appConfig.ts` mirrors the new
  Rust diagnostic types; `PilotDiagnosticsPanel.tsx` gained a quality-tier
  section (install button + diagnostics, following the fast tier's own
  layout); `LiveChurchBrain.tsx` tracks corrected-segment ids
  (`correctedTextBySegmentId`) via the new event and renders a small
  "corrected below" badge on the *original* transcript line - the
  correction itself already appears as its own new entry via the existing
  `onTranscriptUpdated` handler, unchanged.

## Why `TranscriptSegment` itself was never touched

An earlier design draft considered a `corrects_segment_id: Option<Uuid>`
field directly on `TranscriptSegment`. Rejected once the actual blast
radius was measured: `TranscriptSegment` is constructed as a struct
literal at roughly three dozen sites across `core/ai`, `core/intelligence`,
`ai/speech`, and nearly every `apps/desktop/src-tauri` module's own test
helpers - a required new field would have touched all of them for a
capability only two call sites (`spawn_quality_worker`'s job construction,
and the frontend badge) actually need. Keeping the correction link in its
own table (mirroring `saved_content_candidates`/
`saved_sermon_findings`'s own precedent for "an association that doesn't
belong on the primary record") kept this phase's real diagnosis - "how do
two engines share audio, and where does the second engine's output go" -
from being buried under a mechanical, unrelated refactor.

## Real-browser verification

Not performed this phase, and honestly not applicable the way Phase
24.2's was: Phase 24.2 shipped a frontend rendering change
(`interimTranscript`) that a mocked-event Playwright script could
meaningfully exercise. Phase 24.3's actual risk surface - two
`WhisperContext` instances, a second thread, a bounded channel, real
audio hand-off - is exactly the class of behavior a headless-Chromium/
mocked-Tauri-event harness cannot touch (same limitation `whisper.rs`'s
own module docs have documented since Phase 1.2: no real Whisper model is
obtainable in this container). The frontend badge/event-wiring was
proven correct only structurally (compiles, typechecks, the existing 303
frontend tests pass unchanged) - a real operator confirming the badge
appears and the corrected line reads as expected on real hardware with
both models installed is Environment C's job, not yet performed.

## Testing boundary

`transcribe_once`/`take_last_final_window_audio` are not directly unit
tested for the same reason `decode_pass`/`try_interim_decode` never were
(`docs/phase-24-2-audit.md`'s own "Testing boundary" section): they need a
real `WhisperContext`, which needs a real model file this container
cannot obtain. What *is* directly tested:

- `apps/desktop/src-tauri/src/config.rs`: two new tests proving
  `whisper_quality_model_path` defaults under `model_dir` and honors its
  env override, mirroring the fast tier's own existing tests exactly.
- `apps/desktop/src-tauri/src/persistence.rs`:
  `persists_a_transcript_correction_linking_two_already_persisted_segments`
  proves the link table round-trips both ids correctly against a real,
  migrated in-memory SQLite database.
- The `database` crate's own `applied.len() == MIGRATIONS.len()` migration
  test (unchanged in source, self-referential) already covers migration
  0019 registering and applying correctly - confirmed by running the full
  suite (368/368 `cip-desktop` unit tests, including this crate's, still
  pass after this phase, both feature configs).

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets
-- -D warnings` clean, and again with `--features whisper` on the desktop
crate - clean. `cargo test --workspace` unchanged pass counts everywhere
except the 2 new `config.rs` tests and 1 new `persistence.rs` test
(368/368 desktop-crate unit tests total, both feature configs; 31/31
`cip-ai-speech --features whisper`, unchanged - no new tests there, per
the testing-boundary note above). Frontend: `npm run typecheck` 0 errors,
`npm run lint` the same 4 pre-existing warnings (unchanged), `npm run
test -- --run` 303/303 (unchanged count - `eventNames.test.ts`'s "exactly
N events" assertion was updated from 55 to 56 to match the new
`TranscriptCorrected` event, not a new test), `npm run build` clean.

## Known limitations (honest, not deferred silently)

- **No real-hardware confirmation yet.** This entire phase - two
  concurrently-loaded Whisper models, a second thread, real audio
  hand-off, real decode timing - has never run against real microphone
  audio with two real model files installed. Everything above proves the
  code compiles, typechecks, and its pure/persistence logic is correct;
  it does not prove the quality tier's corrections are actually useful,
  or that running two models concurrently is tolerable on modest pilot
  hardware (a real, measurable CPU/memory cost - see the next point).
- **A second loaded Whisper model is a real, ongoing resource cost** -
  roughly double the RAM of whichever two model sizes are configured, plus
  whatever CPU the quality worker's own inference calls need, for the
  entire lifetime of a listening session, even during a quiet stretch
  with no jobs to process. Entirely opt-in (no quality model installed =
  zero extra cost, verified by `speech_quality_ready` gating the channel/
  worker spawn), but a real cost once an operator does install one.
- **The quality worker can fall behind and silently drop jobs** - by
  design (a slower model should never block the fast tier), but an
  operator watching `jobsDroppedBacklog` climb has no way from this UI
  alone to distinguish "the model is simply too slow for this hardware"
  from "a transient spike." `docs/phase-3-8-7-3-audit.md`'s backpressure
  work for the *fast* tier's own backlog does not apply here - the
  quality channel is deliberately much simpler (drop-newest on a small
  fixed-capacity channel, no overload-drain state machine).
- **No history/replay view of past corrections.** `transcript_corrections`
  persists the link durably, but no command/UI reads it back after the
  live session - an operator reviewing history after a service sees only
  the corrected segment's own text among the ordinary transcript, with no
  visual indication it was a correction (the live badge only exists while
  `LiveChurchBrain`'s own in-memory `correctedTextBySegmentId` map is
  populated for that session).
- **`transcribe_once`'s language conditioning always uses the quality
  engine's own `requested_language`** (defaulting to `"en"`, settable via
  the existing `set_language`) - it is never told what language the fast
  tier detected for that specific window, so a service using `"auto"`
  language detection could see the two tiers condition on different
  languages for the same audio. Not a defect this phase introduces (the
  fast tier's own per-window detected language was never threaded
  anywhere before this), but a real, undocumented-until-now gap in how
  far language auto-detection actually reaches.
- **Model file size/latency tradeoff is entirely the operator's problem to
  reason about** - `install_whisper_quality_model` validates that a file
  loads as a real Whisper model, never that it is meaningfully "better"
  than the fast tier's model or appropriately sized for the hardware
  running it (the same honest limitation `classify_model_size_tier`'s own
  docs already carry for the fast tier).

## Final gate

Environment A (fmt/clippy/test in both feature configs, frontend
typecheck/lint/test/build): PASS. Environment B (Xvfb smoke test): not
re-run this phase - no display/presentation-layer code changed, and the
prior Environment B baseline (Phase 3.7) remains the relevant proof for
that surface. Environment C (a real operator confirming the quality tier
produces useful corrections on real hardware, at an acceptable resource
cost, with real microphone audio): **not yet performed** - the most
significant open item this phase leaves, consistent with every prior
Whisper-model-dependent phase's own honest gate in this environment.
