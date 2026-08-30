# Phase 3.8.7.5 — Audit/Design: Live Intelligence Router + Adaptive Transcript Segmentation

## Baseline (confirmed directly, not assumed)

- Branch: `claude/cip-foundation-init-i85g87` (`git branch --show-current`)
- HEAD: `88a60b2` (Phase 3.8.7.4's own commit, `git rev-parse HEAD`)
- Working tree: clean (`git status --porcelain`, 0 lines)

## Why this phase exists

Phase 3.8.7.4's audit (`docs/phase-3-8-7-4-audit.md`) confirmed, by direct
code citation, that `handle_final_transcript` calls exactly one engine
(Bible) and that Sermon/Service Phase/Music-text are complete, tested,
but deliberately manual-command-only engines. The operator's own
follow-up gave a concrete two-part design: (A) stop treating Whisper's
own ~3s buffering window as the unit of persistence/analysis - instead
accumulate it into bounded 12-20s logical segments - and (B) route each
completed logical segment through Bible, Sermon, Service Phase, and
(where suitable) Music-text, reusing each engine's already-tested
`analyze_and_queue` unchanged.

## Design decisions

### Part A - segmentation

`TranscriptSegmenter` (new module, `segmentation.rs`) concatenates
consecutive raw Whisper-window segments' text until the accumulated
span reaches a 15s target (the middle of the operator's 12-20s band),
then emits one logical `TranscriptSegment` with its own id, a
confidence averaged across every raw segment it absorbed, and
start/end timestamps spanning the whole window. Given Whisper's own
fixed ~3.0s emission cadence (`CHUNK_SAMPLES`, unchanged this phase),
this produces segments landing between 15s and ~18s in practice -
inside the requested band without a second, redundant "max" constant.

**Explicitly not implemented**: pause/silence-based early flushing.
`AudioEngine`/`WhisperSpeechEngine` expose no voice-activity signal
today - Whisper's buffer fills at a fixed audio-time cadence regardless
of speech vs. silence (confirmed by re-reading `ai/speech/src/whisper.rs`
fresh this phase: `feed_audio` triggers `run_inference()` purely on
`buffer.len() >= CHUNK_SAMPLES`, with no reference to silence/VAD
anywhere in that file). Inventing an early-flush trigger keyed to a
signal that doesn't exist would be guessing, which this project's own
discipline forbids. A fixed time-window is therefore the only trigger
this phase implements; true pause-aware segmentation remains a distinct,
larger, future design that would need new evidence from the audio layer -
explicitly out of scope, per the operator's own instruction not to
modify Whisper/CPAL again without evidence requiring it.

**Overload interaction (Phase 3.8.7.3)**: when the speech worker's own
backlog-overload logic drains stale queued audio and calls
`SpeechEngine::discard_buffered_audio()`, it must also call
`TranscriptSegmenter::reset()` on the same worker's segmenter - otherwise
text accumulated just before the overload gap would be spliced onto
unrelated text arriving after recovery, the exact discontinuous-buffer
problem `discard_buffered_audio` exists to prevent one layer down. This
is a real interaction the implementation must get right, not an
optional nicety.

**Stop-mid-window**: when `stop_listening` closes the speech channel and
`spawn_speech_worker`'s loop exits, whatever is still buffered in the
segmenter (less than 15s of real speech) is force-flushed and routed
exactly like a normal completed window - never silently dropped. This is
a new, small behavior this phase adds (previously, a partial ~3s Whisper
buffer below its own threshold was simply lost on stop, unchanged
pre-existing behavior the operator did not ask to fix, but the same gap
at the new, coarser 12-20s grain would now discard far more real speech
if left unhandled).

### Part B - the router

`route_segment_to_live_intelligence_engines` runs immediately after
`handle_final_transcript` succeeds, on the exact same bounded logical
segment. It builds one `IntelligenceContext` (via the existing
`build_music_context`, reused rather than rebuilt three times) and calls:

- `crate::sermon::analyze_and_queue` against `AppState.sermon_engine` -
  covers Sermon Intelligence findings **and** Prayer ("PrayerPoint" is a
  `SermonElementKind` this engine already detects internally - Phase
  3.8.7.4 Finding, no separate call needed).
- `crate::service::analyze_and_queue` against `AppState.service_engine` -
  covers Service Phase Intelligence **and** Worship (`ServicePhase::Worship`
  is a phase this engine already detects internally - same reasoning).
- `music::analyze_and_queue` against the registry's Music engine - the
  lyric/text path, included because it is the one case Phase 3.8.7.4
  found already built to accept arbitrary transcript text safely (its
  own distinctiveness/confidence gating already returns zero findings
  for non-lyric prose - this is the engine's own existing, tested
  behavior, not a new safety mechanism added here).

Each function is a thin wrapper mirroring its corresponding manual
command's post-context logic exactly (same event emissions, same
timeline records) - the router adds no new engine logic, no new
database schema, no new event contracts, only a new caller.

**Deliberately excluded**: Cross-Domain Correlation and Content
Intelligence. Both are explicitly documented, in their own doc
comments, as "an explicit operator/diagnostic action, never triggered
automatically by a transcript segment arriving" - a considered design
decision from Phase 2.4/2.7/2.8. Wiring them into the router would
silently reverse that decision, which the operator did not ask for.

**Deliberately excluded**: automatic Altar Call detection.
`SermonSectionKind::AltarCall` exists only as an operator-assignable
label with no phrase detector (Phase 3.8.7.4 Finding) - there is no
engine here to route to. Building one is new detection logic, a
separate, larger phase.

## What this phase does NOT touch

Per the operator's own explicit instruction: the CPAL callback, the
speech worker's channel/backpressure logic, `WhisperSpeechEngine`'s
buffering/inference implementation, and the Phase 3.8.7.3 overload-drain
thresholds are all unchanged - the router and segmenter operate strictly
after Whisper has already produced a raw final segment, on the same
speech-worker thread, never before or during inference.

## Performance consideration (the operator's own point 4)

This hardware's own Phase 3.8.7.3/3.8.7.4 diagnostics showed average
Whisper inference at 13.9s per ~3s audio window - the dominant cost in
this pipeline by roughly three orders of magnitude versus what Sermon/
Service/Music's deterministic, non-AI, regex/rule-based analysis costs
(each is plain in-process pattern matching over a string, no model
inference). Calling three additional engines once per ~15-18s logical
segment (a ~5-6x lower call rate than the previous per-~3s-segment Bible
call alone) is not expected to be a measurable addition against a
13,900ms baseline, but this is stated as reasoning, not measured fact -
this container cannot reproduce the operator's real hardware's inference
timing to verify it directly. The regression suite and the operator's
own next real-hardware test remain the actual verification, exactly as
every prior phase's performance claims have been treated.
