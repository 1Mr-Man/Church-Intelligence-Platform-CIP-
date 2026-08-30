# Phase 3.8.7.6 — Audit: Service/Prayer/Worship + React/Event + Finding Persistence Trace

Audit-only phase. No production code was modified to perform this
audit; every claim below is a direct citation of code already in the
repository at the baseline commit.

## 1. Baseline

- Branch: `claude/cip-foundation-init-i85g87` (`git branch --show-current`)
- Commit: `d7b2bbb` (`git log -5 --oneline`; this is Phase 3.8.7.5's own commit)
- Working tree: clean (`git status --porcelain`, 0 lines)

## IMPORTANT CORRECTION TO THIS PHASE'S OWN PREMISE

This prompt's "CURRENT KNOWN FACTS" section states the pipeline today
is `CPAL → ... → handle_final_transcript → Bible Intelligence →
Database/events` with **no router yet**, and frames the whole phase as
tracing architecture "before designing the router."

That premise is stale. **The Live Intelligence Router already exists**,
implemented and shipped in Phase 3.8.7.5 (this exact baseline commit,
`d7b2bbb`). `commands.rs::route_segment_to_live_intelligence_engines`
already runs after `handle_final_transcript`, already calls Sermon's,
Service's, and Music-text's `analyze_and_queue` on every bounded
segment, and the Windows installer built from this commit already ships
it (see `docs/phase-3-8-7-5-live-intelligence-router.md` and
`pilot-evidence/3.8.7.5/`).

Per this project's own audit-first, no-guessing discipline - and this
prompt's own explicit instruction to "trace the real code, not
assumptions" - this audit traces the **actual, already-implemented**
router's full downstream path (persistence, events, React), rather than
a hypothetical future one. This is a more useful audit than the one the
prompt anticipated: it verifies the already-shipped implementation
end-to-end instead of re-litigating a design decision already made and
built. Every question the prompt asks is still answered below, against
the real code, with one framing change: "should the router call X" is
answered as "does the router already call X, and is that correct."

## 2. Executive Summary

- Sermon, Service, and Music-text intelligence are live-connected as of
  `d7b2bbb` - confirmed by direct citation of the router's call sites.
- Prayer and Worship are **not** separate engines and never will need
  to be routed separately: `PrayerPoint` is produced inside Sermon's
  `analyze()`, `ServicePhase::Worship` is produced inside Service's
  `analyze()`. Routing Sermon+Service (already done) already surfaces
  both.
- Altar Call has no automatic detector. Confirmed again this phase,
  unchanged from Phase 3.8.7.4.
- **Sermon/Service/Music findings are never persisted to any database
  table.** `FindingQueue` is a plain in-memory `Vec`, explicitly
  documented as a deliberate Phase 2.0 design choice. This is true both
  for the pre-existing manual commands and for the new live router -
  not a regression introduced this phase, a pre-existing architectural
  fact the router inherits correctly.
- The React frontend already renders router-produced findings with
  **zero additional frontend code**, because the router emits the exact
  same Tauri events (`SermonFindingDetected`, `ServicePhaseChanged`,
  `ServiceAnomalyDetected`, `MusicFindingDetected`) the manual commands
  already emitted, and the frontend's listeners are domain-agnostic
  about origin.
- `build_intelligence_context` (the router's context builder) holds
  `state.db` plus three other mutexes for its full duration and always
  clones the *entire* in-memory finding history before truncating to
  20 - a real, identified inefficiency, not fixed this phase (not asked
  for, and not evidenced as a real bottleneck against Whisper's own
  13.9s average inference cost).
- No duplicate-processing risk was found in the current implementation:
  each bounded segment is routed exactly once, `FindingQueue::add`'s
  equivalence check prevents duplicate unresolved findings per domain,
  and the segmenter's flush/reset paths are mutually exclusive (verified
  by Phase 3.8.7.5's own unit tests).

## 3. Service Phase Trace

**A1 - where defined**: `core/intelligence/src/service_adapter.rs:98-108`.
`pub enum ServicePhase { Unknown, Opening, Worship, Prayer,
ScriptureReading, Sermon, Offering, Announcement, Closing }`.

**A2 - what classifies it**: `detect_phase_cues` + `strongest_cue`
(`service_adapter.rs:191-261`) run a fixed table of regex phrase cues
(`PHASE_CUES`, `:201-226`) against `input.transcript_segment.text`,
each tagged `CueStrength::Strong` or `CueStrength::Weak`
(`:172-180`). `ServiceIntelligenceEngine::analyze`
(`:391-493`) is the call chain:

```text
route_segment_to_service (commands.rs)
  -> crate::service::analyze_and_queue (service.rs:113)
    -> ServiceIntelligenceEngine::analyze (service_adapter.rs:391)
      -> detect_phase_cues(text) -> strongest_cue(&cues)
        -> Strong cue, phase changed: transition() + finding_for_transition()
        -> Weak cue, repeated >= WEAK_DEBOUNCE_STREAK(2) times: same, lower confidence (0.6 vs 0.85)
      -> IntelligenceResult::new(findings)
```

This is **phrase/rule-based, not scoring/ML-based** - every finding's
`reason` field embeds the literal matched phrase (`matched_phrase`,
always a verbatim substring, `:230-234`), never a paraphrase or a
learned score.

**A3 - input**: `&IntelligenceInput` (`service_id: Uuid`,
`transcript_segment: TranscriptSegment`, `runtime:
RuntimeCapabilities`) and `&IntelligenceContext` - the engine reads
only `input.transcript_segment.text` and `context.service_status`
(`:408`, gates all analysis to `ServiceStatus::Started` - "no false
phase transitions while paused/ended").

**A4 - does it already detect Worship**: **Yes.**
`ServicePhase::Worship` is a real variant (`:101`). Strong cues:
`r"(?i)\b(let.?s|let\s+us)\s+worship\b"`, `r"(?i)\bworship\s+the\s+lord\b"`,
`r"(?i)\b(let.?s|let\s+us)\s+praise\b"` (`:205-207`); one Weak cue,
`r"(?i)\bworship\b"` (`:208`, needs to repeat twice with no stronger
cue winning first). No separate Worship classifier exists or is
needed.

**A5 - after detection**: `findings.push(finding_for_transition(...))`
(and, if the transition is architecturally implausible,
`push_plausibility_finding` also queues an anomaly finding,
`:496-513`) → returned as `IntelligenceResult` → `route_segment_to_service`
(`commands.rs`) calls `crate::service::analyze_and_queue`, which calls
`findings.add(finding.clone())` on `state.intelligence_findings`
(`FindingQueue`, in-memory - see §7) → the router then does the exact
same event-emission `analyze_service_transcript` already did:
`record_timeline` + `emit(app, ServicePhaseChanged | ServiceAnomalyDetected,
finding.clone())` (`commands.rs::route_segment_to_service`, mirrors
`analyze_service_transcript` verbatim) → React (see §9).

## 4. Prayer Trace

**B1 - separate engine?** **No.** Grep for `Prayer|PrayerPoint|pray` across
`core/` confirms exactly two places Prayer exists, both inside other
engines' own taxonomies:
- `core/sermon/src/taxonomy.rs:53`: `SermonElementKind::PrayerPoint` -
  a Sermon Intelligence taxonomy value.
- `core/intelligence/src/service_adapter.rs:102`: `ServicePhase::Prayer` -
  a Service Intelligence phase value.

There is no `IntelligenceDomain::Prayer`, no standalone
`PrayerIntelligenceEngine`, and no third implementation anywhere.

**B2 - automatic detection**:
- Sermon path: `core/sermon/src/detection.rs:115-118` -
  `shape!(r"(?i)\blet.?s\s+pray\b", SermonElementKind::PrayerPoint)`,
  `r"(?i)\bpray\s+that\b"`, `r"(?i)\bask\s+god\b"`,
  `r"(?i)\bfather,?\s+help\s+us\b"`. `sermon_adapter.rs:331-334` turns a
  detected `PrayerPoint` into `IntelligenceFinding { domain:
  IntelligenceDomain::Sermon, summary: "Prayer Point: {raw}", ... }`
  (confidence 0.85, `ConfidenceSource::Heuristic`, reason "explicit
  prayer-point trigger phrase matched").
- Service path: `service_adapter.rs:210-211` - Strong cues
  `r"(?i)\b(let.?s|let\s+us)\s+pray\b"`, `r"(?i)\bbow\s+(your|our)\s+heads?\b"`
  trigger a `ServicePhase::Prayer` transition finding (domain `Service`,
  summary `"Service phase changed #<n>: ... -> Prayer"`).

Both are real, phrase-anchored, already-shipping detectors - not
placeholders.

**B3 - how the router surfaces Prayer**: Confirmed by reading
`commands.rs::route_segment_to_live_intelligence_engines` directly -
it already calls both `route_segment_to_sermon` and
`route_segment_to_service` on every bounded segment. **No separate
"Prayer engine" call exists or was added** - the router's own doc
comment states this explicitly ("Covers Prayer detection for free").
This is exactly the correct architecture the prompt warns against
duplicating: there is no `Router { Sermon, Prayer engine }` shape in
this codebase.

## 5. Worship Trace

**C1 - independently detected?** **No** - `ServicePhase::Worship`
only, inside Service Intelligence (§3/A4 above). No Music-domain
"worship" concept exists; Music findings are named
`"Song Match: <title>"`/similar (see `music_adapter.rs::finding_for_candidate`),
never phase-labeled. The one cross-reference to "Worship" outside
`service_adapter.rs` is `cross_domain.rs:1537`, a **test fixture**
building a `service_finding(service_id, "Phase: Worship", ...)` to
exercise Cross-Domain's own correlation logic against an already-produced
Service finding - not a second Worship detector.

**C2 - what the router should call**: Already calls
`route_segment_to_service`, which already produces
`ServicePhase::Worship` transitions via the cue table in §3. No
separate Worship classifier exists, is called, or should be added.

## 6. Altar Call Verification

Re-confirmed this phase, unchanged from Phase 3.8.7.4:

1. **Enum/label**: `SermonSectionKind::AltarCall`,
   `core/sermon/src/foundation/section.rs:25`.
2. **How assigned**: Only via an explicit operator command
   (`apps/desktop/src-tauri/src/commands.rs:4014` area - a
   `set`/`open`-style sermon-section command; persisted/restored as the
   string `"altar_call"` in `persistence.rs:955,967`).
3. **Automatic classifier**: **None.** `SectionOrigin` has exactly two
   producible values today, `OperatorAssigned` and `SystemBoundary`
   (`section.rs:52-58`); the third, `Inferred`, is explicitly documented
   as "reserved for a future phase's semantic section inference - never
   produced by anything in this crate today" (`section.rs:59-61`).
4. **Phrase detector**: None in `core/sermon/src/detection.rs` - grepped
   fresh this phase, no `AltarCall` pattern exists there (unlike
   `PrayerPoint`, which has four).
5. **Would live routing detect it automatically?** No - there is
   nothing for a router to call.

```text
AUTOMATIC ALTAR CALL DETECTION:
NOT IMPLEMENTED
NOT TO BE ADDED IN THIS PHASE
```

## 7. FindingQueue Lifecycle

**E1 - producers** (confirmed by direct citation, not inferred):
- `crate::music::analyze_and_queue` (`music.rs`) - called both by the
  manual `analyze_music_transcript` command and by the router's
  `route_segment_to_music_text`.
- `crate::sermon::analyze_and_queue` (`sermon.rs`) - manual
  `analyze_sermon_transcript` and the router's `route_segment_to_sermon`.
- `crate::service::analyze_and_queue` (`service.rs`) - manual
  `analyze_service_transcript` and the router's `route_segment_to_service`.
- Bible is **not** a producer here - Bible's live output is
  `Suggestion`/`ScriptureDetection`, persisted to real SQLite tables via
  `persistence.rs`, an entirely separate mechanism (see §8).
- Cross-Domain Correlation and Content Intelligence write to their
  *own*, separate in-memory queues (`CorrelationQueue`,
  `ContentCandidateQueue` - `AppState.correlation_queue`/
  `content_candidate_queue`), not `FindingQueue`. Confirmed these are
  distinct types, not the same queue Sermon/Service/Music share.

**E2/E3 - consumer and persistence**: `core/intelligence/src/queue.rs`
(read in full). `FindingQueue` is `pub struct FindingQueue { findings:
Vec<IntelligenceFinding> }` - a plain in-memory vector, no database
handle, no serialization to disk anywhere in the type. Its own module
doc states this explicitly: *"Phase 2.0 spec section 31 prefers an
in-memory abstraction over persistence unless persistence is clearly
justified - nothing yet needs a finding to survive a restart (unlike
suggestions/presentation items, which already have their own persisted
tables)."*

**`analyze_and_queue()` alone does NOT persist anything to a database.**
The only "consumers" are read-only accessor methods (`pending()`,
`all()`, `get()`) called by the `list_*_findings`/`list_service_transitions`/
`list_service_anomalies` Tauri commands (`commands.rs:3725, 4139, 5282,
5304`), which filter the **one shared** `AppState.intelligence_findings:
Mutex<FindingQueue>` by `service_id` + `domain` (or a
`is_transition_finding`/`is_anomaly_finding` predicate for Service) at
read time. There is exactly one `FindingQueue` instance in the whole
app, shared across Sermon/Service/Music.

**E4 - database tables**: None. No migration, no table, no
repository/insert function exists for `IntelligenceFinding` anywhere in
`persistence.rs` or the migrations directory. A finding produced by
either the manual command or the live router is lost the moment the
process exits, unless an operator has already explicitly acted on it
through a mechanism that *does* persist (there is none for
Sermon/Service/Music findings themselves - accepting/rejecting one only
changes its in-memory `status`).

**E5 - events emitted** (all confirmed by direct `emit()` call-site
citation in `commands.rs`):

| Event | Payload | Emitter |
|---|---|---|
| `SermonFindingDetected` | `IntelligenceFinding` | `analyze_sermon_transcript`, `route_segment_to_sermon` |
| `SermonStateChanged` | `SermonState` | same two |
| `SermonThemeChanged` | `Option<String>` (theme) | same two |
| `SermonStructureUpdated` | `Vec<SermonPoint>` | same two |
| `ServicePhaseChanged` | `IntelligenceFinding` | `analyze_service_transcript`, `route_segment_to_service` |
| `ServiceAnomalyDetected` | `IntelligenceFinding` | same two |
| `MusicFindingDetected` | `IntelligenceFinding` | `analyze_music_transcript`, `analyze_music_audio`, `route_segment_to_music_text` |
| `TranscriptUpdated` | `TranscriptSegment` | `finalize_and_route_segment` (once per bounded segment) |

Every one of these is a **pre-existing** event name Phase 2.x already
defined - the router (Phase 3.8.7.5) added zero new event variants.

## 8. Manual Command Architecture (Part F)

Traced `analyze_sermon_transcript` end to end (identical shape to
`analyze_music_transcript`/`analyze_service_transcript`):

```text
React button (LiveChurchBrain.tsx, e.g. line ~1640)
  -> commands.analyzeSermonTranscript(text)  [Tauri invoke]
    -> analyze_sermon_transcript (commands.rs:3781, #[tauri::command])
      -> persistence::persist_transcript_segment (a manually-typed segment IS persisted here)
      -> emit(TranscriptUpdated, segment)
      -> build_music_context(&state, service_id)   [locks db, active_service, context_manager, intelligence_findings]
      -> crate::sermon::analyze_and_queue(&state.sermon_engine, &input, &context, &mut findings)
        -> engine.analyze() -> FindingQueue::add()
      -> emit(SermonFindingDetected / SermonStateChanged / SermonThemeChanged / SermonStructureUpdated, ...)
  -> React event listeners update local state (LiveChurchBrain.tsx ~399-413)
  -> unifiedFeed.ts merges into the Intelligence Feed
```

The live router (`route_segment_to_sermon`, `route_segment_to_service`,
`route_segment_to_music_text`, all in `commands.rs`) is a byte-for-byte
copy of this same post-context logic, minus the "persist a manually-
typed transcript segment"/"emit TranscriptUpdated" steps - those
already happened once, for the bounded segment, in
`finalize_and_route_segment` before the router runs.

## 9. Transcript Event Architecture (Part G)

**G1 - where `TranscriptUpdated` is emitted**: Two places today -
`commands.rs::finalize_and_route_segment` (the live path, once per
bounded ~15-18s segment, payload = the full `TranscriptSegment`
struct), and each manual `analyze_*_transcript` command (once per
manually-typed entry). Same event name, same payload shape both times.

**G2 - where listened to**: `LiveChurchBrain.tsx:360` -
`liveEvents.onTranscriptUpdated((segment) => { if (!segment.isFinal)
return; setTranscript((prev) => [...prev.slice(-(TRANSCRIPT_LIMIT -
1)), segment]); ... })`.

**G3 - does the frontend retain the entire transcript?** **No** -
`TRANSCRIPT_LIMIT = 20` (`LiveChurchBrain.tsx:50`), and
`prev.slice(-(TRANSCRIPT_LIMIT - 1))` caps the in-memory array at 20
entries before appending the new one. The *database* keeps the full
history (`persist_transcript_segment` writes every segment
unconditionally); only the React display state is capped. Initial load
on service (re)selection is also bounded: `commands.listTranscript(TRANSCRIPT_LIMIT)`
(`LiveChurchBrain.tsx:291`) fetches the most recent 20 from SQLite, it
does not load the whole service history into memory.

**G4 - expected format**: An array of `TranscriptSegment` objects
(`useState<TranscriptSegment[]>`), never a single growing string. Since
Phase 3.8.7.5's bounded segmentation still produces the identical
`TranscriptSegment` shape (just one covering ~15-18s instead of ~3s),
**no frontend change was required or made** - confirmed by this
session's own Phase 3.8.7.5 regression run showing zero frontend diffs
and an unchanged frontend test count.

## 10. Intelligence Feed Architecture (Part H)

**H1 - how findings appear**: Confirmed dual-path, not polling:
1. **Initial load**: one-shot `commands.list*Findings()`/
   `list_service_transitions`/`list_service_anomalies` calls fire in a
   `useEffect` keyed on `activeServiceId` becoming truthy
   (`LiveChurchBrain.tsx:278-302`) - runs once per service selection,
   not repeatedly.
2. **Live updates**: Tauri event listeners
   (`onSermonFindingDetected`, `onServicePhaseChanged`,
   `onServiceAnomalyDetected`, `onMusicFindingDetected`, plus their
   accept/reject counterparts) push directly into the same React state
   arrays (`LiveChurchBrain.tsx:389-444`), unconditionally on receipt -
   no re-fetch, no polling loop anywhere in this file.

`unifiedFeed.ts::buildUnifiedFeed` (read in full in Phase 3.8.7.4's
audit, re-confirmed unchanged this phase) merges exactly these React
state arrays (`sermonFindings`, `serviceTransitions`, `serviceAnomalies`,
`musicFindings`, plus `suggestions`/`contentCandidates`/`correlations`)
into the one chronological Intelligence Feed list.

**H2 - will a router-generated finding automatically appear?**

```text
YES
```

The router (`route_segment_to_sermon`/`_service`/`_music_text`) emits
the identical event names, with the identical payload shape
(`IntelligenceFinding`), as the pre-existing manual commands. The
frontend's event listeners have no way to distinguish "this finding
came from a button click" from "this finding came from the live
router" - both are just a Tauri event carrying an `IntelligenceFinding`.
No additional existing-integration step, and no new one, is required.
This is verified by construction (the router's own code literally reuses
`emit(app, AppEvent::SermonFindingDetected, finding.clone())` etc.,
copy-pasted from the manual command) and by this session's full
regression pass showing the frontend needed zero changes for Phase
3.8.7.5.

## 11. IntelligenceContext Performance Analysis (Part I)

`build_music_context` (`commands.rs:3886-3917+`) - the function the
router calls once per bounded segment - performs, **while holding**
`state.db`, `state.active_service`, `state.context_manager`, and
`state.intelligence_findings` simultaneously (none of these locks are
scoped to drop early inside this function - all four live until the
function returns):

1. `timeline::list_timeline(&db, service_id, 20)` - one bounded SQL
   query (`LIMIT`-equivalent via the `20` argument) against `state.db`.
2. `persistence::list_transcript_segments(conn, service_id,
   bounds.max_recent_transcript_segments)` (called inside
   `build_intelligence_context`, `intelligence.rs:96-100`) - a second
   bounded SQL query against the **same** `db` handle already locked
   above (`bounds.max_recent_transcript_segments` defaults to 20,
   `context.rs:35`).
3. `content_registry.list(None)` (`intelligence.rs:104`) - an
   **unbounded** SQL query (no `LIMIT`) against `content_registry`'s
   own **separate** connection (not `state.db` - confirmed via
   `integrations/content/src/lib.rs:111-129`, which locks its own
   `self.conn`). In practice this table only ever holds registered
   *datasets* (Bible translations, Music song packs), not per-segment
   or per-finding rows, so its row count does not grow during a live
   service - unbounded in code, bounded in realistic data volume.
4. `state.intelligence_findings.lock()....all().into_iter().cloned().collect()`
   (`commands.rs:3900-3907`) - clones **every** `IntelligenceFinding`
   ever queued this session, across all three domains, before
   `IntelligenceContext::build` truncates the result down to 20
   (`DEFAULT_MAX_RECENT_FINDINGS`, `context.rs:36,180`). **This is the
   one real, identified inefficiency**: the clone cost grows linearly
   with total findings-ever-produced this service (realistically dozens
   to low hundreds over a 2-3 hour service, not thousands - each clone
   is a small struct, not deep data) rather than being bounded at the
   source.

**Answers to the prompt's five explicit questions**:
1. Which queries: two bounded `SELECT`s against `state.db` (timeline,
   transcript segments), one unbounded `SELECT` against a separate,
   dataset-sized connection (content registry).
2. Does it read entire history: no for transcript/timeline (both
   `LIMIT`ed at the SQL layer); yes for in-memory findings (cloned in
   full before truncation, see above).
3. Are results bounded: yes, everything downstream of
   `IntelligenceContext::build` is truncated to 20 per category
   (`ContextBounds::default()`); the pre-truncation *cost* of the
   findings clone is not bounded at the source.
4. Could repeated calls become expensive over 2-3 hours: the two SQL
   queries stay flat-cost (bounded `LIMIT`, indexed by `service_id`
   already for transcript/timeline per existing schema/migrations);
   the findings clone grows slowly and linearly but off a small base
   (in-process struct clones, not I/O) - not evidenced as a real
   bottleneck against this hardware's own 13.9s average Whisper
   inference (Phase 3.8.7.4's own diagnostics).
5. Does it require locks that could block speech processing: **yes,
   `state.db` specifically** - it is the same single
   `Mutex<rusqlite::Connection>` every Tauri command (approve a
   suggestion, list history, run diagnostics) also locks. Since the
   router already runs synchronously on the speech worker thread
   (unchanged from Phase 3.8.7.5's design), a manual operator action
   needing `state.db` at the same moment would block for the duration
   of the router's two SQL queries plus whatever `handle_final_transcript`
   itself was doing just before - bounded by millisecond-scale SQLite
   query time, not by Whisper's multi-second inference time (that
   lock, `speech_engine`, is a separate mutex, already decoupled from
   `state.db` since Phase 3.8.7.2/3.8.7.3). Real, but not evidenced as
   severe; not fixed this phase per the audit-only scope.
6. Should it run synchronously inside the speech worker: it already
   does (Phase 3.8.7.5's design, unchanged) - this audit did not find
   evidence that this is currently a problem, only that it is a
   coupling worth knowing about if a future phase adds a much heavier
   context consumer.

## 12. Duplicate Processing Risks

Checked directly against the current implementation:

- **Same segment analyzed twice?** No - `TranscriptSegmenter::push`
  returns `Some` exactly once per closed window (verified by Phase
  3.8.7.5's own unit tests, e.g.
  `a_new_window_starts_clean_after_a_flush_no_reprocessing_of_old_text`),
  and `flush_remaining()` returns `None` if the buffer is already empty
  (`flush_remaining_returns_none_when_nothing_is_buffered`) - so the
  stop-mid-window path can never re-flush a window the normal path
  already closed.
- **Duplicate findings written?** `FindingQueue::add`
  (`queue.rs:41-53`) rejects a new finding if an equivalent
  (same service/domain/kind/summary, per `IntelligenceFinding::is_equivalent_to`)
  finding is still `Detected`/`Reviewed` (not yet resolved) -
  `QueueAddOutcome::DuplicateIgnored`. This applies identically whether
  the producer was a manual command or the live router; the router adds
  no new dedup logic and needs none, since it reuses the same `add()`.
- **Duplicate events emitted?** No - each `route_segment_to_*` function
  only emits for findings actually returned by `analyze_and_queue`
  (i.e., ones `FindingQueue::add` accepted), matching the manual
  commands' own behavior exactly.
- **Duplicate transcript records?** No - `finalize_and_route_segment`
  calls `handle_final_transcript` (which persists the transcript
  segment) exactly once per bounded segment, before the router runs;
  the router itself never calls `persist_transcript_segment`.
- **Cross-domain double-counting?** Each `route_segment_to_*` function
  operates on its own domain's `IntelligenceEngine`/`FindingQueue`
  filter; Bible (via `handle_final_transcript`, unchanged) and
  Sermon/Service/Music (via the router) never call into each other's
  detection logic. A single segment mentioning both a Bible reference
  and a prayer phrase correctly produces one Bible suggestion and one
  Sermon `PrayerPoint` finding - two distinct, correctly-attributed
  records, not a duplicate.

No duplicate-processing defect was found in the current implementation.

## 13. Recommended Router Insertion Point

This is a **retrospective** answer, since the router already exists.
Re-verifying it against the actual code: `finalize_and_route_segment`
calls `route_segment_to_live_intelligence_engines` immediately after
`handle_final_transcript` returns `Ok`, still on the speech worker
thread, still after Bible detection and persistence have already
completed for this exact bounded segment. Comparing against the
prompt's four candidates:

```text
A. Inside handle_audio_chunk        - too early: no bounded segment exists yet at this granularity (raw ~3s Whisper windows)
B. Inside handle_final_transcript   - would couple the router to Bible's own persistence function, breaking the "each engine module owns its own logic" convention
C. After handle_final_transcript    - CHOSEN (and already implemented this way)
D. Separate worker after transcript events - would require a second thread/channel and duplicate the generation-guard/backpressure-interaction logic Phase 3.8.7.3/3.8.7.5 already solved once
```

C is correct, confirmed against real lock/lifetime behavior (§11): it
runs after Bible's own `db` lock scope has already closed (no
nested-lock risk), on the same thread Phase 3.8.7.3 already protected
from cpal's real-time contract (the router never touches the audio
callback or the channel/backpressure logic), and it reuses
`finalize_and_route_segment`'s single call site for both the normal
and stop-mid-window flush paths (no duplicated logic between the two).

## 14. Exact Minimal Future Change

There is no Phase 3.8.7.7 required by this audit's findings - the
router, persistence reuse, and event reuse are all already correct.
The one real, non-urgent finding worth a future look:

```text
Files likely to change (if ever addressed):
- apps/desktop/src-tauri/src/commands.rs (build_music_context: avoid
  cloning the full FindingQueue before truncation - e.g. add a
  FindingQueue::most_recent(n) accessor that only clones the tail)

Files that must NOT change for this:
- Whisper worker (spawn_speech_worker, handle_audio_chunk's feed_audio call)
- CPAL callback / audio queues
- Backpressure/overload-drain logic (Phase 3.8.7.3)
- Database schema (no migration needed - FindingQueue's in-memory
  design is a deliberate, documented Phase 2.0 choice, not something
  this finding argues for reversing)
- React/frontend (no change needed - already correct, per §9/§10)
```

This is explicitly **not** a recommendation to implement anything now -
per this phase's own audit-only scope, this is documented as a finding
for a future phase to decide on, not acted on here.

## 15. Final Gate

| Gate | Status |
|---|---|
| Service trace | PASS |
| Prayer trace | PASS |
| Worship trace | PASS |
| Altar verification | PASS |
| Finding persistence trace | PASS |
| Event trace | PASS |
| React transcript trace | PASS |
| Intelligence Feed trace | PASS |
| Router architecture determined | PASS (retrospectively confirmed correct) |
| Production code modified | NO |
