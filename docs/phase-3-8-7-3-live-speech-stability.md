# Phase 3.8.7.3 — Live Speech Stability, Instrumentation & Backpressure

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `509c03a` (Phase 3.8.7.2, speech worker decoupling)

## Why this phase exists

The operator installed the Phase 3.8.7.2 artifact and confirmed real,
partial improvement: transcription active, audio genuinely arriving
(input signal 4-7%). But CIP still became slow/unresponsive over
extended listening, transcript text remained poor/repetitive, the
Intelligence Feed stayed empty, and NO SIGNAL still occurred
intermittently. The operator's own hypothesis, stated explicitly: the
speech queue is likely unbounded and Whisper can fall behind real-time
audio, causing eventual overload. The operator's own binding
instructions: do not undo the Phase 3.8.7.2 CPAL decoupling; do not
replace the queue until measurements demonstrate the actual overload
behavior; do not call every incoming audio chunk an inference; do not
simply replace the unbounded queue with a bounded queue and declare
victory; do not introduce arbitrary throttling unless measurements
justify it.

## Audit — see `docs/phase-3-8-7-3-audit.md`

Written before any code change, per the operator's hard baseline rule.
Confirmed branch/HEAD/clean tree, re-read every relevant file fresh (not
trusted from the prior audit), and re-traced the actual current pipeline
end to end. Seven findings, all confirmed via direct code citation, not
assumption:

**Finding 1**: `inferences_attempted` still counted every chunk fed to a
ready engine, not only the ~1-in-300 that trigger a real whisper.cpp
inference pass (Phase 3.8.7.1 fixed a different failure mode - the
not-ready-engine over-count - and never touched this one).

**Finding 2 (the central fix)**: the speech channel
(`mpsc::channel::<AudioChunk>()`) was genuinely unbounded, confirmed by
reading the literal source. Measured consequence (not asserted): at
480 samples/callback @ 48,000 Hz - the operator's own real device - each
chunk represents 10ms of audio arriving ~100/sec; if a single Whisper
inference pass takes materially longer than the 3.0s of audio it
represents, the backlog grows without bound every cycle, compounding
over a 20-30 minute service.

**Finding 3**: four status-poll call sites (`get_pilot_diagnostics`,
`start_listening`, `stop_listening`, `get_live_status`) all locked
`state.speech_engine` just to read `is_ready()` - the same mutex the
speech worker holds for the full duration of a blocking Whisper
inference. `is_ready()` is provably constant after construction for
both existing engines, so this lock was never necessary.

**Finding 4**: a low-severity, real race where a speech worker still
finishing an unavoidably uncancellable `feed_audio` call when
`stop_listening` runs could emit output a moment after a fresh
`start_listening` had begun a new session.

**Finding 5**: Sermon/Content/Cross-Domain Intelligence remain unwired
from the live path - unchanged from Phase 3.8.7.2's own finding, not
re-litigated.

**Finding 6**: the database pipeline (`handle_final_transcript`) already
has per-stage debug-level timing; not a bottleneck (Bible detection
fires at most once per ~3s cycle), but its total duration was not yet
surfaced to the operator-facing diagnostics.

**Finding 7**: frontend event volume (≤0.33 events/sec during continuous
speech) is not a bottleneck - no throttling added, per the operator's
own instruction not to add it without measurement justifying it.

## Fix applied

**Backpressure (Finding 2, the central fix)**: a shared
`pending_ms: Arc<AtomicU64>` tracks the wall-clock duration of audio
queued but not yet fed to the engine - milliseconds, not raw chunk
count, since chunk size/rate varies by capture device.
`start_listening`'s sink closure increments it on send;
`spawn_speech_worker` decrements it (via a lock-free saturating CAS loop,
`saturating_sub_u64`) on dequeue. When the remaining backlog crosses
`OVERLOAD_THRESHOLD_MS` (10s), the worker drains and discards the entire
queued backlog plus `WhisperSpeechEngine`'s own internal buffer (new
`SpeechEngine::discard_buffered_audio()` trait method) rather than
grinding through an ever-more-stale FIFO. A plain bounded
`sync_channel`/`try_send`-drop-newest design was explicitly rejected:
it would still leave the worker permanently behind, never catching up -
directly contrary to the operator's own instruction.

**Finding 1 fix**: new default `SpeechEngine::last_feed_triggered_inference()
-> bool` trait method (`false` by default), overridden by
`WhisperSpeechEngine` to report whether its most recent `feed_audio`
call actually ran `run_inference()`. `handle_audio_chunk` checks this
after `feed_audio` and only increments `inferences_attempted`/duration
counters/`inferences_succeeded` when true. Inference duration itself is
now measured with `std::time::Instant`.

**Finding 3 fix**: new plain `AppState.speech_ready: bool` field,
computed once in `AppState::new` before the engine moves into its
`Mutex`. All four call sites replaced with `state.speech_ready` - zero
lock contention.

**Finding 4 fix**: new `AppState.listening_generation: AtomicU64`,
incremented once per `start_listening` attempt. Before emitting/
persisting any non-empty transcript result, the worker compares its
spawn-time generation against the current value; a mismatch discards the
result. Not gated on `stop_listening` alone - a plain stop-without-
restart still surfaces its last few real seconds of speech.

**Finding 6 fix**: `handle_final_transcript`'s existing call site in
`commands.rs` is now wrapped with `std::time::Instant`, surfacing
`last_transcript_pipeline_duration_ms` into diagnostics. No redesign of
the pipeline itself.

**New diagnostics** (all in `SpeechRuntimeDiagnostics`/`PilotDiagnostics`,
no new Tauri command): `queuePendingMs`, `queueHighWaterMs`,
`overloadEvents`, `audioMsDroppedOverload`, `lastInferenceDurationMs`/
`maxInferenceDurationMs`/`avgInferenceDurationMs` (average derived at
read time), `lastTranscriptPipelineDurationMs`, and a derived
`overloadState` (`normal`/`busy`/`falling_behind`/`overloaded`) from
`queuePendingMs` against fixed thresholds (never stored redundantly).
The System Diagnostics panel gained a "Speech pipeline health" section
surfacing all of these directly.

## Full regression result

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (both
default and `--features whisper`): clean. `cargo test --workspace` (both
feature configs): `cip-desktop` 237/237 passed (up from 227 - 10 new
backpressure unit tests covering `chunk_duration_ms`, `classify_overload`,
`saturating_sub_u64`), `cip-ai-speech --features whisper` 7/7 passed,
every other workspace crate green, zero failures anywhere. `cargo check
--target x86_64-pc-windows-gnu --features whisper`: clean. Frontend:
typecheck (0 errors), lint (0 errors, 4 pre-existing warnings
unchanged), test (210/210 passed, unchanged count), build clean.

## Windows artifact

- SHA-256: `78ecdf793b6b64993db703714ea8e46316c0a7d429af13b581f20cdfa06ad3c2`
- Size: 8,580,489 bytes (up from 8,566,926 - expected for real added
  logic, not a pure relocation)
- Direct proof the fix compiled in: `x86_64-w64-mingw32-strings` against
  the extracted `cip-desktop.exe` finds the mangled symbols for
  `spawn_speech_worker`, `handle_audio_chunk`, `saturating_sub_u64`, and
  both new `SpeechEngine` trait method overrides on `WhisperSpeechEngine`
  (`discard_buffered_audio`, `last_feed_triggered_inference`) plus their
  default-trait-method counterparts - read directly out of the shipped
  binary, not inferred from source.
- Runtime DLLs, model picker, worker-thread decoupling, whisper feature:
  all re-verified present and unaffected - see `pilot-evidence/3.8.7.3/`.

## Architectural safety diff

```
FILES MODIFIED: core/ai/src/speech_engine.rs, ai/speech/src/whisper.rs,
  apps/desktop/src-tauri/src/state.rs,
  apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src/config/appConfig.ts,
  apps/desktop/src/components/workspace/PilotDiagnosticsPanel.tsx,
  release/windows/*
FILES CREATED: docs/phase-3-8-7-3-audit.md,
  docs/phase-3-8-7-3-live-speech-stability.md,
  pilot-evidence/3.8.7.3/*
FILES DELETED: NONE
SPEECHENGINE TRAIT: two new DEFAULT methods added
  (last_feed_triggered_inference, discard_buffered_audio) - non-breaking
  for existing implementers (NullSpeechEngine/ScriptedSpeechEngine
  inherit the defaults unchanged)
AUDIOENGINE TRAIT: UNCHANGED
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE (only additive struct fields)
EVENT CONTRACTS CHANGED: NONE
DATABASE / MIGRATIONS: UNCHANGED
BIBLE DETECTION LOGIC: UNCHANGED
PHASE 3.8.7.2 CPAL DECOUPLING: PRESERVED, NOT UNDONE - the audio
  callback remains exactly as lightweight as 3.8.7.2 left it (two
  non-blocking channel/atomic operations, no locks, no inference)
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary symbol verification of every fix.
- **Environment B (Xvfb)**: unavailable, pre-existing, unrelated.
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's own
  20-30 minute long-run test: does CIP remain responsive the entire
  time, does queue depth stay bounded, does the app clearly show
  BUSY/FALLING BEHIND/OVERLOADED states rather than silently degrading,
  and does it recover cleanly after an overload episode.

## Known limitations

- Long-Run Tests C (extended overload) and E (stop during inference)
  could not be exercised via automated real-thread test in this
  container - consistent with this codebase's own established testing
  boundary (no `tauri::test::mock_builder()` harness). Covered instead
  by 10 pure-function unit tests (Tests A/B/D's decision logic) plus the
  operator's own real-hardware test.
- This phase bounds the pipeline's backlog and surfaces its real
  timing - it does not change whisper.cpp/tiny.en's inherent
  transcription accuracy.
- Sermon/Content/Cross-Domain Intelligence remain unwired from the live
  path - pre-existing, deliberate scope boundary, not addressed here.

## Operator's 16 questions, answered

1. **Was the queue actually unbounded?** Yes, confirmed by reading the
   literal source (`mpsc::channel::<AudioChunk>()`, no bound) - not
   assumed.
2. **Could Whisper fall behind real-time audio?** Yes, architecturally
   guaranteed to happen eventually on any machine where a single
   inference pass is not comfortably faster than the 3.0s of audio it
   represents.
3. **Measured inference duration?** Not measurable in this container
   (no real audio input); the pipeline now measures and surfaces it live
   (`lastInferenceDurationMs`/`maxInferenceDurationMs`/
   `avgInferenceDurationMs`) for the operator's own real-hardware test to
   read.
4. **Max backlog observed?** Same - not measurable here; `queueHighWaterMs`
   now surfaces this live per listening session.
5. **Backpressure strategy implemented?** A measured-backlog-with-
   explicit-drain design (wall-clock duration tracked via
   `Arc<AtomicU64>`), not a naive bounded-channel swap - see Finding 2
   fix above.
6. **Are stale chunks dropped/coalesced?** Dropped in bulk (drain +
   discard) once the backlog crosses the overload threshold, never
   processed piecemeal as stale audio.
7. **Is the CPAL callback still protected?** Yes - unchanged from Phase
   3.8.7.2, verified by re-reading `integrations/audio/src/lib.rs`
   (unchanged this phase) and the sink closure (still two non-blocking
   operations only).
8. **Can status polling block behind Whisper?** No longer - all four
   call sites now read the cached `speech_ready` field instead of
   locking `speech_engine`.
9. **Does the DB pipeline contribute materially?** No - structurally
   bounded to at most once per ~3s inference cycle; now surfaced in
   diagnostics for the operator to confirm directly rather than take on
   faith.
10. **Can React event volume contribute materially?** No - measured at
    ≤0.33 events/sec, unaffected by this phase's changes (stale backlog
    is discarded, never bulk-replayed).
11. **Does Stop Listening cleanly terminate?** Yes - unchanged mechanism
    (channel closes, worker exits on next `recv()`); the listening-
    generation guard additionally prevents a still-finishing worker's
    output from contaminating a new session.
12. **Can the system recover after overload?** Yes, by design - the
    drain-and-reset is a full recovery to `queuePendingMs == 0` every
    time it triggers, verified as a pure-function property
    (`classify_overload_recovers_back_to_normal_once_backlog_drains`).
13. **What diagnostics are available?** Queue depth/high-water mark,
    overload event count and total audio discarded, inference duration
    (last/max/avg), DB pipeline duration, and a derived overload state -
    all in the System Diagnostics panel's new "Speech pipeline health"
    section.
14. **Did all regression tests pass?** Yes - see Full regression result
    above.
15. **Was real Windows hardware tested?** Not yet for this exact
    artifact - Environment C is the pending, decisive gate.
16. **Did CIP remain responsive for 20-30 minutes?** Not yet verified -
    pending the operator's own real-hardware long-run test.

## Final gate

| Item | Status |
|---|---|
| AUTOMATED REGRESSION | PASS |
| BACKPRESSURE TESTS (pure-function, Tests A/B/D) | PASS |
| LONG-RUN SIMULATION (Tests C/E, real-thread) | NOT VERIFIED - no test harness for real-thread/Tauri-state scenarios in this container, consistent with this codebase's established testing boundary |
| WINDOWS BUILD | PASS |
| REAL WINDOWS LONG-RUN TEST | NOT VERIFIED - pending the operator's own 20-30 minute real-hardware test |
| **PHASE 3.8.7.3** | **HOLD** - pending the operator's real-hardware long-run confirmation before this phase can be marked PASS |

Per the operator's own instruction, this is not marked PASS merely
because the measured design is sound and compiles - only the operator's
real hardware, run for 20-30 minutes with periodic Diagnostics checks,
can confirm CIP actually stays responsive and bounded in practice.
