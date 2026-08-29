# Phase 3.8.7.3 — Audit: Live Speech Stability, Instrumentation & Backpressure

Written before implementation, per the operator's own hard baseline
rule.

## Baseline (confirmed directly, not assumed)

- Branch: `claude/cip-foundation-init-i85g87` (confirmed via `git branch --show-current`)
- HEAD: `509c03a87c921d4bf068f3e78586c970be53db31` (confirmed via `git rev-parse HEAD` - matches Phase 3.8.7.2's own commit)
- Working tree: clean (confirmed via `git status --porcelain`, 0 lines)

## Documents/code read fresh for this audit (not trusted from memory)

`docs/phase-3-8-7-2-audit.md` and `docs/phase-3-8-7-2-real-time-speech-performance.md`;
`core/ai/src/speech_engine.rs` (the `SpeechEngine` trait in full);
`ai/speech/src/whisper.rs` (`WhisperSpeechEngine`'s buffering/inference
split in full); `integrations/audio/src/lib.rs` (already fully read in
Phase 3.8.7.2, re-confirmed unchanged); `apps/desktop/src-tauri/src/commands.rs`'s
`start_listening`, `spawn_speech_worker`, `handle_audio_chunk`,
`stop_listening`, `get_live_status`, `get_pilot_diagnostics` - every
`.speech_engine` call site re-grepped fresh (5 total: line 472
`get_pilot_diagnostics`, 1003 `start_listening`, 1051 `stop_listening`,
1280 `handle_audio_chunk`, 5068 `get_live_status`).

## Current pipeline, re-traced from actual code (not assumed correct from the prior audit)

```
cpal real-time callback (integrations/audio/src/lib.rs:327, unchanged since 3.8.7.2)
    -> sink(AudioChunk) - non-blocking since 3.8.7.2:
        acoustic_tx.try_send(chunk.clone())   [bounded, best-effort]
        speech_tx.send(chunk)                 [UNBOUNDED std::sync::mpsc::channel()]
            -> spawn_speech_worker's loop: while let Ok(chunk) = rx.recv()
                -> handle_audio_chunk (commands.rs:1275)
                    -> speech_engine.lock() HELD across the whole call below
                    -> is_ready() check (fast path if not ready)
                    -> resample_pcm16 (cheap)
                    -> diag.inferences_attempted += 1  <-- STILL WRONG, see Finding 1
                    -> speech.feed_audio(...)
                        -> ai/speech/src/whisper.rs: buffer.extend_from_slice;
                           only if buffer.len() >= CHUNK_SAMPLES (3.0s @ 16kHz)
                           does it call run_inference() (the real, blocking
                           whisper.cpp state.full() call) - otherwise
                           returns Ok(vec![]) immediately, cheaply
                    -> handle_final_transcript (pipeline.rs) on final segments
                    -> emit(TranscriptUpdated / ScriptureDetected / SuggestionCreated)
```

## Finding 1 (confirmed defect, NOT fixed by Phase 3.8.7.1/3.8.7.2): the inference-attempted counter still counts every ready-engine chunk, not every real inference

Phase 3.8.7.1 fixed the "engine not ready" over-counting
(`chunks_skipped_engine_not_ready`), but `handle_audio_chunk` (commands.rs:1343,
current code) still does `diag.inferences_attempted += 1;`
**unconditionally for every chunk fed to a ready engine** - including
the ~299 out of every ~300 calls where `WhisperSpeechEngine::feed_audio`
only appends to its internal buffer and returns immediately, without
ever calling `run_inference()`/whisper.cpp's `full()`. This directly
violates the operator's own instruction ("a Whisper inference should
only increment the inference counter when Whisper inference actually
begins") and was not caught by Phase 3.8.7.1 because that phase's fix
targeted a different failure mode (not-ready engine), not this one
(ready engine, buffering-only calls).

**Fix**: extend `SpeechEngine` with a new default-`false` method
`last_feed_triggered_inference(&self) -> bool`, overridden by
`WhisperSpeechEngine` to report whether its most recent `feed_audio`
call actually ran `run_inference()`. `handle_audio_chunk` checks this
*after* calling `feed_audio`, only incrementing
`inferences_attempted`/timing/`inferences_succeeded` when true.

## Finding 2 (confirmed hypothesis): the speech channel is genuinely unbounded

`start_listening` (commands.rs:957, current code): `let (speech_tx,
speech_rx) = mpsc::channel::<AudioChunk>();` - plain
`std::sync::mpsc::channel`, no bound. Confirmed by reading the literal
source, not inferred. If the speech worker ever falls behind real-time
audio arrival (inference duration exceeding the ~3s of audio each
inference window represents), this channel accumulates `AudioChunk`s
without limit: unbounded memory growth, and - worse for the operator's
actual symptom - the worker keeps processing an ever-growing FIFO
backlog of increasingly stale audio, falling further behind
indefinitely rather than ever catching up, exactly matching "CIP
becomes slow/freezes/blank after listening runs for an extended
period."

**Measured consequence, not asserted**: at 480 samples/callback @
48,000 Hz (this operator's own real device), each `AudioChunk`
represents 10ms of audio and arrives roughly every 10ms (~100/sec).
Each chunk is a small `Vec<i16>` (480 × 2 bytes = 960 bytes) plus a
`u32` - call it ~1KB per queued chunk including allocator overhead.
Whisper's own inference window is 3.0s of audio; if a single
`state.full()` call over a modest CPU takes materially longer than 3.0
wall-clock seconds to process 3.0 audio-seconds (very plausible for
`tiny.en` on an average church-office PC, especially with OBS/vMix/
PowerPoint also running), the backlog grows by the excess duration on
every single inference cycle, compounding without bound over a 20-30
minute service - this is the exact overload scenario the operator's
real hardware exposed, and it is architecturally guaranteed to happen
eventually on any machine where inference is not comfortably faster
than real-time, regardless of how much RAM is available.

**Fix**: track backlog not by raw chunk count but by **milliseconds of
queued audio** (an `AtomicU64`, incremented by the producer on send,
decremented by the worker on dequeue - see Part 3 below). When the
worker's own read of the backlog crosses an explicit overload
threshold, it drains and discards the *entire* backlog (never
processes stale audio) and resets `WhisperSpeechEngine`'s own
accumulation buffer (a new `discard_buffered_audio()` trait method, see
below) so the next real chunk starts a clean window - never splicing
fresh audio onto minutes-old buffered samples. This directly
implements the operator's own required outcome ("prefer real-time
audio over processing audio that is minutes old") and keeps memory
provably bounded: the queue can never hold more than
`overload_threshold_ms` worth of audio before being drained.

Chosen over a plain bounded `sync_channel`/`try_send`-drop-newest
design (Option A alone) because that would leave the worker forever
walking through an ever-more-stale FIFO once full, still falling
further behind rather than ever catching up to live audio - the
operator's spec explicitly warns against exactly this ("do not simply
replace the unbounded queue with a bounded queue and declare victory").
This is Option B/C combined (coalesce by discarding stale backlog in
bulk + resume from a fresh rolling window), the one that actually
satisfies "never process audio that is minutes old."

## Finding 3 (confirmed, still present after 3.8.7.2): status-poll commands still lock the same mutex the speech worker holds during inference

`get_pilot_diagnostics` (commands.rs:472), `start_listening` (:1003),
`stop_listening` (:1051), and `get_live_status` (:5068) all call
`state.speech_engine.lock()...is_ready()`. Phase 3.8.7.2 moved the
*work* off the audio callback thread, but did **not** remove the lock
contention for these *other* threads: `handle_audio_chunk` (now running
on the dedicated speech worker thread) still acquires
`state.speech_engine.lock()` and holds it for the full `feed_audio`
call, including the blocking `state.full()` inference. Any Tauri
command thread trying to lock the same mutex - including
`get_live_status`, which the frontend polls every 3000ms
(`LiveChurchBrain.tsx:49`, unchanged) - blocks until that inference
finishes. This is the mechanical explanation for status/UI staleness
persisting even after 3.8.7.2's fix, exactly matching operator concern
#6.

**Key fact making the fix trivial**: `is_ready()` never changes after
construction for either engine that exists today -
`WhisperSpeechEngine::is_ready()` always returns `true` (hardcoded,
`ai/speech/src/whisper.rs:145-147`) and `NullSpeechEngine::is_ready()`
always returns `false` (`ai/speech/src/lib.rs:33-35`). `AppState`'s
`speech_engine` field is also never reassigned after construction
(confirmed: all 5 `.speech_engine` uses are either the one-time
`AppState::new` construction or a `.lock()...` read - no
`*speech_engine = ...` anywhere). So the readiness fact is knowable
once, at startup, without ever touching the mutex again.

**Fix**: add a plain (non-`Mutex`) `pub speech_ready: bool` field to
`AppState`, computed once in `AppState::new` before the engine moves
into its `Mutex`. Replace all four status-read call sites with
`state.speech_ready` - zero lock contention, zero behavior change
(same value, same timing of when it's known).

## Finding 4 (confirmed, real but low-severity): a stale speech worker can outlive `stop_listening` and write into a new listening session

Tracing lifecycle: `stop_listening` drops the cpal `Stream` (via
`AudioEngine::stop()`), which drops the `sink` closure and its captured
`speech_tx`, closing the channel - `spawn_speech_worker`'s
`while let Ok(chunk) = rx.recv()` then exits **the next time it loops
back to `rx.recv()`**. If the worker is *currently inside* a blocking
`handle_audio_chunk`/`feed_audio` call when `stop_listening` runs, it
finishes that one call (whisper.cpp exposes no cancellation API - this
is unavoidable, not a bug) before noticing the channel closed. Meanwhile
`stop_listening` itself returns immediately (it doesn't wait for the
worker thread), so `start_listening` can be called again right away,
spawning a *new* speech worker/channel/generation while the *old*
worker's tail call may still be finishing. `active_service`
(`Mutex<Option<ServiceSession>>`) does **not** change across a
stop/start-listening cycle within the same service - listening and
service lifecycle are independent - so a same-service restart would not
be caught by an `active_service`-based check.

**Not a deadlock, not data corruption** (the old worker's final segment
is a real transcript of real audio, correctly attributed to the
correct, still-existing service row) - but it *can* emit a
`TranscriptUpdated`/`ScriptureDetected`/`SuggestionCreated` event a
moment after a fresh listening session has begun, which could read as
confusing "stale" content briefly appearing in what the operator now
perceives as a new session.

**Fix**: add `pub listening_generation: std::sync::atomic::AtomicU64`
to `AppState` (starts at 0). `start_listening` increments it once per
successful start and passes the new value into `spawn_speech_worker`.
Before emitting/persisting a **non-empty** result (i.e., only when
there is something to emit - not on every buffering-only call, keeping
this cheap), the worker checks the current generation still matches the
value it was spawned with; if not, the result is logged and discarded,
never persisted or emitted. Deliberately **not** gated on `stop_listening`
alone (a plain stop-without-restart should still surface its last few
real, in-flight seconds of speech - discarding those would throw away
genuine transcript for no reason) - only a *new* `start_listening`
invalidates an old worker's output.

## Finding 5 (confirmed, not a defect): Sermon/Content/Cross-Domain live-wiring - unchanged from Phase 3.8.7.2's own finding, still out of scope

Re-confirmed by re-reading `pipeline.rs::handle_final_transcript` and
`LiveChurchBrain.tsx`'s `onTranscriptUpdated` handler: unchanged from
Phase 3.8.7.2's Finding 3. Not re-litigated here.

## Finding 6 (measured, not a bottleneck): database pipeline already instrumented, just not surfaced

`pipeline.rs::handle_final_transcript` already times each stage
(`persist_transcript_segment`, `process_transcript_segment`,
detection/suggestion persistence) via `Instant`, logged at `debug`
level under the `cip::performance` target - this already existed before
this phase. Given Bible detection fires at most once per ~3s inference
cycle (never per audio chunk), this pipeline's absolute cost is
structurally bounded to a low, infrequent rate - not a plausible
contributor to progressive slowdown. **Not redesigning it** (no
evidence justifies that), but surfacing the existing total duration
into `SpeechDiagnostics` (a new `last_transcript_pipeline_duration_ms`
field, set from a single `Instant` wrapped around the existing
`handle_final_transcript` call in `commands.rs`) so the operator can
see it in Diagnostics without reading debug logs.

## Finding 7 (measured, not a bottleneck): frontend event volume

`onTranscriptUpdated`/`onScriptureDetected`/`onScriptureUpdated`/
`onSuggestionCreated` (`LiveChurchBrain.tsx`, re-read fresh) all fire
only on genuine final-segment results - at most once per ~3s Whisper
inference cycle (≈0.33 events/sec during continuous speech), since
`WhisperSpeechEngine` never fabricates interim segments (confirmed in
its own module docs, unchanged). With the Finding 2 fix in place
(stale backlog is *discarded*, never bulk-replayed), there is no
mechanism by which this phase's changes could cause an event burst
either. **No frontend throttling added** - per the operator's own
instruction not to add it without measurement justifying it, and this
measurement does not justify it.

## Decisions for this phase

1. Fix the inference-counter miscounting (Finding 1) - small, direct,
   already flagged by name in the operator's own spec.
2. Replace the unbounded speech channel's *unbounded backlog* behavior
   with a measured, bounded-backlog-with-explicit-drain design
   (Finding 2) - the phase's central fix.
3. Cache `speech_ready` to eliminate status-poll lock contention
   (Finding 3) - small, exactly matches the operator's own preferred
   architecture description in section 6.
4. Add a listening-generation guard against a stale worker writing into
   a new session (Finding 4) - small, precisely scoped to the confirmed
   race, not a broader rewrite.
5. Surface existing DB pipeline timing into diagnostics (Finding 6) -
   small, additive.
6. **Not** touched: Sermon/Content/Cross-Domain wiring (Finding 5, out
   of scope), frontend throttling (Finding 7, unjustified by
   measurement), the whisper.cpp inference implementation itself, the
   database schema, existing Tauri command signatures (only additive
   fields), event contracts, and the Phase 3.8.7.2 worker-thread
   architecture (preserved and built upon, never undone).

## Diagnostics to add (per the operator's list, scoped to what's technically real)

`chunks_received` (existing), `inferences_attempted`/`inferences_succeeded`
(existing, now correctly counted per Finding 1), `queue_pending_ms`
(current backlog, replaces a raw "chunk count" framing with an honest
time-based one - directly answers "is audio backing up" in a unit an
operator can reason about), `queue_high_water_ms` (max ever observed
this session), `overload_events` (count of drain-due-to-overload
events, not per-chunk drops - "coalesced" is not a per-chunk concept
here, it's a bulk-discard event), `audio_ms_dropped_overload` (total
wall-clock audio time discarded across all overload events - the
honest "how much did we throw away" figure the operator's own hard
requirement demands never be hidden), `last_inference_duration_ms`/
`max_inference_duration_ms`/average (derived from sum+count at read
time), `last_transcript_pipeline_duration_ms` (Finding 6),
`overload_state` (Normal/Busy/FallingBegind/Overloaded, derived at read
time from `queue_pending_ms` against fixed, documented thresholds -
never stored redundantly). All surfaced through the existing
`SpeechRuntimeDiagnostics`/`PilotDiagnostics`/Diagnostics panel
mechanism - no new Tauri command needed.

## What this phase does NOT change

Whisper's own inference implementation, the 3-second buffering window
itself, Bible detection logic, the database schema, existing Tauri
command signatures (only additive struct fields), event contracts, the
presentation lifecycle, offline architecture, or the Phase 3.8.7.2
worker-thread decoupling (built upon, not undone - the cpal callback
remains exactly as lightweight as 3.8.7.2 left it: two non-blocking
channel/atomic operations, no locks held across inference, no Whisper
call ever runs on that thread).
