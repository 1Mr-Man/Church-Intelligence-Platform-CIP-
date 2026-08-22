# Live Service Intelligence & Operator Workflow (Phase 1.3)

This document explains the operational layer Phase 1.3 built around the
Phase 1.2 live pipeline: the service lifecycle, the operator's suggestion
queue, the service timeline, and the recovery/offline/archive behavior
that makes CIP a *trustworthy* live-service assistant rather than just a
working detector. [`docs/live-speech.md`](live-speech.md) still owns
audio/speech/the transcript pipeline itself; this document only covers
what sits around it.

**Core principle, unchanged from every earlier phase:** CIP suggests, the
human operator decides. Nothing in this phase adds a path from
`Detected` to `Approved`/`Projected` that skips an explicit operator
action - see "No automatic projection" below.

**Not in this phase:** song/hymn recognition, sermon intelligence,
automatic bullet extraction, a web research engine, online Bible
fallback, content generation, the full presentation designer, OBS/vMix
integration, cloud sync, remote operator accounts, a mobile app. See
[`README.md`](../README.md) for the full phase boundary. Phase 1.4 built
the presentation preparation path (an approved suggestion or a manual
reference through to real, persisted, prepared output) on top of the
operator workflow this document describes, without changing any of it -
see [`docs/presentation.md`](presentation.md). Phase 1.5 built the
content/dataset foundation underneath the whole pipeline - the Content
Registry, the Bible dataset importer/integrity checker, verse-range
retrieval, and local search - and ran a realistic scripted service through
this exact operator workflow (context retention, context replacement,
false-positive protection, operator override, dataset-validation
authority) end to end, again without changing the service lifecycle,
timeline, or suggestion workflow this document describes - see
[`docs/content-registry.md`](content-registry.md),
[`docs/bible-datasets.md`](bible-datasets.md), and
[`docs/full-service-validation.md`](full-service-validation.md). Phase 2.0
established the shared intelligence architecture (`core/intelligence`)
that future Music/Sermon/Content engines will sit behind - the live
transcript pipeline this document describes
(`pipeline.rs::handle_final_transcript`) is completely unchanged; the
Bible compatibility adapter is exercised only by a diagnostic command
and its own tests, not by anything in the live service path - see
[`docs/intelligence-architecture.md`](intelligence-architecture.md).
Phase 2.1 registered the second real engine, Music, alongside it and
added a "Music Intelligence" panel to the Live Church Brain UI (manual
transcript analysis, pending-finding accept/reject, and ad hoc song
search) - also not wired into the live audio/speech pipeline itself, the
same deliberate boundary the Bible adapter keeps. See
[`docs/music-intelligence.md`](music-intelligence.md).

Phase 2.2 is the first phase to wire a second real-time consumer into
the live audio stream: an acoustic recognition worker, running on its
own background thread, fed from `start_listening`'s existing sink
closure alongside (never replacing) the speech-engine feed.
`pipeline.rs::handle_final_transcript` remains completely unchanged -
the acoustic worker is a structurally separate path that only ever
writes to the same in-memory `FindingQueue` Music findings already use.
See [`docs/acoustic-music.md`](acoustic-music.md).

Phase 2.3 registered a third real engine, Sermon, alongside Bible and
Music, and added a "Sermon Intelligence" panel to the Live Church Brain
UI (current theme/state/main point, recorded structure, manual transcript
analysis, and pending-finding accept/reject) - manual-command-only, the
same deliberate boundary the Bible and Music adapters keep;
`pipeline.rs::handle_final_transcript` is unmodified. See
[`docs/sermon-intelligence.md`](sermon-intelligence.md).

## Service lifecycle

```
PLANNED -> LIVE -> PAUSED -> LIVE -> COMPLETED
         (start)  (pause)  (resume)  (end)
```

`core/service::ServiceSession`'s `ServiceStatus` (`Started` / `Paused` /
`Ended`, unchanged since Phase 1.0) is the source of truth; `LiveStatus`'s
`serviceStatus` (`planned` / `live` / `paused` / `completed`) is the
display-level mapping `get_live_status` computes from it plus whether a
service is tracked in `AppState` at all (`planned` = none tracked yet).

Five Tauri commands drive it: `start_service`, `pause_service`,
`resume_service`, `end_service` (all four return the updated
`ServiceSession`), and `get_live_status` for polling. Each is a thin
wrapper around a pure guard function in `commands.rs` (unit-tested
independent of Tauri - see "Testing" below):

- **`ensure_no_active_service`** - `start_service` refuses to run while
  `AppState::active_service` holds any session at all, regardless of its
  internal status. "Every service must have a distinct service ID" and "do
  not create a new service when resuming" both follow from one rule: only
  `end_service` ever clears `active_service` back to `None`, so its mere
  presence is proof a service is already live-or-paused. Starting a second
  one on top of it would silently orphan the first (still `started` in the
  database, unreachable from the running app).
- **`ensure_service_status`** - `pause_service` only succeeds from
  `Started`; `resume_service` only succeeds from `Paused`. Pausing an
  already-paused or never-started service, or resuming a service that
  isn't paused, is a reported `InvalidInput` error, not a silent no-op.

### Start

Creates a new `ServiceSession`, persists it, records `SERVICE_STARTED` on
the timeline, and emits the event. The Scripture Context Manager
(`AppState::context_manager`) is a single long-lived instance shared
across the whole app process, not reset per service - starting a new
service does not clear whatever chapter context happened to be active
from a previous one, since a fresh service should reasonably start
without stale context either way in practice (the operator always names a
new chapter at the top of a service). No suggestions or history are ever
deleted - "do not destroy historical service data."

### Pause

Stops audio capture via `AudioEngine::pause()` where the backend supports
it (`CpalAudioEngine` does), falling back to `stop()` if pausing isn't
supported or fails - either way, best-effort: a capture failure never
blocks the service record itself from pausing. Transcript history,
detections, suggestions, and the active Scripture context are left
exactly as they are; pausing is a status change, not a teardown. Records
`SERVICE_PAUSED`, `AUDIO_STOPPED`, and `SPEECH_STOPPED` on the timeline.

### Resume

Flips the status back to `Started` and calls `AudioEngine::resume()`
best-effort (a failure is logged, not fatal - the operator can retry via
`start_listening`). Transcript sequence numbering and Scripture context
continue from exactly where they were: neither is reset by pause, so
resuming never re-establishes context from scratch or restarts numbering
at zero.

### End

Unchanged from Phase 1.2, now also timeline-recorded (`SERVICE_ENDED`):
stops audio capture best-effort, finalizes the session's `ended_at`
timestamp, persists the final status. Transcript, detections,
suggestions, and timeline are never deleted - they become the service's
permanent archive entry (see "Service history" below).

## Service timeline

Reuses the existing `audit_events` table (defined in Phase 1.0's
`0001_initial_schema.sql`, unused by any code until this phase) instead
of introducing a second event bus or a redundant timeline table -
`apps/desktop/src-tauri/src/timeline.rs`'s `record_event`/`list_timeline`
are the only new code. An entry is exactly `{ event_name, category,
payload, created_at }` (plus `id`/`service_id`); the Live Church Brain
derives a human-readable line ("09:16:07 Romans 8:28 suggested -
confidence 98%") from `event_name` + `payload` via the frontend's
`describeTimelineEntry` (`lib/timelineFormat.ts`) rather than the backend
pre-formatting and storing a description string - "do not duplicate
information unnecessarily."

Recorded events (`AppEvent`, extended this phase with `ServiceResumed`,
`SpeechStarted`/`SpeechStopped`, `ErrorOccurred`,
`ScriptureContextCorrected`, `ScriptureAmbiguousResolved` - six new
variants on the same enum, no new event architecture):

```
SERVICE_STARTED, SERVICE_PAUSED, SERVICE_RESUMED, SERVICE_ENDED
SCRIPTURE_DETECTED, SCRIPTURE_UPDATED
SCRIPTURE_CONTEXT_CORRECTED, SCRIPTURE_AMBIGUOUS_RESOLVED
SUGGESTION_CREATED, SUGGESTION_APPROVED, SUGGESTION_EDITED, SUGGESTION_REJECTED
AUDIO_STARTED, AUDIO_STOPPED, SPEECH_STARTED, SPEECH_STOPPED
PRESENTATION_PREPARED
ERROR_OCCURRED
```

`SPEECH_STARTED`/`SPEECH_STOPPED` are recorded alongside
`AUDIO_STARTED`/`AUDIO_STOPPED` rather than independently: in this
architecture speech processing is driven entirely by audio chunks
arriving (`commands::handle_audio_chunk`), so there is no separately
observable "speech started" moment - recording them together is an
honest reflection of the real control flow, not an invented distinction.
`ReferenceKind::Unresolved` detections are still never recorded (too
frequent/noisy - unchanged Phase 1.2 policy) and, per Phase 1.2, never
persisted as `scripture_detections` rows either.

`list_timeline(serviceId?, limit)` returns entries oldest-first, bounded
by `limit` - the Live Church Brain polls it every 3 seconds alongside
`get_live_status` while a service is active.

## Current / Recent / History

Three distinct levels, deliberately never collapsed into one "what's
happening" view (section 9/20):

- **Current** - `activeContext` (the active chapter, from the most recent
  `ScriptureContext`) and `lastReference` (the most recently resolved
  verse) are tracked as two *separate* pieces of frontend state, because
  they answer different questions: "what chapter is the pastor teaching
  from" vs. "what specific verse was just mentioned." A bare chapter
  reference updates only the former; a resolved verse updates both.
- **Recent** - a client-side, bounded (8-entry) list of recently resolved
  references, built from the same `SCRIPTURE_DETECTED`/`SCRIPTURE_UPDATED`
  event stream that updates Current. Purely a read-only display list.
- **History** - the full service timeline (above) plus the service
  archive (below). Fetched on demand / polled periodically, not pushed
  live for every event.

**Viewing history never mutates Current.** The service archive's detail
view (`viewHistoryService` in `LiveChurchBrain.tsx`) stores its own
`historyDetail` state, entirely separate from `activeContext`/
`lastReference`/`suggestions`/`timeline`. Opening a past service's
timeline while another service is live cannot and does not change what
the live view shows - this is a structural guarantee (different React
state, populated by a different code path), not a convention that could
be violated by a future edit without deliberately wiring the two
together.

Ambiguous detections get their own small, session-only, bounded (5-entry)
list (`ambiguous` state) - see "Ambiguity" below for why they're
deliberately not persisted or included in "Recent."

## Live transcript

Unchanged persistence/bounding from Phase 1.2 (a bounded 20-segment
live window client-side; the complete transcript lives in SQLite,
fetched by `list_transcript` with an explicit `limit`). New this phase:
a real per-segment timestamp. `TranscriptSegment` carries no wall-clock
field (`startMs`/`endMs` are audio-relative, not clock time - see its
own doc comment), so the frontend records the one honest timestamp
available: when it actually received the segment via
`TRANSCRIPT_UPDATED`. A segment loaded from persisted history (before any
live event fired, e.g. right after opening the app mid-service) has no
recorded receipt time and simply shows none, rather than a fabricated
"now."

## Scripture context panel

Shows **Active Context** (book + chapter + confidence level) and
**Current Reference** (the most recently resolved verse) as two clearly
separate fields - never merged into one line - matching the domain
distinction `ScriptureContext.book`/`.chapter` vs. a resolved
`ScriptureReference` already made in Phase 1.1's core types.

## Suggestion queue

Grouped into **High / Medium / Low Confidence** sections
(`SuggestionGroups` in `LiveChurchBrain.tsx`), using the
`ConfidenceLevel` (`cip-core-confidence`) every suggestion already
carries - no new scoring logic, just display grouping. Only `Pending`
suggestions are shown (the existing `list_suggestions("pending")` call);
`Ambiguous`/`Unresolved` detections never enter this queue at all (see
"Ambiguity"). Each card shows the reference, confidence percentage, and -
new this phase - its **source text**: the transcript substring that
produced it (`Suggestion.sourceText`, populated by
`pipeline::handle_final_transcript` from the `TranscriptSegment` it's
processing, since `core/service`'s pure text pipeline never sees segment
*identity*, only segment *text* - see `core/ai::Suggestion::with_source`).

### Approve / Edit / Reject

All three go through `ensure_suggestion_editable` (`commands.rs`): only a
`Pending` or `Edited` suggestion may be approved, edited, or rejected -
attempting any of the three on an already-`Approved` or already-`Rejected`
suggestion is a reported error, not a silent overwrite. Each action is
timeline-recorded (`SUGGESTION_APPROVED`/`EDITED`/`REJECTED` with the
suggestion id and reference).

**Edit validation (section 17):** `edit_suggestion` parses the operator's
replacement reference the same way `prepare_presentation` parses a
suggestion's own reference, then calls `BibleProvider::get_verse` -
exactly like an automatically-detected reference, an edited one must be a
real, validated verse before it can become `Edited`. An invalid edit
(a typo, a nonexistent verse number) is rejected with a clear error and
the suggestion's status never changes.

## Ambiguity

An `Ambiguous` detection (`core/bible`'s existing context-replacement
ambiguity heuristic, unchanged) is shown in its own panel with every
candidate and its confidence, plus **Select**/**Dismiss** actions - never
silently guessed, never auto-approved. Selecting a candidate calls
`resolve_ambiguous_reference`, which:

1. Independently re-validates the chosen candidate against the
   `BibleProvider` (never trusts the frontend's copy of the candidate
   blindly).
2. Commits it to the Scripture Context Manager
   (`context.record_resolved`) exactly like an automatically-resolved
   verse would be.
3. Persists a `scripture_detections` row and creates a `Pending`
   `Suggestion` - the operator's choice enters the normal approval queue
   like any other suggestion, it does not skip review.
4. Records `SCRIPTURE_AMBIGUOUS_RESOLVED` on the timeline with the full
   candidate set that was shown and which one was chosen, for audit.

Ambiguous detections themselves are **not** persisted to
`scripture_detections` (the schema has no "ambiguous" status - unchanged
Phase 1.2 policy) and are not part of the service archive; they exist
only in frontend memory for the duration they're unresolved, bounded to 5
at a time. This is a deliberate, documented scope decision: nothing about
an unresolved ambiguity is lost from the *transcript* record (the
originating segment is persisted regardless), only the "here's an open
question" UI state doesn't survive a restart.

## Manual context correction

`correct_scripture_context(book, chapter)` lets the operator fix a
misunderstood context: validated against the `BibleProvider` exactly like
an automatic chapter detection (`get_chapter` must return a real
chapter), then calls the same `ScriptureContextManager::resolve()` an
automatic detection would, so subsequent bare verses resolve against the
corrected chapter immediately. Recorded on the timeline
(`SCRIPTURE_CONTEXT_CORRECTED`, with the previous and corrected
book/chapter) for audit. **Never rewrites transcript content** - the
correction only changes context going forward; the segment(s) that led to
the original misunderstanding remain exactly as spoken in the transcript
record. Proven directly:
`pipeline::tests::operator_context_correction_updates_active_context_without_altering_transcript_history`.

## Deduplication policy

A pastor repeating "Romans 8:28" mid-explanation should not flood the
suggestion queue with identical suggestions - but a genuine repeat later
in the service must never be silently suppressed. The policy
(`persistence::has_recent_suggestion_for_reference`, applied in
`pipeline::handle_final_transcript`):

> Within one service, a new suggestion for a given reference is created
> only if no suggestion for that same reference was already created in
> this service within the last **60 seconds**, regardless of that
> suggestion's status (pending/approved/edited/rejected). After the
> window elapses, an identical reference is a legitimate fresh mention
> and produces a new suggestion.

Session-scoped (never cross-service, never permanent/global) and
time-window-based, matching the explicit requirement. The underlying
*detection* is never suppressed - every final transcript segment and
every validated `scripture_detections` row is always persisted; only
suggestion-queue spam is filtered, at the pipeline layer, before
persistence and before the `SUGGESTION_CREATED` event fires (so a
suppressed duplicate never even flashes into the UI).

## Manual Bible search

Unchanged from Phase 1.2: `search_bible` requires no speech, no audio, no
network - available at every point in the UI regardless of
audio/speech/network/AI status. Phase 1.3 adds no gating around it.

## Failure recovery

| Failure | What happens |
| --- | --- |
| **Audio** (device disappears, capture fails to start) | `start_listening`'s failure is recorded in `AppState::audio_error`, reported as `AudioStatusKind::Error` (distinct from `Unavailable` - a real, in-progress failure vs. simply no device) by `get_live_status`, and timeline-recorded as `ERROR_OCCURRED`. The service stays `Live`; transcript/suggestions already gathered are untouched; manual entry and search remain available. The operator retries via the same "Start Listening" control (now labeled "Retry"); a success clears `audio_error`. |
| **Speech** (engine returns an error for one chunk) | Recorded in `AppState::speech_error`, reported as `SpeechStatusKind::Error`, timeline-recorded. Only that one audio chunk is dropped - the service, transcript, and suggestions are all untouched, and the very next successful `feed_audio` call clears the error automatically. |
| **Network** | Never a fatal error anywhere in this pipeline - `networkStatus` is a status indicator only (`check_network_online`'s short TCP probe), read by nothing else. Service controls, local Bible lookup, context, transcript, suggestions, approval, and the timeline all continue to function with zero network access, proven structurally (`cargo tree` shows no HTTP client in the pipeline's dependency graph) and by test (`the_pipeline_produces_identical_results_with_no_network_access_possible`, and the canonical scenario test's "disconnect network and continue" segment). |
| **Database** | A persistence failure during the live pipeline (e.g. `persist_transcript_segment` failing) is logged and timeline-recorded as `ERROR_OCCURRED` rather than silently swallowed; the in-memory runtime state (active service, context) is untouched, so the next segment gets a fresh, independent attempt. `get_live_status` also actively pings the database (`SELECT 1`) every poll and reports `DatabaseStatusKind::Error` if that fails, rather than assuming a prior successful connection stays good forever. |

None of these ever end the service automatically or crash the
application - "the service must remain operational where possible."

## Live status dashboard

`get_live_status` reports six *independent* signals - never collapsed
into one "backend connected" boolean, since each answers a different
operator question:

```
RUNTIME    tauri | web          (frontend-only - see docs/live-speech.md's web-runtime section)
SERVICE    planned | live | paused | completed
AUDIO      unavailable | ready | listening | error
SPEECH     unavailable | ready | error
NETWORK    offline | online
AI         available | degraded | unavailable
DATABASE   connected | error
```

`runtimeStatus` isn't a backend field at all - the Live Church Brain only
ever mounts inside the Tauri runtime (the web build renders
`WebRuntimeNotice` instead, per Phase 1.2.1, never reaching
`get_live_status`), so the fact this command executed at all already
proves `tauri`. `aiStatus` remains derived only from `speechStatus`,
never `networkStatus` (unchanged Phase 1.2 policy - a fully offline
machine with a working local model is `available`).

Honesty note on scope: `SpeechStatusKind` has three real states
(`unavailable`/`ready`/`error`) - not the five sometimes imagined for a
speech pipeline (`unavailable`/`loading`/`ready`/`processing`/`error`).
Nothing in this architecture today observably distinguishes "loading a
model" or "mid-inference" from "ready" without deeper engine-side
instrumentation this phase didn't add; claiming those two extra states
existed would have been fabricating status the engine doesn't actually
report. Only the three states genuinely computable from the code Phase
1.2/1.3 built are exposed.

## Web mode

Unchanged from Phase 1.2.1 and re-verified this phase: opening the web
build outside Tauri never calls `invoke`/`listen`, shows the existing
`WebRuntimeNotice`, and makes no attempt to present native
audio/speech/live-service functionality as available. See
[`docs/live-speech.md`](live-speech.md#cip-web-vs-cip-desktop-phase-121).

## Keyboard shortcuts

Implemented, not just architected (section 31): `A` approve / `R` reject /
`E` start editing / `P` preview-prepare the **first pending suggestion**;
`S` focuses the manual search box. Every handler is gated by
`shouldHandleShortcut` (`lib/keyboardShortcuts.ts`), a pure function
tested independent of the DOM: it refuses to fire whenever a modifier key
is held, or the focused element is an `input`/`textarea`/`select`/
content-editable node - typing "search" into the search box can never be
misread as pressing `S`/`E`/`A`/`R`. Approve/reject are additionally
disabled while that exact action is already in flight (`busy` state), so
a held key can't double-submit.

## Service history (the archive)

`list_service_history(limit)` lists `Ended` services, most recently
started first; `get_service(serviceId)` and
`list_timeline(serviceId, limit)` (both accepting an explicit
`serviceId` override of the implicit "active service" every other
Phase 1.2 command used) let the operator inspect one completed service's
timeline independent of whichever service, if any, is currently live.
This is the beginning of the archive, not a full analytics system: title,
start/end time, and the timeline (from which references
detected/approved are derivable) - nothing more.

## Restart / recovery

Nothing is resumed automatically on restart - "do not automatically
resume live audio after restart unless explicitly designed and tested,"
and nothing here was. A fresh launch starts with no active service
(`AppState::active_service` is always `None` at startup) and no audio
capture running; the operator explicitly starts a new service or opens
history to review a past one. What *is* proven to survive a restart:
every persisted row. `pipeline::tests::service_history_survives_a_simulated_application_restart`
uses a real file-backed SQLite database (not a kept-open in-memory
connection): it persists a transcript segment, an approved suggestion,
and a timeline entry, drops the connection entirely (simulating the
application closing), reopens a fresh connection to the same file
(simulating a new launch), and verifies every piece of data - and the
service's `Ended` status - reads back correctly.

## Auditability

Every operator action this phase adds records on the timeline: approve,
edit (with the original and edited value), reject, ambiguity resolution
(with the full candidate set), context correction (with the previous and
corrected chapter), service start/pause/resume/end. No personal
information beyond what the operator already typed (a service title, a
book/chapter correction) is ever recorded.

## Performance

Each pipeline stage in `handle_final_transcript` (persist transcript,
run detection, persist detections/suggestions) is timed with
`std::time::Instant` and logged at `debug` level under the
`cip::performance` target - "record where practical," not a formal
benchmark harness or a real-time guarantee. Run against the canonical
scenario's in-memory SQLite database in this development environment,
the whole pipeline (persist + detect + persist) for one transcript
segment consistently completed in low single-digit milliseconds; real
hardware, a larger persisted history, or the real Whisper backend's
~3-second inference window (see `docs/live-speech.md`) would all shift
these numbers, so no hard latency claim is made beyond "logged and
observably fast on the tested path." Nothing in the pipeline blocks the
Tauri command thread pool - `handle_audio_chunk` runs entirely on the
`AudioEngine`'s own capture thread, unchanged from Phase 1.2.

## Memory

Bounded client-side windows, unchanged in spirit from Phase 1.2, extended
to the new UI pieces this phase adds:

- Live transcript: 20 segments.
- Timeline (live view): 50 entries, refreshed by polling, not accumulated
  indefinitely.
- Recent references: 8 entries.
- Ambiguous detections: 5 entries.

The complete history of all four lives in SQLite; every bounded list here
is a *display* window, not the source of truth.

## Testing

Consistent with Phase 1.2's documented decision, this project has no
`tauri::test::mock_builder()` harness: every command in `commands.rs`
takes the concrete `tauri::AppHandle`/`tauri::State` (not generic over
`R: Runtime`), so exercising them through Tauri's mock runtime would
require making every command signature generic - a redesign of the
command layer, not a test-only addition, and out of this phase's scope
("do not redesign the architecture"). Instead:

- **Pure guard logic** extracted out of each new command
  (`ensure_no_active_service`, `ensure_service_status`,
  `ensure_suggestion_editable` in `commands.rs`) is unit-tested directly,
  the same pattern `parse_uuid`/`parse_display_reference`/
  `parse_suggestion_status` already established in Phase 1.2.
- **Everything else new this phase** - lifecycle persistence, timeline
  read/write, deduplication, context correction, ambiguity resolution,
  the full canonical scenario, and restart recovery - is tested directly
  against `persistence.rs`/`pipeline.rs`/`timeline.rs` with a real
  SQLite-backed `BibleProvider`, replicating exactly the steps the
  corresponding Tauri command performs (validate, mutate context, persist,
  record timeline) without the `AppHandle`/event-emission wrapper around
  them - the same boundary Phase 1.2's own pipeline tests already draw
  ("event emission... is the caller's job," tested separately from
  persistence/pipeline logic).
- **The Phase 1.1 deterministic acceptance test remains untouched and
  still passes** - `bible_intelligence_acceptance.rs`'s Romans 8 -> John 3
  sequence, unmodified.

```sh
cargo test --workspace
cargo test -p cip-desktop
cargo test -p cip-integration-tests --test bible_intelligence_acceptance
```

## Cross-domain correlation (built under an earlier internal "Phase 2.4" label)

The Live Church Brain's "Cross-Domain Intelligence" panel is a separate,
read-only-except-review/dismiss view layered on top of the workflows
above - it never changes how Scripture detection, Music, or Sermon
findings themselves are produced or reviewed. See
[`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md). Under
this repository's authoritative Phase 2 roadmap, this work is reserved
for formal validation as Phase 2.8; the roadmap's actual Phase 2.4 is
Service Intelligence - see
[`docs/service-intelligence.md`](service-intelligence.md).

## Sermon Foundation (Phase 2.5, per the authoritative Phase 2 roadmap)

The "Sermon Foundation" panel is a separate structural layer: which
sermon is active, who is speaking, and which section it's in - never a
semantic claim about what was said (that remains the "Sermon
Intelligence" panel's job - the roadmap's actual Phase 2.6, built under an
earlier internal "Phase 2.3" label and extended in place under Phase 2.6
to read this foundation's context: every Sermon finding now carries this
panel's active sermon's id, and a candidate section it's about to move
into). It reuses this document's own transcript/timeline/event
architecture unchanged - no new event bus, no new persistence mechanism
beyond the additive `sermons`/`sermon_sections`/`sermon_segments` tables.
See [`docs/sermon-foundation.md`](sermon-foundation.md) and
[`docs/sermon-intelligence.md`](sermon-intelligence.md).

## Limitations (stated honestly)

- Ambiguity state is session-only, not persisted (see "Ambiguity" above).
- `SpeechStatusKind` has three real states, not five - see "Live status
  dashboard."
- Performance numbers are logged observations from this development
  environment's test suite, not a benchmarked or guaranteed real-time
  bound.
- Interactive browser automation of the actual keyboard-driven operator
  workflow was not run in this development environment (see
  `docs/live-speech.md`'s equivalent note about the model-download
  blocker) - `shouldHandleShortcut`'s typing-safety guard and
  `describeTimelineEntry`'s formatting are unit-tested directly instead,
  and the full command-layer workflow is proven end to end via
  `pipeline.rs`'s canonical scenario test rather than a live UI click-through.
