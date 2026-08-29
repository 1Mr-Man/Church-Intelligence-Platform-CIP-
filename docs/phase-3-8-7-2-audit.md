# Phase 3.8.7.2 — Audit: Real-Time Speech Performance & Detection

Written before implementation, per the operator's own instruction ("do
not guess and do not redesign the pipeline yet - audit the real running
implementation first").

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `91a918e` (Phase 3.8.7.1, model provisioning)

## Trigger

With a real model now installed, the operator's real Windows report
shows: transcription active, audio arriving (Stereo Mix, 4-7% input),
but CIP becomes slow/unresponsive, transcript text is poor/repetitive,
the Intelligence Feed stays empty, and the UI sometimes flashes `NO
SIGNAL`. The operator's own hypothesis: something is processing the
tiny (480-sample, ~10ms) audio callbacks too eagerly.

## Full runtime path traced (with file:line citations)

```
cpal real-time audio callback (integrations/audio/src/lib.rs:316-336,
  inside device.build_input_stream's closure)
    -> downmix_to_i16, rms_level (cheap, <1ms)
    -> sink(AudioChunk) - called SYNCHRONOUSLY, same thread
        -> apps/desktop/src-tauri/src/commands.rs:939-942 (start_listening's sink closure)
            -> acoustic_tx.try_send(chunk.clone())  [non-blocking, bounded channel - OK]
            -> handle_audio_chunk(&app, service_id, chunk)  <-- RUNS INLINE, SAME THREAD
                -> commands.rs:1232-1400
                -> resample_pcm16 (cheap)
                -> state.speech_engine.lock() HELD across the entire call below
                -> speech.feed_audio(...)
                    -> ai/speech/src/whisper.rs: buffers until 48,000 samples
                       (3.0s at 16kHz) have accumulated, THEN calls
                       state.full(params, &audio_f32) - whisper.cpp's real,
                       synchronous, CPU-bound inference call
                -> handle_final_transcript (pipeline.rs:64) - Bible detection,
                   SQLite writes (transcript segment, detections, suggestions)
                -> emit(TranscriptUpdated / ScriptureDetected / SuggestionCreated)
```

## Finding 1 (root cause): Whisper inference runs synchronously on the real-time audio capture thread

`CpalAudioEngine`'s own module docs (`integrations/audio/src/lib.rs:9-21`)
already state the standard pattern this project follows elsewhere:
cpal's `Stream` is thread-affine, so a dedicated worker thread owns it;
callers talk to it over channels. The **acoustic** consumer already
follows this discipline fully: `start_listening` hands each chunk to
`spawn_acoustic_worker` (commands.rs:1080) via a channel, so acoustic
analysis - however slow - can never block audio capture. Its own doc
comment says so explicitly: *"a slow/backed-up acoustic worker can never
block the audio capture thread or the speech-engine feed right after
it"* (commands.rs:929-931).

**The speech path was never given the same treatment.** `handle_audio_chunk`
is called directly, inline, inside the `sink` closure - which is itself
invoked synchronously from inside cpal's own real-time callback
(`integrations/audio/src/lib.rs:327`, `sink(AudioChunk {...})`, no
channel in between). Real-time audio callback contracts (on every
backend cpal supports, including WASAPI on Windows) require this
closure to return quickly - typically low-single-digit milliseconds -
so the OS can keep feeding the ring buffer. Once every ~3 seconds of
buffered audio (`CHUNK_SAMPLES` in `ai/speech/src/whisper.rs:38`),
`handle_audio_chunk` instead blocks that same thread for the full
duration of a real whisper.cpp `full()` call - real CPU-bound work with
no defined upper bound, easily hundreds of milliseconds to multiple
seconds depending on the machine, even for the `tiny.en` model.

This single defect plausibly explains every symptom reported:

- **Slowness/unresponsiveness**: a real-time-priority audio thread
  being monopolized by CPU-bound ML inference competes directly with
  the rest of the process (and, on Windows, a starved real-time audio
  thread can itself trigger OS-level scheduling/priority effects).
- **Poor/repetitive transcription**: if the OS's audio backend cannot
  service the stream promptly during a blocked callback, samples can be
  dropped or the stream can glitch - feeding Whisper discontinuous or
  corrupted audio, a well-known cause of hallucinated/repetitive output
  (e.g. looping short phrases) from Whisper-family models on
  low-quality input.
- **Intermittent `NO SIGNAL`**: see Finding 2 below - this is not
  independent, it is a direct, mechanical consequence of Finding 1.

## Finding 2 (direct consequence of Finding 1): the same lock blocks the UI's own status poll

`handle_audio_chunk` acquires `state.speech_engine.lock()`
(commands.rs:1238-1337ish) and holds it for the *entire* `feed_audio`
call - including, once every ~3 seconds, the blocking whisper.cpp
inference itself. Three other call sites need that exact same lock,
all just to read `.is_ready()`: `get_pilot_diagnostics`
(commands.rs:472), `stop_listening` (commands.rs:1030), and
`get_live_status` (commands.rs:5025) - and the frontend polls
`getLiveStatus()` on a timer (`LiveChurchBrain.tsx:245`,
`STATUS_POLL_MS = 3000`, `LiveChurchBrain.tsx:49`). **3000ms is the
same cadence as Whisper's own 3-second inference window** - not a
coincidence worth ignoring: any poll that lands while inference is
running blocks on the mutex until inference finishes, so the UI's own
status read (input level, capturing/idle, everything `AudioEngineStatus`
carries) can stall or read stale exactly when the operator would see
`SIGNAL CAPTURED` flip to `NO SIGNAL` or the UI freeze.

## Finding 3: Sermon/Content/Cross-Domain Intelligence are never invoked from the live path at all - a pre-existing, documented scope boundary, not a new bug

Traced precisely: `handle_final_transcript` (`pipeline.rs:64-135`) calls
only `cip_core_service::process_transcript_segment` - **Bible**
Intelligence only. Confirmed by reading its own module docs
(`pipeline.rs:1-16`, the diagram lists exactly `persist -> Bible
Intelligence Core -> persist detections -> persist suggestions`,
nothing else). On the frontend, `onTranscriptUpdated` (`LiveChurchBrain.tsx:360-364`)
only appends the segment to the displayed transcript list; the only
place `analyzeSermonTranscript` is called is the **manual** Sermon
text-entry path (`LiveChurchBrain.tsx:1640`), never from a live segment.
Music (acoustic) is the one other domain wired live, via the separate
`spawn_acoustic_worker` channel already discussed.

This matches this project's own history: Phase 3.8's `ServiceReplay`
screen exists *specifically* because live speech was never wired to
Sermon/Content/Cross-Domain Intelligence (see `docs/phase-3-8-*` audits).
**This is not a regression and not this phase's bug** - it is a real,
already-known, already-documented architectural boundary. The
operator's "Intelligence Feed: Nothing detected yet" during live
listening is expected today for those three domains regardless of
transcript quality. For **Bible** Intelligence specifically (the one
domain that IS live-wired), "nothing detected" is fully explained by
Finding 1: garbage/repetitive Whisper output has no real scripture
references in it to find - fixing Finding 1 should let Bible detection
actually see clean text and be judged on its own merits, which is
exactly why Finding 1 must be fixed first, per the operator's own
"measure -> verify buffering -> verify Whisper output -> trace
downstream -> fix the exact broken point" ordering.

## Answering the operator's specific questions

1. **How often is `handle_audio_chunk` called?** Once per cpal callback
   - at 480 samples/callback and a 48,000 Hz device rate, that's ~100
   times/second (~10ms cadence).
2. **How many samples/seconds passed into Whisper per inference?**
   Exactly `CHUNK_SAMPLES` = 48,000 samples at 16kHz = 3.0 seconds of
   (resampled) audio per real inference call - this part is already
   correct; Whisper is not naively invoked per tiny callback.
3. **Per-callback or per-window?** Per-window (confirmed above) - the
   defect is *where* that window's inference runs, not how often.
4. **Database write frequency during listening?** Only on a **final**
   segment (i.e., roughly once per completed 3-second inference window,
   not per callback) - `persist_transcript_segment` plus one row per
   detection/suggestion. Not excessive by itself.
5. **Frontend update frequency?** `TranscriptUpdated`/`ScriptureDetected`/
   `SuggestionCreated` events fire only on real results (interim or
   final segments), so also roughly per-inference-window, not
   per-callback. The *separate* `getLiveStatus` poll is time-based
   (3000ms) regardless of audio activity - see Finding 2.
6. **Expensive synchronous work on the UI/event thread?** Not on
   Tauri's own command/event thread directly - the expensive work is on
   cpal's real-time audio thread (Finding 1), whose stalls are then
   *observed* via the UI's blocked status poll (Finding 2).
7. **Is the Intelligence Feed receiving finalized segments?** Yes, for
   Bible Intelligence (garbage-in/garbage-out per Finding 1). Sermon/
   Content/Cross-Domain never receive live segments at all (Finding 3,
   pre-existing).
8. **Why is detection skipped?** Confirmed: not confidence-thresholding,
   not a missing detector, not context loss - Bible detection runs on
   every final segment; it finds nothing because the segments contain
   (per the operator's screenshots) hallucinated/repetitive
   near-nonsense text, not because the pipeline itself is broken.
9. **`NO SIGNAL` vs `SIGNAL CAPTURED` vs "transcription active"?**
   `NO SIGNAL`/`SIGNAL CAPTURED` are the frontend's own read of
   `AudioEngineStatus.input_level` from the polled `get_live_status` -
   a live RMS reading of the actual capture stream (see
   `integrations/audio/src/lib.rs:288-297,325`), independent of
   Whisper. "Transcription active" reflects `speech.is_ready()`/
   `engine_ready` - whether a loaded model exists, independent of
   whether any particular chunk produced text. The intermittent
   flip to `NO SIGNAL` is the poll stalling behind the inference lock
   (Finding 2), not audio hardware actually going silent.
10. **Confirm the exact running build?** The operator's own screenshot
    showed `61c80a... + uncommitted changes` before this phase - exactly
    the Phase 3.8.7.1 build-dirty fix (Phase 3.8.7.1's own build ran
    with 13 uncommitted files, matching `build_dirty: true`) working as
    designed, not a stale/misleading reading. This phase's own rebuild
    will embed the real commit this phase lands on.

## Decision

Fix Finding 1 (and its Finding 2 consequence) with the smallest change
that preserves the existing architecture: give the speech path the
**exact same worker-thread treatment already proven correct for the
acoustic path** - hand each `AudioChunk` off through a channel to a
dedicated speech worker thread, and move `handle_audio_chunk`'s entire
body there. The cpal callback then only ever does cheap, bounded work
(downmix, RMS, two non-blocking channel sends) before returning,
restoring the real-time-audio contract this project's own docs already
describe as the reason a worker thread exists at all.

One deliberate difference from the acoustic worker: acoustic uses a
**bounded** channel with `try_send` (dropping a chunk there costs one
best-effort fingerprint window, acceptable per its own docs). Dropping
a chunk mid-buffer on the **speech** path would introduce a gap into
whatever Whisper is accumulating - reintroducing a different flavor of
the same audio-corruption problem this fix exists to solve. The speech
channel is therefore **unbounded** (`mpsc::channel`, not
`mpsc::sync_channel`): the producer (cpal's thread) must never block or
drop, and steady-state throughput is not at risk - the worker's own
processing (resample + buffer-append) is far cheaper than real-time
audio arrival except during the brief inference window, so the queue
drains as fast as it grows in all but that one predictable interval,
which is bounded (`CHUNK_SAMPLES` duration between whisper.cpp calls).

Sermon/Content/Cross-Domain intelligence (Finding 3) is explicitly
**out of scope** for this phase - it is a pre-existing, documented
architectural boundary, not a regression, and wiring three more
intelligence domains into the live path is a much larger design
decision than "audit the runtime path and fix the confirmed
performance/quality defect." Recording it here so it is not lost, but
not fixing it without being asked.

## What this fix does NOT change

No change to `WhisperSpeechEngine`'s buffering window, resampling logic,
Bible detection logic, database schema, or event contracts. Purely
moves *where* the existing per-chunk work executes.
