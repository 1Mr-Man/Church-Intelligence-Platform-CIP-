# Phase 3.8.7.4 — Audit: Engine-by-Engine Live-Wiring Trace

Written before any code change, per the operator's own instruction:
"Do not change segmentation or routing yet... Only after this audit
should we implement the router and transcript segmentation changes."

## Baseline (confirmed directly, not assumed)

- Branch: `claude/cip-foundation-init-i85g87` (`git branch --show-current`)
- HEAD: `d5b293144bfa0fe57f62d7526189ad2ee01d0e06` (`git rev-parse HEAD` -
  matches Phase 3.8.7.3's own commit)
- Working tree: clean (`git status --porcelain`, 0 lines)

## Why this phase exists

The operator's real Windows long-run test of the Phase 3.8.7.3 artifact
confirmed the stability fix works: Whisper is transcribing, audio
capture/resampling both work, the app no longer hangs, 62 real
inferences succeeded, the transcript pipeline is fast (6ms). But
inference itself is far slower than real time on this hardware (avg
13.9s, max 36.3s per ~3s audio window - 61 overload events, 847s of
audio discarded) - a genuine, separate hardware/model-capability
finding, not a defect in the 3.8.7.3 fix, and explicitly not being
touched this phase (per the operator's own instruction not to undo the
stability work).

Separately, the same real test showed only Bible-related activity in
the Intelligence Feed during live listening - Sermon, Music, Prayer,
Worship, Altar Call, and Service Phase intelligence never appeared. The
operator's instruction: trace the actual code, do not guess whether
this is a missing feature or a disconnected one.

## Method

Every claim below is a direct code citation (file:line), confirmed by
reading the literal source in this exact checkout - never inferred from
a prior phase's audit or from memory. Grepped fresh: every
`pub fn analyze_*` command in `commands.rs`, every
`impl IntelligenceEngine for` in `core/intelligence`, the frontend's
`onTranscriptUpdated` handler and every `commands.analyze*` call site,
and `unifiedFeed.ts`'s actual data sources.

## The trace: Whisper → transcript event → ??? (traced, not assumed)

```text
Whisper (ai/speech/src/whisper.rs::feed_audio/run_inference)
    -> TranscriptSegment (one per ~3.0s buffered audio window - CHUNK_SAMPLES)
    -> handle_audio_chunk (commands.rs:~1275, on the dedicated speech worker thread)
        -> for each final segment:
            -> handle_final_transcript (pipeline.rs:64)
                -> persist_transcript_segment (transcript_segments table)
                -> process_transcript_segment (cip_core_service - BIBLE INTELLIGENCE ONLY)
                -> persist_scripture_detection (scripture_detections table)
                -> persist_suggestion (ai_suggestions table, Pending)
            -> emit(TranscriptUpdated, segment)
            -> emit_processed_segment_events -> emit(ScriptureDetected/ScriptureUpdated, ...)
                                              -> emit(SuggestionCreated, ...)
    [PIPELINE ENDS HERE - nothing else is called]

Frontend: liveEvents.onTranscriptUpdated (LiveChurchBrain.tsx:360)
    -> appends segment to the displayed transcript list ONLY
    -> does NOT call analyzeSermonTranscript/analyzeMusicTranscript/
       analyzeServiceTranscript/analyzeCrossDomain/analyzeContentIntelligence
```

`pipeline.rs::handle_final_transcript` (read in full, lines 1-135)
calls exactly two things: `persist_transcript_segment` and
`process_transcript_segment` (`cip_core_service`, the Bible Intelligence
Core orchestrator). Nothing else. This is not new information -
Phase 3.8.7.2's Finding 3 and Phase 3.8.7.3's Finding 5 both already
confirmed it - but this phase re-confirms it against the current
checkout and extends the trace to every other engine by name, which
neither prior phase did.

## Engine-by-engine evidence table

| Engine | Exists? | Implements `IntelligenceEngine` trait? | Tested? | Live-connected to `handle_final_transcript`? |
|---|---|---|---|---|
| Bible | Yes - `cip_core_service::process_transcript_segment` | Yes (`bible_adapter.rs:73`, used for capability listing only - the live path calls `cip_core_service` directly, not through the registry) | Yes - extensive unit/integration/acceptance tests in `pipeline.rs`, `core/service` | **YES** - the only engine `handle_final_transcript` calls |
| Music (acoustic) | Yes - `commands.rs::spawn_acoustic_worker` (`:1244`) → `acoustic::recognize_fuse_and_queue` (`:1299`) | Yes (`music_adapter.rs:537`) | Yes - Phase 2.2's own acceptance tests | **YES, but only via a separate audio path** - `spawn_acoustic_worker` is fed directly by the cpal sink closure's `acoustic_tx` (`start_listening`, `:1052`), running in parallel with (not through) the speech/Whisper/transcript path. It queues into `state.intelligence_findings` - the same store the Intelligence Feed reads. Requires a real acoustic recognizer configured (`AppConfig.acoustic`); the default `NullAcousticMusicRecognizer` never produces findings - see `known limitations` in every prior Windows release manifest ("Music Library is legitimately empty in a production build"). |
| Music (lyrics/transcript text) | Yes - `commands.rs::analyze_music_transcript` (`:3336`) | (same registered engine as above) | Yes - Phase 2.1's own acceptance tests | **NO** - explicitly documented in its own doc comment: "never routed through `handle_final_transcript`/the Bible pipeline... Music must be reachable independently of Bible's path." Manual-command-only; the frontend only calls it from a manual-text-entry button (`LiveChurchBrain.tsx:1292`). |
| Sermon | Yes - `commands.rs::analyze_sermon_transcript` (`:3781`) → `crate::sermon::analyze_and_queue` → `AppState.sermon_engine` | Yes (`sermon_adapter.rs:122`, registered for diagnostics only - `lib.rs:304`) | Yes - Phase 2.3/2.6's own acceptance tests | **NO** - explicitly documented: "Deliberately manual-command-only, mirroring Music's Phase 2.1 lyric path... nothing here is wired into `pipeline.rs::handle_final_transcript`" (`commands.rs:3762-3766`). Frontend only calls it from a manual-text-entry button (`LiveChurchBrain.tsx:1640`). |
| Service Phase | Yes - `commands.rs::analyze_service_transcript` (`:4897`) → `crate::service::analyze_and_queue` → `AppState.service_engine` | Yes (`service_adapter.rs:376`, registered for diagnostics only - `lib.rs:317`) | Yes - Phase 2.4's own acceptance tests | **NO** - same pattern as Sermon: manual-command-only, never called from the live pipeline. Frontend only calls it from a manual-text-entry button (`LiveChurchBrain.tsx:1945`). |
| Cross-Domain Correlation | Yes - `commands.rs::analyze_cross_domain` (`:4516`) → `crate::cross_domain::analyze_and_queue` | No - reads every domain's already-queued findings, never implements the per-segment `IntelligenceEngine` trait itself (`cross_domain.rs`'s own module docs) | Yes - Phase 2.4/2.8's own acceptance tests | **NO, and explicitly by design, not by omission** - its own doc comment states this is "an explicit operator/diagnostic action, never triggered automatically by a transcript segment arriving (spec section 24: 'read-only... never automatic')." It correlates findings *other* engines already produced; it cannot receive a transcript segment directly even in principle. |
| Content Intelligence | Yes - `commands.rs::analyze_content_intelligence` (`:4665`) → `crate::content_intelligence::analyze_and_queue` | No - same reason as Cross-Domain (reads `context.recent_findings`, never a per-segment engine) | Yes - Phase 2.7's own acceptance tests | **NO, and explicitly by design** - identical framing to Cross-Domain: "an explicit operator/diagnostic action, never triggered automatically by a transcript segment arriving (mirrors `analyze_cross_domain` exactly)." |
| Prayer | **No separate engine exists.** `PrayerPoint` is one `SermonElementKind` taxonomy value *inside* Sermon Intelligence (`core/sermon/src/taxonomy.rs:53`, detected by phrase patterns like "let's pray"/"pray that" in `core/sermon/src/detection.rs:115-118`), and `ServicePhase::Prayer` is one phase label *inside* Service Intelligence (`service_adapter.rs:102`). | N/A | Yes, as part of Sermon's/Service's own test suites | **NO** - inherits Sermon's and Service's own not-live-connected status above; there is nothing further to wire for "Prayer" specifically. |
| Worship | **No separate engine exists.** `ServicePhase::Worship` is one phase label inside Service Intelligence (`service_adapter.rs:101`). Music's song-recognition finding summaries reference "Phase: Worship" in test fixtures (`cross_domain.rs:1537`) but that is Cross-Domain correlating a *Service* phase finding with a Music finding, not a distinct Worship detector. | N/A | Yes, as part of Service's own test suite | **NO** - inherits Service's not-live-connected status. |
| Altar Call | **Exists only as an operator-assignable label, never as automatic detection.** `SermonSectionKind::AltarCall` is a real Sermon Foundation section-kind taxonomy value (`core/sermon/src/foundation/section.rs:25`), settable via an explicit operator command (`commands.rs:4014`, persisted/restored in `persistence.rs:955,967`). It has no phrase-pattern detector anywhere in `core/sermon/src/detection.rs` (unlike `PrayerPoint`, which does). Its only possible `SectionOrigin` values today are `OperatorAssigned` and `SystemBoundary`; `SectionOrigin::Inferred` exists in the enum but is explicitly documented as "reserved for a future phase's semantic section inference - never produced by anything in this crate today" (`section.rs:59-61`). `ServicePhase` (Service Intelligence) deliberately excludes an `AltarCall` variant for the same reason - `service_adapter.rs:93-95`: "no reliable cue," a considered exclusion, not an oversight. | N/A (no detector to test) | Live-connected in the sense that an operator's manual selection is real, persisted state - but there is no automatic detection to wire into a router in the first place. Adding one would be new detection logic (phrase patterns, most naturally mirroring how `PrayerPoint` was built), not a wiring fix. |

## Answering the operator's diagram directly

```text
Whisper
   v
Transcript event                    <- TranscriptUpdated (real, fires every final segment)
   v
React/backend transcript handling   <- LiveChurchBrain.tsx:360, display-only
   v
handle_final_transcript             <- pipeline.rs:64
   v
Bible detection                     <- cip_core_service::process_transcript_segment (the ONLY call)
   v
???                                 <- NOTHING. handle_final_transcript returns here.
```

The five other domains (Music-via-lyrics, Sermon, Service Phase,
Cross-Domain, Content) are not broken, not partially wired, and not
missing pieces - they are **complete, tested, working engines that were
deliberately built as manual/diagnostic-only commands**, each with a
doc comment saying so explicitly, going back to when each was first
built (Phase 2.1 through 2.8). This was a real, considered architectural
choice at the time (see `docs/cross-domain-intelligence.md`'s and
`docs/content-intelligence.md`'s persistence-decision sections,
referenced in their own modules) - not an oversight this audit is
"discovering." What *is* new information this phase is confirming, by
name, exactly which engines share this status and which (Music via
acoustic) do not.

## IntelligenceContext - the shared input contract already exists

`crate::intelligence::build_intelligence_context` (wrapped by
`commands.rs::build_music_context`, `:3605` - generic despite its
Music-era name, per that function's own doc comment) is already the
**one, shared, engine-agnostic context builder** every manual
`analyze_*` command calls: it reads the real database, active service,
scripture context manager, recent timeline, recent intelligence
findings, and (Phase 2.5) active sermon/section/segments - fresh, from
real state, every time. Every domain engine (`IntelligenceInput::new`
+ this context) is called the identical way regardless of domain. This
is the load-bearing fact for Step 2 below: **the plumbing a router would
need already exists and is already proven correct** (every manual
command already uses it); a router does not need to invent a new
context-passing mechanism, only decide *when* to call it.

## Why the Intelligence Feed is empty for non-Bible domains during live listening

`unifiedFeed.ts::buildUnifiedFeed` (read in full) merges exactly six
data sources: `suggestions` (Bible, populated live), `musicFindings`,
`sermonFindings`, `serviceTransitions`, `serviceAnomalies`,
`contentCandidates`, `correlations`. All six of the non-Bible sources
are populated *only* by: (a) a manual `analyze_*` button click in
`LiveChurchBrain.tsx`, or (b) the acoustic Music path (a real singing
voice recognized against an installed Music dataset with a real
recognizer configured - not exercised in the operator's own test, per
the known, already-documented "Music Library is legitimately empty"
limitation). During ordinary live listening with no manual button
presses, the feed can only ever show Bible suggestions - this is not a
bug in the feed or a bug in any engine; it is the direct, correct
consequence of no engine besides Bible receiving live transcript
segments at all.

## Step 2 — smallest safe insertion point for a Live Intelligence Router

Per the operator's own proposed architecture:

```text
FINAL SPEECH SEGMENT -> LIVE INTELLIGENCE ROUTER -> [Bible, Sermon, Music, Service, ...]
```

The smallest safe insertion point, confirmed against the actual code
(not designed in the abstract): **inside `handle_audio_chunk`
(`commands.rs`), immediately after the existing call to
`handle_final_transcript`, on the same final-segment branch that
already exists (`for mut segment in segments { if !segment.is_final {
...continue } ... }`)**. This is the exact point where a final segment
is already known, the service id is already known, and
`build_music_context`'s equivalent (already computed once per manual
command, would need to be computed once per final segment here instead)
is the exact, already-proven mechanism for building each engine's
input. A router function here would call `crate::sermon::analyze_and_queue`,
`crate::service::analyze_and_queue`, and the Music engine's text-based
`analyze_and_queue` the same way each manual command already does -
reusing existing, tested functions, adding no new engine logic.

This insertion point deliberately does **not** touch:
- `handle_audio_chunk`'s existing Bible call (`handle_final_transcript`) -
  unchanged, still the first thing that runs.
- The cpal audio callback or `spawn_speech_worker`'s backpressure logic
  (Phase 3.8.7.3) - a router runs on the same speech-worker thread,
  after Whisper has already produced a segment, never before or during
  inference.
- Cross-Domain/Content Intelligence's deliberate "never automatic" design -
  those two are correlation/structuring layers *over* other engines'
  findings, not per-segment engines; routing a transcript segment to a
  per-segment engine (Bible/Sermon/Music-text/Service) is a different,
  narrower change than making Cross-Domain/Content automatic, which the
  operator did not ask for and which would reverse an explicit,
  documented design decision from Phase 2.4/2.7/2.8.

## What this audit does NOT decide

Per the operator's explicit instruction, this audit does not implement
the router, does not change segmentation, and does not touch the
Phase 3.8.7.3 backpressure/stability work. It also does not attempt to
build automatic Altar Call *detection* - `SermonSectionKind::AltarCall`
already exists as an operator-assignable taxonomy value, but giving it
a phrase-pattern detector (mirroring how `PrayerPoint` was built in
`core/sermon/src/detection.rs`) is new detection logic, a separate,
larger design decision from wiring, and out of scope for a router
phase.

## Recommended next phase (for operator decision, not started here)

1. Implement a small `route_to_live_intelligence_engines` function
   called from `handle_audio_chunk` after `handle_final_transcript`,
   reusing `crate::sermon::analyze_and_queue`, `crate::service::analyze_and_queue`,
   and Music's text-based `analyze_and_queue` unchanged.
2. Leave Cross-Domain/Content Intelligence's manual-only trigger exactly
   as designed - not part of this router.
3. Leave transcript segmentation (Whisper's 3.0s buffering window)
   unchanged in this phase - a genuinely separate, larger design
   decision (the operator's own "hybrid segmenter" proposal) that
   deserves its own audit-then-implement cycle, not bundled with the
   router's wiring fix.
4. Measure event-volume/performance impact of calling up to three
   additional engines per final segment before/instead of assuming it's
   negligible - this hardware's own diagnostics (13.9s avg inference)
   already show headroom is not unlimited.
