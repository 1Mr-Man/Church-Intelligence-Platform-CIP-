# Cross-Domain Intelligence (Phase 2.4, extended in Phase 2.8)

Phase 2.4 added CIP's first cross-domain reasoning layer: a deterministic
rule engine that reads the findings the Bible, Music, and Sermon engines
have already produced and derives *correlations* between them - "this
sermon point references the same verse as this Bible finding," "this song
was recognized in the same breath as this Scripture reference." It is not
a new engine, not an LLM, not semantic search, and not a recommendation
system. Every correlation traces to an explicit rule and explicit evidence;
none is ever fabricated from mere coincidence.

Phase 2.4 predates Service Intelligence, Sermon Foundation, and Content
Intelligence - all landed later in the roadmap, so the original engine had
nothing from those domains to correlate against yet. Phase 2.8 (per the
authoritative Phase 2 roadmap) is not a second engine: it **extends** this
same `CrossDomainCorrelationEngine`, adding only what those newer domains
made possible - Service now participates in the weakest fallback rule, and
a Content Intelligence candidate can be correlated with the finding it
relates to. Every Phase 2.4 rule, confidence value, and behavior described
below is unchanged.

> **SAME SERVICE ≠ AUTOMATIC CORRELATION.** Nothing in this engine, in
> either phase, ever correlates two findings merely because they occurred
> in the same service. Every rule below requires either an explicit shared
> reference/identifier or a bounded transcript-proximity tier
> (`Immediate`/`Near`) - "same service, otherwise unrelated" is not, on its
> own, evidence for anything this engine produces.

## Critical design principle

> Engines produce findings. Correlation connects findings. Operators review
> intelligence. Presentation remains a separate, human-controlled layer.

Concretely: Bible → Music, Music → Sermon, and Sermon → Bible calls are
forbidden (Phase 2.0's engine independence, unchanged); a correlation is
never automatically turned into a presentation item, an automatic
approval, or an automatic projection. See `docs/intelligence-architecture.md`
for the architecture this phase extends.

## Architecture: a correlation layer, not an `IntelligenceEngine`

[`cip_core_intelligence::CrossDomainCorrelationEngine`](/core/intelligence/src/cross_domain.rs)
deliberately does **not** implement the [`IntelligenceEngine`] trait and is
**never** registered into `IntelligenceEngineRegistry`. `IntelligenceEngine::analyze`
returns `Vec<IntelligenceFinding>`; a correlation is a structurally
different value ([`IntelligenceCorrelation`], never folded into
`IntelligenceFinding` - see below), so it does not fit that shape. This
exactly mirrors Phase 2.2's `MusicIntelligenceEngine::analyze_acoustic`, an
inherent method outside the shared trait for the same reason.

```text
Bible/Music/Sermon engines
        |
        v
IntelligenceContext.recent_findings  (already built by the Tauri layer,
        |                             every domain, unchanged from Phase 2.0)
        v
CrossDomainCorrelationEngine::analyze(&context) -> Vec<IntelligenceCorrelation>
        |
        v
CorrelationQueue  (operator review: review / dismiss - never automatic)
        |
        v
Cross-Domain Intelligence panel (read-only except review/dismiss)
```

The engine reads `IntelligenceContext` - the same shared, bounded context
type every other engine reads - and never calls another engine directly.
The only channel between domains is what the Tauri orchestration layer has
already assembled into `context.recent_findings`.

## Correlation domain model

`core/intelligence::correlation` (extended, not rewritten, from Phase 2.0)
defines [`IntelligenceCorrelation`]:

| Field | Meaning |
| --- | --- |
| `id` | Correlation identity. |
| `service_id` | Which service this correlation belongs to. |
| `source_finding_ids` | Every finding this correlation connects. |
| `domains` | Every `IntelligenceDomain` represented among the source findings. |
| `kind` | See [`CorrelationKind`] below. |
| `assertion_level` | Always `Inferred` in Phase 2.4 (see below). |
| `status` | Reuses `FindingStatus` exactly - `Detected`/`Reviewed`/`Rejected` are the only states this phase drives. |
| `confidence` | A `ConfidenceResult`, the same shared type every finding uses. |
| `summary` | A short, specific description - never "these things seem related." |
| `evidence` | `Vec<EvidenceSource>`, reusing the existing evidence model. |
| `rule_id` / `rule_version` | Which rule produced this, and which version (provenance/rule-versioning - see below). |
| `created_at` | Timestamp. |

Correlations are deliberately their own type, not folded into
`IntelligenceFinding`: a correlation's meaning ("these findings are
related, here is why") is structurally different from a finding's meaning
("this was detected"). `FindingKind::Correlation` remains a reserved,
unused variant, exactly as it was in Phase 2.0-2.3.

### `CorrelationKind`

Extended additively at every phase - every Phase 2.0 variant
(`TemporalProximity`, `SharedContext`, `Other(String)`) and every Phase 2.4
variant is unchanged; Phase 2.8 adds exactly two new variants, deliberately
**not** the full taxonomy the Phase 2.8 spec sketched as *possible*
(`ServiceMusic`, `ServiceScripture`, `MultiDomainConvergence`,
`ThematicConvergence`, ...) - only the smallest complete set the actually
implemented rules require:

| Variant | Meaning | Phase |
| --- | --- | --- |
| `ScriptureSermon` | A Sermon finding names the same Scripture reference (or chapter) as a Bible finding. | 2.4 |
| `ScriptureMusic` | A Bible finding and a Music finding share a transcript segment. | 2.4 |
| `SermonMusic` | A Sermon finding (typically transition/conclusion/prayer) and a Music finding occur close together. | 2.4 |
| `ThemeScripture` | A sermon theme candidate and a Bible finding occur close together. | 2.4 |
| `ThemeMusic` | A sermon theme candidate and a Music finding occur close together (proximity only). | 2.4 |
| `ServiceTransition` | A sermon conclusion/transition signal coincides with a service-lifecycle event. | 2.4 |
| `SermonContent` | A Content Intelligence candidate relates to a Bible or Music finding, via the candidate's own source Sermon finding's transcript proximity. | 2.8 |
| `MultiDomainConvergence` | Three or more distinct domains' findings share the same literal transcript segment. | 2.8 |
| `TemporalProximity` | (Phase 2.0, reused) Two findings occurred near each other, with no stronger evidence - this is what the spec calls "TemporalAssociation"; it was not given a duplicate variant. Phase 2.8 additionally includes `Service` in the domain set this rule considers (see below). | 2.0 |
| `SharedContext` | (Phase 2.0, reserved; no rule in either phase produces this.) | 2.0 |

**Why no `ServiceMusic`/`ServiceScripture`:** no evidence stronger than
temporal proximity connects a Service finding to a Music or Bible finding
anywhere in this engine - a Service finding only ever says "the service
entered phase X," never anything about a specific song or verse. Adding a
same-strength dedicated `CorrelationKind` for these pairs would only widen
the taxonomy without adding informational value over the existing
`TemporalProximity` kind, so Phase 2.8 instead fixed a real gap: Service
was previously excluded even from that weakest fallback (see
`rule_temporal_association` below). Sermon↔Service keeps its own
`ServiceTransition` rule because a sermon conclusion/transition signal is
meaningfully, specifically tied to a service-lifecycle event in a way no
other domain pair is.

## Evidence and provenance

Every correlation's `evidence` is built from `EvidenceSource::AnotherFinding`
(pointing at the finding ids it connects), plus `EvidenceSource::Temporal`
or `EvidenceSource::ServiceEvent` where the rule's reasoning is temporal or
lifecycle-anchored - no new evidence variant was added. `rule_id` (e.g.
`"scripture_sermon_v1"`) and `rule_version` (`"1.0"`) are plain string
constants per rule, not a plugin system - every correlation is traceable
to exactly which deterministic rule produced it, and a future change to a
rule's behavior would bump its version rather than silently reinterpreting
old output.

## The rule catalogue

Rules never re-detect anything - they only read summaries the Bible/Sermon
adapters already produce. `parse_reference_token` is a pure syntactic
parser over strings like `"ROM 8:28"` (from `bible_adapter`) or
`"Supporting Scripture: ROM 8:28"` (from `sermon_adapter::finding_for_scripture_cross_link`),
never a new Scripture-detection path.

| Rule (`rule_id`) | Kind | Trigger | Confidence | Phase |
| --- | --- | --- | --- | --- |
| `scripture_sermon_v1` | `ScriptureSermon` | Sermon's `"Supporting Scripture: ..."` reference matches a Bible finding's book+chapter | 0.75 (chapter only), **0.95** (exact book+chapter+verse) | 2.4 |
| `theme_scripture_v1` | `ThemeScripture` | A sermon theme finding at `Immediate`/`Near` proximity to a Bible finding | 0.7 (Immediate), 0.5 (Near) | 2.4 |
| `sermon_music_v1` | `SermonMusic` | A Sermon finding at `Immediate` proximity to a Music finding, or `Near` proximity **and** transition/conclusion/prayer-shaped | 0.85 (Immediate), 0.7 (Near + transition-shaped) | 2.4 |
| `theme_music_v1` | `ThemeMusic` | A sermon theme finding at `Immediate`/`Near` proximity to a Music finding - proximity only, no lyric/title matching | 0.55 (Immediate), 0.4 (Near) | 2.4 |
| `scripture_music_v1` | `ScriptureMusic` | A Bible finding and a Music finding **share a transcript segment** (`Immediate` only - see the conservatism note below) | 0.8 | 2.4 |
| `service_transition_v1` | `ServiceTransition` | A conclusion/transition-shaped Sermon finding within 120s of a `SERVICE_ENDED`/`SERVICE_PAUSED`/`SERMON_STATE_CHANGED` service event | 0.55 | 2.4 |
| `sermon_content_v1` | `SermonContent` | A Content Intelligence candidate's own source Sermon finding is at `Immediate`/`Near` proximity to a Bible or Music finding | 0.65 (Immediate), 0.45 (Near) | 2.8 |
| `multi_domain_convergence_v1` | `MultiDomainConvergence` | Three or more of {Bible, Music, Sermon, Service} findings **share a literal transcript segment** | 0.85 (3 domains), 0.9 (4 domains) | 2.8 |
| `temporal_association_v1` | `TemporalProximity` | Fallback: any cross-domain pair at `Immediate`/`Near` proximity not already claimed by a stronger rule above; the domain set is {Bible, Music, Sermon, Service} since Phase 2.8 (previously {Bible, Music, Sermon} only) | 0.35 (Immediate), 0.25 (Near) | 2.0/2.8 |

### `sermon_content_v1`: never a tautology

A `ContentCandidate` always carries `source_finding_ids` pointing at the
exact Sermon finding it was derived from (Phase 2.7). Restating that link
as a correlation would add no information - it already exists, verbatim,
on the candidate itself. `rule_sermon_content` never does this: it only
correlates a candidate with a *different* domain's finding (Bible or
Music) that is temporally near the candidate's source Sermon finding, and
explicitly excludes the candidate's own parent from the correlation's
`sourceFindingIds`. It also never mutates the candidate - not its
`contentPotential`, `titleOrLabel`, or `workingConcept` - and never turns
it into final content; Content Intelligence's own accept/reject workflow
(`docs/content-intelligence.md`) is entirely unaffected.

### `multi_domain_convergence_v1`: breadth of evidence, not meaning

Requires the literal same transcript segment (`Immediate` tier only -
never `Near`), across at least three of the four true `IntelligenceFinding`
domains this engine reads (`Bible`/`Music`/`Sermon`/`Service`). Content
candidates are deliberately excluded from this rule - they carry no
`transcriptSegmentIds` of their own to cluster by (`sermon_content_v1` is
the one rule that connects them). The confidence step at four domains
(0.9) versus three (0.85) reflects strictly more corroborating evidence,
never a claim about *why* the domains converged - the summary text names
only what was said together, never a causal or theological interpretation
("shares the same transcript segment," never "God intentionally aligned
this song with this verse").

### The `scripture_music_v1` conservatism requirement

The spec's own example - "Amazing Grace" (a Music finding) plus a Romans 8
Bible finding elsewhere in the same service - **must not** automatically
become a `ScriptureMusic` correlation. `rule_scripture_music` enforces
this by only firing at `TemporalTier::Immediate` (the same transcript
segment literally names both); `Near`/`Recent` proximity between a Bible
and a Music finding is never promoted to `ScriptureMusic` - it can, at
most, surface through the low-confidence `temporal_association_v1`
fallback, correctly labeled as proximity-only evidence.

## Temporal association windows

Built once per `analyze()` call from `IntelligenceContext.recent_transcript_segments`'s
existing `.sequence` field - no new clock, no new segment concept:

| Tier | Definition |
| --- | --- |
| `Immediate` | The two findings share at least one transcript segment id. |
| `Near` | Within **3** sequence numbers of each other (`NEAR_WINDOW_SEGMENTS`). |
| `Recent` | Within **10** sequence numbers (`RECENT_WINDOW_SEGMENTS`) - defined, computed honestly by `temporal_relationship`, but deliberately **never** used to gate any rule above. Proximity alone, beyond `Near`, is never strong enough evidence for anything Phase 2.4 produces. |

`rule_service_transition` is the one rule that does not use transcript
sequence at all - service-lifecycle events carry no segment id, so it
compares `finding.created_at` against `event.occurred_at` (already-existing
timestamp fields) within a **120-second** window (`SERVICE_TRANSITION_WINDOW_SECONDS`).

## Confidence hierarchy

Reuses `ConfidenceResult`/`ConfidenceSource::Heuristic` exactly - no new
confidence system. From highest to lowest:

1. **0.95** - exact scripture reference (book + chapter + verse) shared between Sermon and Bible.
2. **0.9** - four distinct domains share the same literal transcript segment (`MultiDomainConvergence`).
3. **0.85** - Sermon and Music findings share a transcript segment; three distinct domains share the same literal transcript segment (`MultiDomainConvergence`).
4. **0.8** - Bible and Music findings share a transcript segment.
5. **0.75** - Sermon and Bible findings share a scripture chapter (no verse match).
6. **0.7** - Sermon and Music at `Near` proximity, transition-shaped; theme and Bible at `Immediate`.
7. **0.65** - a content candidate's source sermon finding at `Immediate` proximity to a Bible or Music finding (`SermonContent`).
8. **0.55** - Theme and Bible at `Near`; a sermon conclusion coinciding with a service transition; theme and Music at `Immediate`.
9. **0.45** - a content candidate's source sermon finding at `Near` proximity to a Bible or Music finding (`SermonContent`).
10. **0.4** - theme and Music at `Near`.
11. **0.35 / 0.25** - the temporal-only fallback (`Immediate`/`Near`), always the lowest tier: proximity alone is weak evidence, whichever two domains it connects (including Service, since Phase 2.8).

## Assertion level

Every correlation is `AssertionLevel::Inferred` - a correlation is always
CIP's own derived judgment that two findings relate, never a verbatim
observation (`Observed`), a specific proposal for review in the sense
`Suggested` findings are, and **never** `Generated`. Neither phase
introduces generated content of any kind.

## No engine-to-engine calls

`CrossDomainCorrelationEngine` holds no reference to `BibleIntelligenceEngine`,
`MusicIntelligenceEngine`, `SermonIntelligenceEngine`, `ServiceIntelligenceEngine`,
or `ContentIntelligenceEngine`, and never calls `IntelligenceEngine::analyze`
on anything. It only reads the `IntelligenceContext` the Tauri orchestration
layer already built (which, since Phase 2.8, additively carries
`recent_content_candidates` the same way it has carried
`active_sermon`/`recent_sermon_segments` since Phase 2.5). This is the same
Phase 2.0 rule ("engines never call each other") applied to the one layer
that sits above all of them.

## Deduplication

Two correlations are equivalent (`IntelligenceCorrelation::is_equivalent_to`)
when they share the same `service_id`, the same `kind`, and the same *set*
of `source_finding_ids` (order-independent) - regardless of id, timestamp,
confidence, or evidence. `dedup()` keeps only the first occurrence of each
equivalence class, using a hash-keyed pass (`service_id` + `kind`'s `Debug`
form + sorted source ids) rather than an O(n²) nested scan - see
Performance below for why this mattered in practice.
`CorrelationQueue::add` applies the same rule again at queue-insertion
time, so a second `analyze_cross_domain` call never duplicates an
already-pending correlation.

## Ordering

`sort_deterministically` orders correlations by confidence descending,
then `kind.label()`, then sorted source finding ids, then `id` as a final
tiebreak - never dependent on `HashMap` iteration order. Two `analyze()`
calls against equivalent input always produce the same ordering (modulo
the randomly-generated `id`/`created_at` fields, the same convention every
other determinism test in this codebase already uses).

## Cross-domain finding creation

Correlations are **not** auto-converted into findings. They stay their own
concept ([`IntelligenceCorrelation`]), stored in their own queue
([`CorrelationQueue`]), never re-queued as `IntelligenceFinding`s. This
was a deliberate decision (spec section 18's stated default): a
correlation and a finding answer different questions, and merging them
would force one of the two concepts to carry fields it doesn't need.

## The correlation queue

[`CorrelationQueue`] mirrors [`FindingQueue`] structurally (`add`,
`pending`, `review`, `dismiss`, `get`, `all`, `len`, `is_empty`) but holds
`IntelligenceCorrelation` rather than `IntelligenceFinding` - a distinct
type is needed only because the two queues are hard-typed to different
element types, not because the behavior differs. In-memory only, exactly
like `FindingQueue`: **no new database table** (see Database below).
Lifecycle reuses `FindingStatus` directly - `review()` moves `Detected` →
`Reviewed`; `dismiss()` moves any state to `Rejected`; `Accepted`/`Expired`
are part of the reused enum but nothing in either phase drives them. No new
queue bound was added in Phase 2.8: like `FindingQueue`
(`docs/intelligence-architecture.md`) and `ContentCandidateQueue`
(`docs/content-intelligence.md`), `CorrelationQueue` has no hard size cap -
this is the established precedent across every queue in this crate, not a
regression Phase 2.8 introduced or needed to fix.

## Making a Bible finding reachable: the `analyze_bible_transcript` bridge

Investigating the Tauri layer for Phase 2.4 surfaced one real gap: unlike
Music (Phase 2.1) and Sermon (Phase 2.3), nothing ever produced a
Bible-domain `IntelligenceFinding` into `AppState.intelligence_findings` -
the live Scripture-detection workflow uses the older, separate
`ScriptureDetection`/`Suggestion` model (`core/service::process_transcript_segment`),
never `IntelligenceFinding`. Without a Bible finding in
`context.recent_findings`, no rule above involving Bible could ever fire.

The fix is new, additive wiring only - `commands::analyze_bible_transcript`,
mirroring `analyze_music_transcript`/`analyze_sermon_transcript` exactly:
it persists a transcript segment, builds a real `IntelligenceContext`, and
calls the already-registered (since Phase 2.0) `BibleIntelligenceEngine`
via `intelligence_registry.resolve(IntelligenceDomain::Bible)` - an engine
that has sat registered but never invoked from a live command since Phase
2.0. **No line of `core/bible` or `core/service` changed.** The existing
live Scripture-detection pipeline (`pipeline.rs::handle_final_transcript`)
is completely unaffected; this is a second, parallel, manual/test-mode
entry point, exactly like Music's and Sermon's.

## Making Content candidates reachable: `IntelligenceContext.recent_content_candidates`

Investigating the core crate for Phase 2.8 surfaced the analogous gap on
the Content Intelligence side: `IntelligenceContext` (`core/intelligence::context`)
had no field, and no additive `with_*` builder, for `ContentCandidate` at
all - unlike `active_sermon`/`recent_sermon_segments` (Phase 2.5). Without
it, no rule could ever see a content candidate, regardless of how the
Tauri layer built the context.

The fix mirrors `with_sermon_context` exactly: `recent_content_candidates:
Vec<ContentCandidate>` (bounded by a new `ContextBounds.max_recent_content_candidates`,
default 20, same order of magnitude as every other bound), and a new
`IntelligenceContext::with_content_candidates(...)` builder - additive,
never a required argument to `build()`, so every existing caller (Bible/
Music/Sermon/Service/CrossDomain adapters and their tests) remains valid,
unmodified source. `commands::build_music_context` (the same helper every
`analyze_*` Tauri command already shares) now additionally reads
`state.content_candidate_queue` and attaches it, exactly like it already
attaches `state.active_sermon`. No new Tauri command, event, or `AppState`
field was needed - `content_candidate_queue` already existed (Phase 2.7);
Cross-Domain Intelligence simply gained a second reader of it, alongside
`analyze_content_intelligence` itself.

## Failure isolation

`CrossDomainCorrelationEngine` sits outside `IntelligenceEngineRegistry::analyze_all`'s
existing per-engine `catch_unwind` isolation (since it isn't a registered
engine), so it needed its own: `run_rules` wraps every rule call in its
own `catch_unwind(AssertUnwindSafe(...))` - one rule panicking contributes
zero correlations for that call, and every other rule (and the temporal
fallback) still runs. Proven by `scenario_g_a_panicking_rule_never_stops_the_others`,
which injects a deliberately panicking function alongside real rules. A
correlation-engine failure of any kind can never stop the Bible/Music/
Sermon/audio/speech/service lifecycle - `analyze_cross_domain` is a wholly
separate, explicit Tauri command, never on the critical path of live
transcript processing.

## Operator workflow (Tauri commands)

All five commands (Phase 2.4) are additive; none is wired into
`pipeline.rs::handle_final_transcript` or any other automatic path. Phase
2.8 added **no new command** here - see the note on `build_music_context`
above for why the existing `analyze_cross_domain()` already suffices.

| Command | Effect |
| --- | --- |
| `analyze_bible_transcript(text)` | The Bible-finding bridge described above. |
| `analyze_cross_domain()` | Explicit operator/diagnostic action: builds the real cross-domain context (every domain's queued findings and, since Phase 2.8, queued content candidates, via the same context-building helper Music's own commands use) and queues any new correlations. |
| `list_cross_domain_correlations()` | Correlations still awaiting a decision (`Detected`/`Reviewed`), for the active service. |
| `review_cross_domain_correlation(id)` | Informational-only review - never required before dismissal. |
| `dismiss_cross_domain_correlation(id)` | Explicit operator dismissal - changes only this correlation's own status; has no way to alter a source finding, a content candidate, the transcript, or the active Scripture context. |

Because `analyze_cross_domain()` reads whatever is already queued in
`content_candidate_queue`, a `SermonContent` correlation only appears after
an operator has separately run `analyze_content_intelligence()` first (the
existing Phase 2.7 workflow, entirely unchanged) - this is not a new
ordering requirement invented for Phase 2.8, it is the same "engines
produce, correlation only reads what's already there" rule every other
domain pair in this engine already follows.

The Live Church Brain's "Cross-Domain Intelligence" panel is read-only
except for these two operator actions: correlations only ever appear after
an explicit "Run cross-domain analysis" click (or arrive via the
`CROSS_DOMAIN_CORRELATION_DETECTED` event from a prior such call), never
automatically as a side effect of a transcript segment arriving. Phase 2.8
added no new panel and no new dashboard (that is Phase 2.9's Unified
Operator Workspace, explicitly out of scope here) - the existing panel
already renders `kind`/`domains`/`confidence`/`ruleId`/`status` generically
from the correlation object, so the two new `CorrelationKind` values
display correctly with zero frontend component changes.

## Events

Three new `AppEvent` variants, each carrying the updated `IntelligenceCorrelation`:
`CROSS_DOMAIN_CORRELATION_DETECTED`, `CROSS_DOMAIN_CORRELATION_REVIEWED`,
`CROSS_DOMAIN_CORRELATION_DISMISSED` - mirroring the `*_FINDING_DETECTED`/
`*_ACCEPTED`/`*_REJECTED` shape Music and Sermon already established, so
the frontend's event-handling pattern needs no new shape to learn.

## Database: no new tables

`CorrelationQueue` is in-memory only, exactly like `FindingQueue` - a
correlation is derived from findings (and, since Phase 2.8, content
candidates) that themselves already carry provenance (their own
`IntelligenceFinding.id`/`ContentCandidate.id`s), so nothing here needs to
survive a restart. This persistence decision is unchanged from Phase 2.4 -
Phase 2.8 introduced no database migration, and none was warranted.
`dismiss_cross_domain_correlation` records a timeline entry (reusing
`audit_events`, the same mechanism every other phase's operator actions
already use) - no new migration.

## Frontend

`domain/intelligence.ts` mirrors the `IntelligenceCorrelation`/
`CorrelationKind` shape field-for-field with the Rust structs (camelCase,
matching serde's `rename_all`) - Phase 2.8 only added the two new
`CorrelationKind` union members (`sermon_content`/`multi_domain_convergence`),
both unit variants requiring no new fields. `lib/commands.ts` and
`lib/liveEvents.ts` still expose exactly the five commands and three
events from Phase 2.4, guarded by the same `isTauriRuntime()`/
`TauriUnavailableError` discipline every other command/event wrapper uses.
The Cross-Domain Intelligence panel in `LiveChurchBrain.tsx`, also
unchanged, remains the only UI surface - see the operator-workflow note
above for why it needed no changes to display the two new kinds.

## Performance

Phase 2.4's own measurements (unchanged, reproduced here for context):
20 findings ~0.5ms, 100 findings ~4.4ms, 1,000 findings ~0.43s (after
`dedup()`'s O(n²)-in-produced-correlations bug was fixed - see
Deduplication above).

Phase 2.8 measured its two new rules the same way (`std::time::Instant`,
release build, this machine, one run against an adversarial dataset
combining Bible/Music/Sermon/Service findings and content candidates in
equal proportion so every new rule fires - a throwaway test, deleted
before commit, matching the established methodology):

| Findings (+ candidates) | Correlations produced | `analyze()` time |
| --- | --- | --- |
| 20 | 72 | ~0.29ms |
| 100 | 392 | ~1.6ms |
| 1,000 | 3,992 | ~86ms |

No algorithmic fix was needed this time: `rule_multi_domain_convergence`
groups findings by segment id in a single pass (a `HashMap<Uuid, Vec<...>>`
built once, not rebuilt per candidate), and `rule_sermon_content` is a
pairwise scan bounded the same way every other pairwise rule
(`rule_scripture_music`, `rule_theme_music`, ...) already is. As with
Phase 2.4, this scenario cannot occur in production either: `IntelligenceContext`
is always built with `ContextBounds::default()` (20 recent findings, 20
recent content candidates) by the Tauri layer, so the real cost at the
actual production bound is well under a millisecond.

## Offline guarantee

Neither phase adds a dependency to any `Cargo.toml` or any frontend
`package.json` - `cargo tree -p cip-core-intelligence` and
`cargo tree -p cip-desktop` are unchanged from Phase 2.7, and `Cargo.lock`
has zero diff from this work. `core/intelligence`'s normal dependency tree
still carries no network-related crate; see
`docs/intelligence-architecture.md#offline-operation` for the
architecture-wide guarantee this phase inherits unchanged.

## PROVEN

- Every rule in the catalogue above fires on real, hand-constructed
  `IntelligenceFinding` fixtures and produces the documented `CorrelationKind`
  and confidence tier (unit tests per rule, plus the scripted scenarios A-J
  and the canonical full-service scenario).
- `scripture_music_v1`'s conservatism: a Bible and Music finding at `Near`
  (not `Immediate`) proximity never produces `ScriptureMusic` - proven by
  a test asserting the correlation, if any, is `TemporalProximity` only.
- Deduplication: a repeated identical `analyze_and_queue` call produces no
  new queued correlations.
- Ordering: `analyze()` output is sorted deterministically, independent of
  input order.
- Failure isolation: a deliberately panicking rule never stops the others
  from running.
- Rejected/Expired findings are excluded from `AnalysisContext`'s
  candidate pool - a dismissed finding is never silently treated as
  accepted evidence for a new correlation.
- The `analyze_bible_transcript` bridge queues a real Bible finding from
  the real, registered `BibleIntelligenceEngine`, with zero changes to
  `core/bible`/`core/service`.
- `CrossDomainCorrelationEngine` is not registered into
  `IntelligenceEngineRegistry` (a direct assertion in the orchestration
  tests) and has no field or method that could call another engine.
- Operator dismissal changes only a correlation's own `status` - no code
  path here can create a `PresentationItem`.
- 1,000 bounded findings never panic and stay within a sane correlation
  count relative to the (already-truncated) candidate pool.
- (Phase 2.8) `rule_sermon_content` fires at both `Immediate` (0.65) and
  `Near` (0.45) proximity, never correlates a candidate with its own
  parent Sermon finding, and produces nothing when the candidate's parent
  finding has aged out of the bounded context or when no candidates were
  ever attached to the context at all (three separate tests).
- (Phase 2.8) `rule_multi_domain_convergence` fires at exactly the
  documented confidence for 3 and 4 converging domains, never fires for
  only 2 domains, and never fires when the domains are merely `Near`
  rather than sharing the literal segment.
- (Phase 2.8) Service now participates in `temporal_association_v1` -
  proven by a test that would have failed before this phase (Service was
  previously excluded from that rule's domain set entirely).
- (Phase 2.8) A canonical full-service walkthrough (`phase_2_8_canonical_full_service_walkthrough`)
  exercises Service→Worship, a recognized song, a worship-transition
  sermon signal, a sermon main point with an explicit Scripture cross-link
  to a real BSB-format reference (`ROM 8:28`), and an already-queued
  content candidate, together in one `analyze()` call - and asserts
  `ScriptureSermon`, `SermonMusic`, `MultiDomainConvergence`, and
  `SermonContent` all appear with correct evidence, and that every
  correlation stays `Detected`/`Inferred`.
- (Phase 2.8) Determinism: `phase_2_8_analysis_is_deterministic_across_repeated_calls`
  runs the same context through `analyze()` 10 times and asserts identical
  kind/source-id/confidence sequences every time (ids/timestamps excluded,
  the established convention).
- (Phase 2.8) `build_music_context` (Tauri layer) compiles and is exercised
  by the full desktop test suite (186 tests) and the real-runtime checks
  below; its content-candidate attachment mirrors the already-proven
  sermon-context attachment exactly.

## NOT AVAILABLE / NOT VERIFIED

- No semantic or theological reasoning of any kind - "Amazing Grace" and
  Romans 8 are never correlated merely because both are about grace; only
  explicit transcript-position evidence (shared/near segments) or an
  explicit shared Scripture reference ever produces a correlation. This
  extends to Phase 2.8's `MultiDomainConvergence`: it reports that domains
  converged, never why.
- No automatic presentation, preparation, or projection triggered by any
  correlation, at any confidence level - including `SermonContent`, which
  never mutates or auto-accepts the `ContentCandidate` it references.
- No cloud service, LLM, embeddings, vector database, or graph database of
  any kind.
- No new persistence: a correlation queue does not survive an application
  restart (matching `FindingQueue`'s own Phase 2.0 decision, unchanged by
  Phase 2.8).
- `TemporalTier::Recent` (±10 segments) is computed but never used to gate
  any rule in either phase - proximity beyond `Near` is not verified as
  meaningful evidence for anything this engine produces.
- No lyric-content or song-title matching against sermon/theme text of any
  kind (`rule_theme_music`'s explicit scope limit, unchanged).
- The correlation queue is not exposed as a `list_music_findings`-style
  per-domain filter; `list_cross_domain_correlations` returns every
  pending correlation for the active service regardless of which domains
  it connects.
- No dedicated `ServiceMusic`/`ServiceScripture` `CorrelationKind` -
  deliberately not added; see the taxonomy section above for why.
- No Content-Candidate participation in `MultiDomainConvergence` - a
  candidate carries no `transcriptSegmentIds` of its own, so it cannot be
  clustered by the same-segment rule; only `SermonContent` connects
  candidates to other domains.
- No Unified Operator Workspace, dashboard, or any Phase 2.9 surface -
  Phase 2.8 prepared no new contract beyond the two `CorrelationKind`
  values themselves (which the existing generic panel already renders);
  what, if anything, Phase 2.9 needs beyond that is Phase 2.9's own
  decision to make.
- No end-to-end Tauri-command-level test for `build_music_context`'s new
  content-candidate attachment specifically (this codebase's established
  testing convention stops short of standing up `tauri::test::mock_builder()`
  for `#[tauri::command]` functions - see `commands.rs`'s own test-module
  docs for why; the wiring is instead proven by compilation, the full
  desktop test suite, and the real-runtime checks below).

[`IntelligenceEngine`]: /core/intelligence/src/engine.rs
[`IntelligenceCorrelation`]: /core/intelligence/src/correlation.rs
[`CorrelationKind`]: /core/intelligence/src/correlation.rs
[`CorrelationQueue`]: /core/intelligence/src/cross_domain.rs
[`FindingQueue`]: /core/intelligence/src/queue.rs
