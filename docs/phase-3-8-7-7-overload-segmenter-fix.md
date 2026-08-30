# Phase 3.8.7.7 — Fix: overload-drain destroying successfully-transcribed text

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `d311ffe` (Phase 3.8.7.6, audit-only)

## Why this phase exists

The operator's real Windows diagnostics for the Phase 3.8.7.5/3.8.7.6
artifact showed 21/21 successful Whisper inferences and audio genuinely
arriving (6,600 chunks received), yet "Nothing transcribed yet" and
"Transcript pipeline: last n/a." The operator asked for an audit-first,
instrumentation-first investigation distinguishing "the worker is simply
busy inferring, backlog is expected" from "the queue is genuinely,
persistently falling behind," and explicitly rejected a blind overload-
threshold increase as the fix.

## Audit — see `docs/phase-3-8-7-7-audit.md`

Traced the real code and reproduced the exact arithmetic against the
operator's own numbers: 480 samples @ 48,000 Hz is 10ms of audio per
`AudioChunk`; `spawn_speech_worker` is single-threaded, so while it is
blocked inside one inference the backlog climbs by roughly that
inference's own wall-clock duration; avg inference duration on this
hardware (14,991ms) already exceeds `OVERLOAD_THRESHOLD_MS` (10s), so the
very next dequeue after *any* successful inference hit the overload
branch - which was unconditionally calling `segmenter.reset()` (added
Phase 3.8.7.5), discarding the text that inference just produced before
it could ever reach the segmenter's 15s accumulation target. 21 successful
inferences and 21 overload events is a 1:1 match, not a coincidence.

Confirmed the distinguishing signal the operator asked for was already
available from existing control flow, with no new instrumentation needed:
because the drain empties the channel completely, the very next dequeue
starts from a near-empty backlog - each overload event on this hardware
is isolated, not consecutive (confirmed by the diagnostics: current
queued audio is 10ms despite 21 overload events having fired this
session).

## Fix applied

A new `consecutive_overloads: u32` counter, local to `spawn_speech_worker`
and owned exclusively by the worker thread (same ownership pattern as
`segmenter` itself), tracks overload crossings since the worker was last
caught up - incremented on every crossing, reset to `0` on every normal
dequeue. A new pure function `should_reset_segmenter_on_overload`
(mirrors `classify_overload`'s own existing pattern in this file) only
returns `true` at `>= 2`: a single isolated crossing (this operator's
exact case) is fully explained by the one inference that just finished
and is expected to resolve on its own; only backlog still elevated across
consecutive dequeues indicates genuine sustained overload, where
resetting the segmenter (Phase 3.8.7.5's original concern - pre-overload
text spliced onto unrelated post-recovery text) remains correct.

The stale audio backlog itself is still discarded unconditionally on
every overload crossing, exactly as Phase 3.8.7.3 designed it - only
whether `segmenter.reset()` additionally fires became conditional.
`OVERLOAD_THRESHOLD_MS` itself was **not** changed, per the operator's
own explicit instruction not to blindly raise it.

No changes to Whisper's inference implementation, the CPAL callback, the
audio-discard behavior, the database schema, or any event contract - this
phase touches exactly one decision inside one existing branch.

## Full regression result

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (both
default and `--features whisper`): clean. `cargo test -p cip-desktop`
(both feature configs): 248/248 passed (up from 246 - 2 new unit tests).
`cargo test -p cip-ai-speech --features whisper`: 7/7 passed. `cargo check
--target x86_64-pc-windows-gnu --features whisper`: clean. Frontend
(unchanged this phase - zero frontend files touched): typecheck (0
errors), lint (0 errors, 4 pre-existing warnings unchanged), test
(210/210 passed, unchanged count), build clean.

## Windows artifact

- SHA-256: `3335f5101d0dbeb1d83dbeac567b83f9d56c5e836459c104459012a1ba2e9cbd`
- Size: 8,587,492 bytes (up slightly from 8,583,538 - expected for a small
  new counter plus one new pure function and two new tests)
- Direct proof the fix compiled in: `x86_64-w64-mingw32-strings` against
  the extracted `cip-desktop.exe` finds the literal log string
  `"%speech worker overloaded: discarded ~"` (now gated by the new
  consecutive-overload logic before it reaches `segmenter.reset()`) and
  the mangled symbols for `commands::handle_audio_chunk`,
  `commands::finalize_and_route_segment`, and
  `segmentation::TranscriptSegmenter::push/flush_remaining/reset` - all
  still compiled into the binary. `should_reset_segmenter_on_overload`
  itself was inlined by the release-mode optimizer (single call site,
  small pure function) - the same pattern already established for
  `classify_overload` and the router's own dispatch functions - and is
  verified via two new dedicated unit tests instead.
- Runtime DLLs, model picker, worker-thread decoupling, backpressure
  instrumentation, whisper feature, segmentation/router: all re-verified
  present and unaffected - see `pilot-evidence/3.8.7.7/`.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/commands.rs, release/windows/*
FILES CREATED: docs/phase-3-8-7-7-audit.md,
  docs/phase-3-8-7-7-overload-segmenter-fix.md,
  pilot-evidence/3.8.7.7/*
FILES DELETED: NONE
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE
DATABASE / MIGRATIONS: UNCHANGED
OVERLOAD_THRESHOLD_MS: UNCHANGED (still 10 seconds) - only whether an
  isolated crossing resets the segmenter is now conditional
AUDIO-DISCARD BEHAVIOR: UNCHANGED - stale backlog is still discarded
  unconditionally on every overload crossing
WHISPER INFERENCE / CPAL: UNCHANGED
SEGMENTATION / ROUTER (Phase 3.8.7.5) LOGIC: UNCHANGED - only the
  condition under which the segmenter is reset changed
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary proof of the fix (the overload log string plus the
  surrounding function symbols).
- **Environment B (Xvfb)**: unavailable, pre-existing, unrelated.
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's own
  re-test on the same machine: with the same real transcript flowing,
  does the Live Transcript panel now show completed text instead of
  "Nothing transcribed yet," does "Transcript pipeline: last" now show a
  real duration instead of "n/a," and does the Intelligence Feed now show
  Sermon/Service/Prayer/Worship findings alongside Bible ones.

## Known limitations

- `OVERLOAD_THRESHOLD_MS` remains 10 seconds, unchanged. On hardware even
  slower than this operator's (avg inference materially above 15s), or
  where genuine sustained overload occurs (consecutive crossings), the
  audio-discard/segmenter-reset safety behavior is unchanged from Phase
  3.8.7.3/3.8.7.5 - this phase does not claim to solve every possible
  slow-hardware profile, only the specific isolated-crossing case this
  operator's diagnostics evidenced.
- This is a targeted fix for one real, evidenced defect - it does not
  change Whisper's inherent transcription latency, nor does it make
  transcription faster on slow hardware. If avg inference time remains
  well above real-time, the operator will still see delayed, batched
  transcript segments rather than smooth continuous output - segments
  will now complete and appear, but not necessarily quickly.

## Final gate

| Item | Status |
|---|---|
| Root cause traced to exact code from the operator's real diagnostics, not inferred | DONE |
| Distinguishing signal (busy-vs-genuine-overload) identified from existing control flow, no new instrumentation needed | DONE |
| Fix scoped to the smallest change (segmenter-reset condition only) | DONE |
| `OVERLOAD_THRESHOLD_MS` left unchanged, per explicit instruction | DONE |
| Full regression green | DONE |
| Windows artifact rebuilt + fix verified in compiled binary | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.8.7.7: Environment A verification PASS, including direct
proof the fix is compiled into the shipped binary. Real Windows re-test
(Environment C) is the pending, decisive gate.** Per this project's own
established discipline, this is not marked PASS merely because the code
compiles and the regression suite is green - only the operator's real
hardware, on the same machine that produced the diagnostics driving this
phase, can confirm the Live Transcript panel now shows completed text and
the Intelligence Feed shows non-Bible findings.
