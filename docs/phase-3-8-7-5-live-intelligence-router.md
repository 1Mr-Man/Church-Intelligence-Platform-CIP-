# Phase 3.8.7.5 — Live Intelligence Router + Adaptive Transcript Segmentation

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `88a60b2` (Phase 3.8.7.4, engine-by-engine audit)

## Why this phase exists

The operator's real Windows long-run test of the Phase 3.8.7.3 artifact
confirmed the stability fix works (62 real inferences succeeded, no
hangs, transcript pipeline 6ms), but the Intelligence Feed showed only
Bible activity - Sermon, Music, Prayer, Worship, Service Phase, and
Altar Call never appeared. Phase 3.8.7.4's audit traced the actual code
and confirmed these engines were complete, tested, but deliberately
manual-command-only since Phase 2.1-2.6. The operator's own follow-up
design, in two parts: (A) stop treating Whisper's raw ~3s buffering
window as the unit of persistence/analysis - accumulate into bounded
12-20s logical segments instead - and (B) route each completed segment
through Bible, Sermon, Service Phase (which already covers Prayer and
Worship internally), and Music-text, reusing each engine's already-
tested `analyze_and_queue` unchanged.

## Audit/Design — see `docs/phase-3-8-7-5-audit.md`

Confirmed via fresh re-reading of `ai/speech/src/whisper.rs` that no
voice-activity/silence signal exists anywhere in the current pipeline,
so pause-based early flushing was deliberately not attempted (it would
be guessing at a boundary CIP cannot detect) - a fixed 15s target window
(landing between 15-18s given Whisper's fixed ~3.0s cadence) is the only
trigger this phase implements. Confirmed the router's insertion point
(immediately after `handle_final_transcript` succeeds, inside
`handle_audio_chunk`'s final-segment branch) and confirmed Cross-Domain
Correlation/Content Intelligence must stay excluded, since both are
explicitly documented as "never automatic" by design.

## Fix applied

**Part A**: new `apps/desktop/src-tauri/src/segmentation.rs::TranscriptSegmenter` -
concatenates consecutive raw Whisper-window segments' text until the
accumulated span reaches a 15s target, producing one logical
`TranscriptSegment` with its own id, averaged confidence, and start/end
timestamps spanning the whole window. Owned exclusively by one
`spawn_speech_worker` thread per listening session (mirrors
`acoustic::AcousticWorkerState`'s ownership pattern) - never shared,
never behind a `Mutex`. Two real interactions with existing Phase
3.8.7.3 machinery had to be handled correctly: the worker's overload-
drain branch now also calls `segmenter.reset()` (preventing pre/post-
overload text splicing, the same problem `discard_buffered_audio`
solves one layer down), and the worker's loop-exit path now flushes and
routes any real partial-window text so `stop_listening` mid-window never
silently drops speech.

**Part B**: new `commands.rs::route_segment_to_live_intelligence_engines`,
called from a new shared `finalize_and_route_segment` function
(extracted so both the normal per-window flush and the stop-mid-window
flush share identical persistence/routing/event behavior) immediately
after `handle_final_transcript` succeeds. Builds one `IntelligenceContext`
(reused, not rebuilt three times) and calls `crate::sermon::analyze_and_queue`,
`crate::service::analyze_and_queue`, and `music::analyze_and_queue` -
the identical functions each manual Tauri command already called, with
identical event emissions and timeline records. Prayer and Worship
require no separate call: `PrayerPoint` (a `SermonElementKind`) and
`ServicePhase::Worship` are both already detected internally by Sermon/
Service Intelligence respectively.

No changes to Whisper's inference implementation, the CPAL callback, the
speech worker's channel/backpressure thresholds, Bible detection logic,
the database schema, or any event contract - the router and segmenter
operate strictly after Whisper has already produced output, on the same
speech-worker thread, calling only pre-existing, pre-tested functions.

## Full regression result

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (both
default and `--features whisper`): clean. `cargo test --workspace` (both
feature configs): `cip-desktop` 246/246 passed (up from 237 - 9 new
segmentation unit tests), `cip-ai-speech --features whisper` 7/7 passed,
every other workspace crate green, zero failures anywhere. `cargo check
--target x86_64-pc-windows-gnu --features whisper`: clean. Frontend
(unchanged this phase - no new commands/events/contracts): typecheck (0
errors), lint (0 errors, 4 pre-existing warnings unchanged), test
(210/210 passed, unchanged count), build clean.

## Windows artifact

- SHA-256: `b48b34895f4db49a56a4909ddb8c9a5bef8e1ea58e8976065f1d64e32345c681`
- Size: 8,583,538 bytes (up slightly from 8,580,489 - expected for a new
  module plus router logic)
- Direct proof the fix compiled in: `x86_64-w64-mingw32-strings` against
  the extracted `cip-desktop.exe` finds the mangled symbols for
  `segmentation::TranscriptSegmenter::push/flush/flush_remaining/reset`
  and `commands::finalize_and_route_segment` - read directly out of the
  shipped binary. The router's own per-domain dispatch functions were
  inlined by the release-mode optimizer (single call site each) and are
  verified via the regression suite instead.
- Runtime DLLs, model picker, worker-thread decoupling, backpressure
  instrumentation, whisper feature: all re-verified present and
  unaffected - see `pilot-evidence/3.8.7.5/`.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/lib.rs (module registration),
  apps/desktop/src-tauri/src/commands.rs, release/windows/*
FILES CREATED: apps/desktop/src-tauri/src/segmentation.rs,
  docs/phase-3-8-7-5-audit.md,
  docs/phase-3-8-7-5-live-intelligence-router.md,
  pilot-evidence/3.8.7.5/*
FILES DELETED: NONE
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE - every event the router emits
  (SermonFindingDetected/SermonStateChanged/SermonThemeChanged/
  SermonStructureUpdated/ServicePhaseChanged/ServiceAnomalyDetected/
  MusicFindingDetected) already existed and was already emitted by the
  corresponding manual command
DATABASE / MIGRATIONS: UNCHANGED
BIBLE DETECTION LOGIC: UNCHANGED (still runs first, on the same
  bounded segment, via the unmodified handle_final_transcript)
SERMON/SERVICE/MUSIC ENGINE LOGIC: UNCHANGED - only a new caller
CROSS-DOMAIN/CONTENT INTELLIGENCE: UNCHANGED, still manual-only by design
WHISPER INFERENCE / CPAL / BACKPRESSURE (Phase 3.8.7.3): UNCHANGED
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary symbol verification of the segmentation module.
- **Environment B (Xvfb)**: unavailable, pre-existing, unrelated.
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's own
  test: with a real sermon/service transcript flowing, does the
  Intelligence Feed now show Sermon/Service/Prayer/Worship findings
  alongside Bible ones, do displayed transcript entries read as complete
  ~15-18s segments rather than choppy ~3s fragments, and does the app
  remain as responsive as the Phase 3.8.7.3 artifact was.

## Known limitations

- Segmentation uses a fixed 12-20s time window only - no pause/silence-
  based early flushing (no voice-activity signal exists in the current
  pipeline to trigger one honestly).
- Cross-Domain Correlation and Content Intelligence remain manual-only,
  deliberately - not reversed this phase.
- Automatic Altar Call detection remains unimplemented - a future,
  focused classifier phase.
- The performance impact of three additional engine calls per ~15-18s
  segment is reasoned (deterministic pattern-matching, negligible next
  to this hardware's 13.9s average Whisper inference) but not measured
  on real hardware in this container.

## Final gate

| Item | Status |
|---|---|
| Engine-by-engine audit (Phase 3.8.7.4) | DONE |
| Segmentation design audited against real Whisper/audio code | DONE - confirmed no VAD signal exists, fixed time-window chosen honestly |
| Router insertion point confirmed against actual code | DONE |
| Smallest justified implementation, no new engine logic | DONE - every analyze_and_queue call and event emission copied verbatim from its manual command |
| Full regression green | DONE |
| Windows artifact rebuilt + fix verified in compiled binary | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.8.7.5: Environment A verification PASS, including direct
proof the segmentation module is compiled into the shipped binary. Real
Windows re-test (Environment C) is the pending, decisive gate.** Per
this project's own established discipline, this is not marked PASS
merely because the code compiles and the regression suite is green -
only the operator's real hardware can confirm the Intelligence Feed
actually shows non-Bible findings and the transcript display reads as
complete segments.
