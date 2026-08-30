# Phase 3.8.7.7 — Audit: overload-drain destroys successfully-transcribed text on hardware slower than the overload threshold

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `d311ffe` (Phase 3.8.7.6, audit-only)

## Trigger

The operator's real Windows hardware diagnostics, pasted verbatim after Phase
3.8.7.5/3.8.7.6, showed:

```
Live Transcript: Nothing transcribed yet.
Audio chunks received: 6600 (last: 480 samples @ 48000 Hz, resampled to 160 samples)
Inferences: 21 succeeded / 21 attempted
Queued audio: 10ms (high water: 24640ms this session)
Overload events: 21 (total 314730ms of audio discarded)
Inference duration: last 13286ms, avg 14991ms, max 24643ms
Transcript pipeline (DB + Bible detection): last n/a
```

21 successful inferences, zero transcript output, and exactly 21 overload
events - a 1:1 match with the inference count, not a coincidence. The
operator asked for an instrumentation-first audit distinguishing "the
worker is simply busy inferring, backlog is expected and will resolve
itself" from "the queue is genuinely, persistently falling behind," and
explicitly rejected a blind threshold increase as the fix.

## Root cause, traced from real code

`480 samples @ 48,000 Hz` is 10ms of audio per `AudioChunk` (confirmed
again this phase, matches `docs/phase-3-8-7-3-audit.md`'s own measurement).
`spawn_speech_worker` (`commands.rs:1418`) is a single thread: while it is
blocked inside one `feed_audio` call that triggers real Whisper inference,
it cannot dequeue from `rx` at all. CPAL keeps depositing new 10ms chunks
into the channel throughout, so `pending_ms` (the shared backlog counter -
incremented by `start_listening`'s sink closure, decremented here on
dequeue, per `docs/phase-3-8-7-3-audit.md` Finding 2) climbs by
approximately the wall-clock duration of that one inference.

`OVERLOAD_THRESHOLD_MS = 10_000` (`commands.rs:488`). On this operator's
hardware, avg inference duration is 14,991ms - already past the threshold
before a single inference even finishes. So the very next `rx.recv()` after
*any* successful inference dequeues a chunk whose `backlog_ms` is already
≥10s, and unconditionally takes the overload branch (`commands.rs:1449-1486`):
it drains the queued backlog (correct - matches Phase 3.8.7.3's own
"never process stale audio" design) **and** calls `segmenter.reset()`
(`commands.rs:1471`, added Phase 3.8.7.5) - discarding the text that
inference *just* produced, before it can ever reach the segmenter's 15s
accumulation target and flush into a persisted, routed, displayed segment.

This is architecturally different from the failure Phase 3.8.7.3 was built
to solve. At the time Phase 3.8.7.3 was designed, each inference's output
was used immediately (no cross-inference accumulation existed) - resetting
`WhisperSpeechEngine`'s own internal buffer on overload was the only state
that needed clearing, and doing so was unconditionally correct: it only
ever discarded *unprocessed audio*, never previously-produced *text*.
Phase 3.8.7.5 added `TranscriptSegmenter`, a second kind of state that must
now survive *across multiple* inference cycles before it produces anything
- and extended the same overload branch to reset it too, without
re-examining whether "backlog crossed 10s" still reliably meant "genuinely,
persistently falling behind" once a single inference's own duration alone
could cross that threshold on ordinary hardware. It does not, on this
operator's exact machine: `docs/phase-3-8-7-3-audit.md` even predicted this
scenario in its own Finding 2 ("if a single `state.full()` call... takes
materially longer than 3.0 wall-clock seconds... the backlog grows by the
excess duration on every single inference cycle") - but that finding
predates the segmenter and only reasoned about audio backlog, not
about a second, valid, real accumulated-text buffer changing hands in the
same branch.

## The distinction the operator asked for, applied to the real control flow

Because the drain loop (`while let Ok(stale) = rx.try_recv()`) empties the
channel completely on every overload event, the very next dequeue afterward
starts from a near-empty backlog and is virtually never itself
overloaded (confirmed by the diagnostics: `Queued audio: 10ms` currently,
despite 21 overload events having fired). Concretely, on this hardware each
overload event is *isolated* - one inference completes, one overload-drain
fires, then many fast (buffering-only, non-inference-triggering) chunks
process normally for the next ~3s of real audio until the next inference
triggers and the cycle repeats. This is exactly the "worker busy, backlog
expected to resolve" case: the backlog spike is fully explained by the one
inference just finished, and resolves on its own the moment the drain
clears the channel - it is not a trend of the backlog staying elevated
independent of any single inference.

A genuinely "falling further behind, not just busy" machine would instead
show *consecutive* overload events - the backlog would still be ≥10s on
the very next dequeue even immediately after a drain (e.g. a machine so
slow that even the "catch-up" fast chunks can't clear the queue before the
next inference-triggering window fills). The current code has no signal to
tell these two cases apart - it treats every single threshold crossing
identically, regardless of what happened on the previous cycle.

## Duplicate/side-effect check

- Audio-discard behavior (draining the channel, calling
  `discard_buffered_audio()`) is **unaffected** by this phase's fix - it
  still happens unconditionally on every overload crossing, exactly as
  Phase 3.8.7.3 designed it. Only whether `segmenter.reset()` additionally
  fires becomes conditional.
- No change to Whisper inference, CPAL, the channel/threshold constants,
  the database, or any event contract.
- `classify_overload`'s operator-facing `OverloadState` (Normal/Busy/
  FallingBehind/Overloaded) is unaffected - it still reflects the raw
  backlog depth exactly as before; this phase does not touch what the
  operator sees in diagnostics, only whether the segmenter survives an
  isolated overload blip.

## Recommended minimal fix

Track one new piece of state local to `spawn_speech_worker` (owned
exclusively by the worker thread, same ownership pattern as `segmenter`
itself): `consecutive_overloads: u32`. Increment it on every overload
crossing; reset it to `0` on every normal (non-overloaded) dequeue. Only
call `segmenter.reset()` when `consecutive_overloads` indicates this is not
the first overload event since the worker was last caught up (i.e. the
backlog was already elevated on the previous cycle too, not just this one)
- extracted as a small pure function (`should_reset_segmenter_on_overload`)
so the threshold is directly unit-testable, mirroring `classify_overload`'s
own existing pattern in this file.

This is the smallest change that implements the operator's own rule
("worker busy + backlog resolving on its own → preserve accumulated text;
backlog still elevated across consecutive cycles → genuine overload, reset
as before") without touching the threshold constant, the audio-discard
behavior, or any other file.

## Final gate

| Item | Status |
|---|---|
| Root cause traced to exact code, not inferred | DONE |
| Arithmetic reproduced against the operator's real diagnostics numbers | DONE |
| Distinguishing signal (busy-vs-genuine-overload) identified from existing control flow, no new instrumentation needed | DONE |
| Fix scoped to the smallest change (segmenter-reset condition only, audio-discard unchanged) | DONE |
| Do not raise `OVERLOAD_THRESHOLD_MS` | HONORED - unchanged |

**Phase 3.8.7.7 audit: PASS.** Proceeding to implement the fix described
above in the same phase, per the operator's own two-step framing
(audit/measure, then fix from the measurements) - the diagnostics already
gathered under Phase 3.8.7.3's instrumentation are sufficient evidence; no
new instrumentation is required before the fix can be justified.
