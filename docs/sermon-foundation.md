# Sermon Foundation (Phase 2.5, per the authoritative Phase 2 roadmap)

## Roadmap note

This repository's Phase 2 roadmap is:

```
2.0 Intelligence Architecture -> 2.1 Unified Intelligence Event/Context Layer -> 2.2 Music Content Foundation ->
2.3 Music Intelligence -> 2.4 Service Intelligence -> 2.5 Sermon Intelligence Foundation (THIS PHASE) ->
2.6 Sermon Intelligence -> 2.7 Content Intelligence -> 2.8 Cross-Domain Intelligence ->
2.9 Unified Operator Intelligence Workspace -> 2.10 Full Phase 2 Validation
```

An earlier body of work in this repository's history - `core/sermon`'s `detection`/`state`/
`structure`/`taxonomy`/`theme` modules, `core/intelligence/src/sermon_adapter.rs`,
`apps/desktop/src-tauri/src/sermon.rs`, and `docs/sermon-intelligence.md` - was built and
committed under an internal label that also read "Phase 2.3." That label is a historical
commit/doc artifact and is **not** rewritten by this document or this phase: the code, tests,
and docs for that earlier work are unchanged and remain fully intact. Under the authoritative
roadmap above, that work - deterministic *semantic* structural/theme detection over transcript
text (main points, illustrations, themes) - is understood to be **Phase 2.6-equivalent**
("Sermon Intelligence") rather than this phase's own foundation work. Nothing in it was
modified, renamed, or relocated to make room for this phase.

This document describes what "Phase 2.5" means under the authoritative roadmap: **Sermon
Intelligence Foundation** - the durable entity/lifecycle layer Phase 2.6's real Sermon
Intelligence work will build on, distinct from (and prerequisite to) the semantic detection
already present in this codebase.

## 1. Purpose

Sermon Foundation answers a question no existing code in this repository answered before this
phase: **"What *is* a sermon, as a durable thing?"** - its identity, who is speaking, when it
started/paused/resumed/ended, what structural section it's currently in, and which transcript
segments belong to it. It never reads transcript *content* to decide anything semantic; every
fact here is either an explicit operator action or a deterministic structural association (an
id, a timestamp, a status transition).

## 2. Scope

In scope: a `Sermon` entity with an explicit lifecycle; explicit speaker assignment; a closed,
deterministic section taxonomy with explicit and system-boundary origins; transcript-segment
linkage; the `IntelligenceFinding`s and events these explicit actions produce; persistence for
restart recovery and history.

## 3. Non-goals

Sermon Foundation must not, and does not: extract themes, main points, or illustrations from
transcript text (that is the historical `core/sermon` semantic engine's job, understood as
Phase 2.6); infer a section from transcript wording; perform biometric speaker recognition or
diarization; generate a summary, interpretation, or theological claim of any kind; call another
intelligence engine; or automatically create a presentation item.

## 4. Sermon vs. Service

`cip_core_service::ServiceSession`/`ServiceStatus` answer **"which church service is currently
happening"** - unchanged by this phase. `cip_core_sermon::foundation::Sermon`/`SermonStatus`
answer a strictly narrower question: **"which message within that service is being
delivered."** One service may contain worship, announcements, offering, prayer, a sermon, an
altar call, and a closing, none of which `ServiceSession` distinguishes on its own - a `Sermon`
is a smaller, optional-in-time-and-number span inside a service's lifetime. The two identities
(`Sermon.id` vs. `Sermon.serviceId`) are always distinct - proven directly by a unit test and
by the canonical acceptance scenario (see "Testing" below).

## 5. Sermon vs. Transcript

`cip_core_ai::TranscriptSegment` (and its `transcript_segments` table) remains the single
canonical record of what was said - Sermon Foundation never copies transcript text into its
own rows. `SermonSegment` is a thin linkage record (`sermon_id`, `transcript_segment_id`,
`sequence`, `section_id`, `linked_at`) answering "which portion of the transcript belongs to
this sermon," never "what does this portion mean." A transcript segment may exist outside any
sermon, be linked to one, or (via a fresh `link_transcript_segment_to_sermon` call) be linked
to a different one later - but no existing link is ever silently rewritten; a new call adds a
new row, fully auditable.

## 6. Domain model

`core/sermon/src/foundation/` (a new module inside the existing `cip_core_sermon` crate - see
"Engine boundary" below for why a new crate was not created):

| Type | File | Answers |
|---|---|---|
| `Sermon`, `SermonStatus`, `is_valid_transition` | `sermon.rs` | Identity, lifecycle |
| `Speaker`, `SpeakerRole` | `speaker.rs` | Who is speaking |
| `SermonSection`, `SermonSectionKind`, `SectionOrigin` | `section.rs` | Structural spans |
| `SermonSegment` | `segment.rs` | Transcript linkage |

Every type is a plain, pure data value (`Debug`/`Clone`/`PartialEq`/`Serialize`/`Deserialize`) -
no SQL, no Tauri, no dependency on `core/intelligence` (dependency direction stays one-way:
`core/intelligence` depends on `cip_core_sermon`, never the reverse, matching every other
domain crate in this codebase). Ten candidate types were evaluated per the spec's own list;
four were built because they represent stable domain concepts this phase's operator workflow
actually needs. A standalone `MessageBoundary`/`BoundaryEvidence` type was deliberately *not*
built - see "Evidence" below for why the existing `EvidenceSource` enum already covers it.

## 7. Lifecycle

```text
Planned --activate--> Active --pause--> Paused --resume--> Active --end--> Ended
   |
   +--cancel--> Cancelled
```

`SermonStatus` is a separate state machine from `ServiceStatus` (never reused for this
purpose - a non-negotiable rule). `is_valid_transition(from, to)` is a pure, directly-tested
function - the single source of truth every guard (`commands.rs::ensure_valid_sermon_transition`)
delegates to. `Ended`/`Cancelled` are terminal: no transition out of either is ever valid,
including back to `Active` - proven by a dedicated test
(`ended_to_active_is_never_valid_even_though_both_are_real_states`). A same-state call (e.g.
`Active` -> `Active`) is also rejected, mirroring `ensure_service_status`'s existing
"already in that state" handling for `ServiceSession`.

`start_sermon` skips `Planned` and creates a sermon directly `Active` (mirroring
`ServiceSession::start`'s own "no separate planning step" convention) - `Planned`/`Cancelled`
are modeled and tested (`core/sermon`'s own 28 foundation unit tests exercise every documented
transition, including these) but not reachable from any Tauri command in this phase, since
nothing yet needs a "schedule a sermon ahead of time" workflow. See "NOT AVAILABLE."

## 8. Sermon context

`IntelligenceContext` (the existing, shared, cross-domain context every engine reads) gained
three additive fields: `active_sermon: Option<Sermon>`, `current_sermon_section:
Option<SermonSection>`, `recent_sermon_segments: Vec<SermonSegment>` - plus a new
`ContextBounds.max_recent_sermon_segments` (default 20, matching the existing "recent"
collections' order of magnitude). These are populated through a **new builder method**,
`IntelligenceContext::with_sermon_context(...)`, called *after* `IntelligenceContext::build(...)` -
never a new required constructor argument. This is the same additive-extension discipline
`IntelligenceFinding`'s own `with_evidence`/`with_provenance` builder methods already
establish, and it means every one of this crate's 15 existing `IntelligenceContext::build`
call sites (across the Bible/Music/Sermon/CrossDomain/Service adapters and their tests) remains
valid, unmodified source - zero blast radius, fully backward compatible (proven by
`a_plain_build_never_carries_sermon_context`).

`apps/desktop/src-tauri/src/commands.rs::build_music_context` (the shared, real context builder
every domain's manual-analysis command already used) now also attaches sermon context, so every
engine's context (Bible/Music/Sermon/Service alike) can *observe* the active sermon - never a
reason for one engine to call another (see "Engine boundary").

## 9. Segment model

See "Sermon vs. Transcript" above for the core design. Persistence-side, `sequence` is derived
from `count_sermon_segments` (a `SELECT count(*)`) at link time - gapless, starting at 0 per
sermon, and independent of the transcript segment's own service-wide sequence number.
`section_id` records which section (if any) was open at the moment of linking, letting a future
phase answer "what was said during the Illustration section" without inferring anything.

## 10. Section model

A closed, seven-member taxonomy (`Introduction`, `ScriptureReading`, `MainMessage`,
`Illustration`, `Prayer`, `AltarCall`, `Conclusion`) - closed for the same reason
`cip_core_sermon::SermonElementKind` is closed: every assignment is one of a documented set,
never a free-text guess. `SectionOrigin` makes the data model's answer to "how was this
established" explicit and queryable:

- `OperatorAssigned` - an operator explicitly chose it (`change_sermon_section`).
- `SystemBoundary` - a deterministic, unambiguous system fact, not a judgment call. The only
  producer in this phase: `start_sermon` automatically opens an `Introduction` section the
  moment a sermon starts delivering.
- `Inferred` - **reserved, unused**. Exactly like `AssertionLevel::Generated` is reserved-but-
  unused in `core/intelligence`, this variant exists so the enum's shape is ready for a future
  phase's semantic section inference without a breaking change, while nothing in this phase
  ever produces it (see "NOT AVAILABLE").

Only one section may be open at a time (`ended_at IS NULL`): `change_sermon_section` always
closes the previously-open section (with the new section's own `started_at` as the shared
timestamp - no gap, no overlap) before opening the new one, and never deletes the closed
section's row - full history remains queryable via `list_sermon_sections`.

## 11. Speaker model

`Speaker { id, name, role }`, `role` ∈ `{ Primary, Guest }`. Explicit/manual assignment only -
`assign_sermon_speaker` is the one way a sermon gets a speaker; nothing in this phase performs
audio-based speaker recognition or diarization, and a `Speaker` is never confused with a CIP
user/operator account. A sermon has **at most one** speaker in this phase - see "NOT AVAILABLE"
for why multiple-speaker support was deliberately deferred rather than half-built.

## 12. Evidence

No second evidence architecture was invented. Every structural fact this phase produces reuses
`EvidenceSource::OperatorAction` (the existing enum's variant for "derived from an explicit
operator action") - because every mutating action in this phase genuinely *is* one. The spec's
suggested "boundary evidence" taxonomy (OperatorAction/ServiceEvent/ExplicitMetadata/
TranscriptCue/SystemState) maps directly onto `EvidenceSource`'s existing variants
(`OperatorAction`, `ServiceEvent`, `Content`, `Transcript`, `Context` respectively) without a
single new type - confirming the "reuse over invention" bias this phase was built under. No
standalone `MessageBoundary` struct was created for the same reason.

## 13. Provenance

Every finding this phase produces carries `IntelligenceProvenance::unknown()` (no content-
registry-backed source - a structural fact about the live session, not installed content),
exactly like every other operator-action finding in this codebase (e.g. Service Intelligence's
`finding_for_operator_action`). The finding's own `engine_id`/`engine_version`
(`"sermon-foundation"`/`"1.0.0"`) names the source; its `service_id`, `summary` (which always
embeds the sermon id or the exact operator-supplied value), and the timeline row `record_timeline`
writes alongside it together answer every provenance question the spec requires ("which
service/sermon/transcript segment/operator action/engine/when").

## 14. Operator workflow

| Command | Action |
|---|---|
| `start_sermon(title?)` | Begins a new sermon, `Active` immediately, opens `Introduction`. |
| `pause_sermon()` / `resume_sermon()` | `Active` <-> `Paused`. |
| `end_sermon()` | `Active`/`Paused` -> `Ended`; closes any open section. |
| `set_sermon_title(title)` | Explicit title correction/assignment. |
| `assign_sermon_speaker(name, role)` | Explicit speaker assignment/correction. |
| `change_sermon_section(kind, note?)` | Closes the open section, opens a new one. |
| `link_transcript_segment_to_sermon(id)` | Links an already-persisted transcript segment. |
| `get_sermon_foundation_state()` | Read-only: active sermon + current section. |
| `list_sermon_segments()` / `list_sermon_sections()` | Read-only history for the active sermon. |
| `list_sermon_history(limit)` / `get_sermon(id)` | Read-only sermon archive (mirrors `list_service_history`/`get_service`). |

Every mutating action: (1) validates its target (`ensure_no_active_sermon`,
`ensure_valid_sermon_transition`, transcript-segment ownership checks), (2) updates state
(`AppState.active_sermon`/`active_sermon_section` + the `sermons`/`sermon_sections`/
`sermon_segments` tables), (3) emits an `AppEvent`, (4) records a `timeline` entry, (5) queues
an `IntelligenceFinding`. None of it touches `cip_core_presentation` - no code path here is
capable of creating, activating, or otherwise touching a `PresentationItem`.

## 15. IntelligenceContext integration

See "Sermon context" above. `Sermon`/`SermonSection`/`SermonSegment` values are exposed to
every engine exclusively through `IntelligenceContext` - never through a direct call. A
dedicated test (`context.rs`'s Sermon Foundation suite plus the canonical acceptance scenario)
proves the shared-context channel works and that no `BibleIntelligenceEngine`/
`MusicIntelligenceEngine`/`SermonIntelligenceEngine` symbol is ever imported by
`sermon_foundation.rs` or `commands.rs`'s sermon-foundation section.

## 16. Engine boundary

**No `IntelligenceEngine` was implemented, and nothing was registered into
`IntelligenceEngineRegistry`.** Every mutating action in this phase is an explicit operator
action, never transcript-driven inference - there is no `analyze(&self, input, context)` call
to make, so forcing an `IntelligenceEngine` implementation onto this phase would misrepresent
what it does (an engine's defining shape is "consumes a transcript segment and infers
something"; this phase never infers anything from transcript content). Instead,
`apps/desktop/src-tauri/src/sermon_foundation.rs` is a plain, Tauri-agnostic orchestration
module (mirroring `content.rs`/`presentation.rs`) exposing pure finding-constructor functions
the Tauri commands call directly. This satisfies the spec's own explicit permission: "If the
correct architecture is to create only a provider/foundation service now and defer the
`IntelligenceEngine` implementation to Phase 2.6, do that instead."

Capability-wise, this phase makes no capability claim at all through the `EngineCapability`
mechanism (`Available`/`Unavailable`/`Disabled`/`Error`) - there is no engine to report a
capability for. The historical `SermonIntelligenceEngine`'s own `EngineCapability::Available`
(semantic detection, genuinely already implemented under the earlier "Phase 2.3" label) is
unchanged and unaffected; nothing here overclaims or duplicates it.

## 17. Persistence decision

**Justified and implemented.** Migration `0008_sermon_foundation.sql` adds three additive
tables: `sermons`, `sermon_sections`, `sermon_segments` - mirroring `services`' own precedent
exactly (service restart recovery / sermon history / operator-workflow auditability /
Phase 2.6 continuity, the spec's own stated justifications). This is a deliberate departure
from Service Intelligence's Phase 2.4 decision to stay in-memory: `ServicePhase` tracking is a
*derived classification* re-computable from transcript replay, whereas a `Sermon`'s identity,
speaker, and title are *facts an operator stated*, exactly as durable as a `ServiceSession`
itself already is (foreign keys, indexes, and `CHECK` constraints follow this codebase's
established migration conventions - see `database/migrations/0008_sermon_foundation.sql`'s
own header comment).

Mirroring `services`' own precedent precisely: rows are durably persisted, but
`AppState.active_sermon`/`active_sermon_section` are **not** automatically restored into the
live session on app restart (the same is already true of `AppState.active_service` today, per
direct inspection of `lib.rs`'s setup code - no session-restoration call exists there for
`ServiceSession` either). Restart recovery means "the history is not lost, and is fully
queryable via `get_sermon`/`list_sermon_history`/`list_sermon_segments`/`list_sermon_sections`,"
not "a live session resumes unattended." This is proven directly by the canonical acceptance
scenario's own restart-recovery section (see "Testing").

No new `LogCategory` variant or `audit_events.category` `CHECK` constraint change was made -
Sermon Foundation timeline entries use `LogCategory::App`, the same conservative choice already
established by Service Intelligence, Sermon Intelligence, and Content Registry management (see
`0007_music_timeline_category.sql`'s own comment on why `'music'` earned a dedicated category
and Content Registry did not).

## 18. Events

Eight new `AppEvent` variants, each with a distinct wire name (locked in by the existing
distinctness test in `events.rs`): `SermonStarted`, `SermonPaused`, `SermonResumed`,
`SermonEnded`, `SermonSectionChanged`, `SermonSpeakerChanged`, `SermonMetadataChanged`,
`SermonSegmentLinked` - all distinct from the historical `SermonFindingDetected`/
`SermonFindingAccepted`/`SermonFindingRejected`/`SermonStructureUpdated`/`SermonThemeChanged`/
`SermonStateChanged` events, which belong to the semantic engine. `SermonCorrected` was
considered and *not* added: this phase has no separate "correct" action distinct from
re-calling `set_sermon_title`/`assign_sermon_speaker` (metadata correction and metadata
assignment are the same mechanism, following the spec's own framing: "`is_correction` only
changes... wording, not... mechanism").

## 19. Offline architecture

The only new dependencies this phase introduced are `chrono` and `uuid`, added to
`core/sermon/Cargo.toml` - both already used extensively elsewhere in this workspace
(`core/service`, `core/intelligence`, and others), both pure, local, offline-only libraries
with zero network I/O. `cargo tree -p cip-core-sermon` and `cargo tree -p cip-desktop` both
confirm zero network-related crates anywhere in either tree (`reqwest`, `hyper`, `native-tls`,
`rustls`, `curl`, and similar are all absent). No credentials, no API keys, no cloud
transmission of transcript or sermon data, no remote database - Sermon Foundation is
local-first and fully functional offline, like every other domain in this codebase.

## 20. Testing

- `core/sermon::foundation`: 28 unit tests across `sermon.rs` (lifecycle, every documented and
  every *rejected* state transition, metadata correction, identity distinctness),
  `section.rs` (open/close, idempotent close, taxonomy labels), `segment.rs` (linkage, no
  transcript-text leakage), and `speaker.rs` (identity, role labels).
- `core/intelligence::context`: 3 new tests proving the `with_sermon_context` builder is
  additive/backward-compatible and that `recent_sermon_segments` stays bounded at 10,000+
  input segments.
- `apps/desktop/src-tauri::sermon_foundation`: 8 finding-constructor tests (Observed/full-
  confidence, `OperatorAction`-only evidence, no fabricated titles/names, distinguishable
  summaries) plus the **canonical Phase 2.5 acceptance scenario** - a full, fictional
  service/sermon walkthrough (service start -> sermon start -> speaker assigned -> title
  assigned -> transcript segments -> section assigned -> segment retains linkage -> pause ->
  resume -> more transcript -> end) proving, in one test: Sermon != ServiceSession, Sermon !=
  TranscriptSegment, the transcript is never rewritten, sermon context is bounded, every
  produced finding is `Sermon`-domain/`Observed`/free of semantic language, every lifecycle
  action is present in the audit timeline, all relevant state survives a simulated restart
  (re-read purely from the database), and replaying an equivalent action sequence against an
  independent database produces an equivalent final status (determinism).
- `apps/desktop/src-tauri::persistence`: 12 new tests - restart-survival round-trips for
  `Sermon`/`SermonSection`/`SermonSegment`, foreign-key rejection of orphan rows, gapless
  sequence numbering, and service-scoped history ordering.
- `apps/desktop/src-tauri::commands`: 7 new guard-function tests (`ensure_no_active_sermon`,
  `ensure_valid_sermon_transition`, `parse_speaker_role_input`, `parse_section_kind_input`) plus
  a camelCase-serialization proof for `SermonFoundationSummary`.
- `database::migrations`: 5 new tests - table/index existence, foreign-key enforcement (both
  `sermons.service_id` and `sermon_segments.{sermon_id,transcript_segment_id}`), and `CHECK`
  constraint rejection of an unknown status.
- Frontend: 6 new domain-contract tests (`contracts.test.ts`), 9 new command-wrapper tests
  including the outside-Tauri-runtime rejection proof (`commands.test.ts`), and 3 new
  event-subscription tests (`liveEvents.test.ts`).

## 21. Performance

A throwaway release-mode benchmark (written, measured, then deleted before commit) measured the
hot path behind `link_transcript_segment_to_sermon` (`count_sermon_segments` +
`persist_sermon_segment`) and `list_sermon_segments` (the hot path behind
`build_music_context`'s sermon-context attachment) at 20/100/1000 already-linked segments:

| n | link (total) | link (per-segment) | list all |
|---|---|---|---|
| 20 | 546µs | 27µs | 42µs |
| 100 | 2.85ms | 29µs | 119µs |
| 1000 | 43ms | 43µs | 1.16ms |

Per-segment link cost stays roughly flat (27-43µs) and `list_sermon_segments` scales
sub-linearly with `n` (both backed by the `idx_sermon_segments_sermon_id` index) - no O(n²) or
worse growth was found. Every other operation this phase introduces (`Sermon`/`SermonSection`
lifecycle mutations, finding construction) is a plain in-memory struct mutation or string
format, O(1) by construction with nothing to benchmark meaningfully.

## 22. PROVEN

- A `Sermon` entity with a real, validated, directly-tested lifecycle, structurally distinct
  from `ServiceSession` and from `TranscriptSegment`.
- Explicit, auditable speaker and title assignment/correction - unknown metadata never guessed.
- A closed section taxonomy with an explicit origin distinction (operator-assigned vs.
  system-boundary vs. reserved-inferred), never two sections open simultaneously, full history
  preserved.
- Deterministic transcript-segment linkage that never duplicates or rewrites transcript text.
- Full `IntelligenceContext` integration, additive and backward-compatible with every existing
  engine's call sites.
- Every operator action recorded in the timeline, emitted as an event, and queued as an
  `Observed` `IntelligenceFinding` with zero semantic/theological language.
- No engine-to-engine calls anywhere in this phase (structural proof: no engine symbol is
  imported by `sermon_foundation.rs`).
- No automatic presentation - no code path here can reach `cip_core_presentation`.
- Full restart-recovery for `Sermon`/`SermonSection`/`SermonSegment` via real SQLite
  persistence, proven end-to-end by the canonical acceptance scenario.
- Deterministic, reproducible behavior for identical operator-action sequences.
- No O(n²)+ growth in the segment-linkage/listing hot path, measured directly.
- Fully offline (only `chrono`/`uuid`, both already used elsewhere, added this phase).

## 23. NOT AVAILABLE / NOT VERIFIED

- Semantic sermon understanding of any kind - themes, main points, illustrations, doctrine,
  applications remain exclusively the historical `core/sermon` semantic engine's job
  (Phase 2.6-equivalent under this roadmap); Sermon Foundation never attempts any of it.
- Automatic/inferred section assignment from transcript wording - `SectionOrigin::Inferred` is
  modeled and reserved but never produced; every section this phase assigns is either an
  explicit operator choice or the one deterministic `Introduction`-on-start system boundary.
- Multiple speakers per sermon (a panel discussion, a guest introduced partway through) - the
  domain model supports exactly one `Speaker` per `Sermon`; a second `assign_sermon_speaker`
  call replaces rather than adds. Deliberately deferred rather than half-built, since nothing in
  this phase's acceptance scenario or operator-action list requires it.
- Real speaker diarization or biometric voice recognition - not implemented, not planned in
  this phase; every speaker assignment is explicit and manual.
- A "schedule a sermon ahead of time" workflow - `SermonStatus::Planned`/`Cancelled` are
  modeled and unit-tested but not reachable from any Tauri command; `start_sermon` always
  begins delivering immediately.
- Automatic, real-time transcript-segment linkage from the live audio pipeline -
  `link_transcript_segment_to_sermon` is explicit/operator- (or test-harness-) invoked only;
  `pipeline.rs::handle_final_transcript` remains completely untouched by this phase.
- Section-scoped segment queries as a dedicated command (e.g. "everything said during the
  Illustration section") - the data (`SermonSegment.section_id`) is captured and persisted, but
  no command yet exposes a filtered view; deferred as a straightforward Phase 2.6+ addition.
- Any LLM-based or statistical reasoning of any kind - explicitly out of scope, consistent with
  every other deterministic-first phase in this codebase.

## 24. Phase 2.6 handoff

Phase 2.6's real Sermon Intelligence work can build directly on this foundation without
redesigning the domain model (invariant 15): `IntelligenceContext.active_sermon`/
`current_sermon_section`/`recent_sermon_segments` are already populated and bounded;
`Sermon`/`SermonSection`/`SermonSegment` already have stable identities and full persistence;
the historical semantic engine (`core/sermon`'s detection/state/structure/theme modules,
`SermonIntelligenceEngine`) already exists and is untouched, ready to be the concrete
`IntelligenceEngine` Phase 2.6 formally validates against this foundation's structural context -
for example, associating a detected main point or theme with the `SermonSection` it was said
in, or scoping theme-tracking to the currently active `Sermon` rather than the whole service.
None of that association logic is built here; the data it would need already is.
