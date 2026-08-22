# Cross-Domain Intelligence (Phase 2.4)

Phase 2.4 adds CIP's first cross-domain reasoning layer: a deterministic
rule engine that reads the findings the Bible, Music, and Sermon engines
have already produced and derives *correlations* between them - "this
sermon point references the same verse as this Bible finding," "this song
was recognized in the same breath as this Scripture reference." It is not
a new engine, not an LLM, not semantic search, and not a recommendation
system. Every correlation traces to an explicit rule and explicit evidence;
none is ever fabricated from mere coincidence.

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

Extended additively - every Phase 2.0 variant (`TemporalProximity`,
`SharedContext`, `Other(String)`) is unchanged; Phase 2.4 only adds new
domain-pair variants:

| Variant | Meaning |
| --- | --- |
| `ScriptureSermon` | A Sermon finding names the same Scripture reference (or chapter) as a Bible finding. |
| `ScriptureMusic` | A Bible finding and a Music finding share a transcript segment. |
| `SermonMusic` | A Sermon finding (typically transition/conclusion/prayer) and a Music finding occur close together. |
| `ThemeScripture` | A sermon theme candidate and a Bible finding occur close together. |
| `ThemeMusic` | A sermon theme candidate and a Music finding occur close together (proximity only). |
| `ServiceTransition` | A sermon conclusion/transition signal coincides with a service-lifecycle event. |
| `TemporalProximity` | (Phase 2.0, reused) Two findings occurred near each other, with no stronger evidence - this is what the spec calls "TemporalAssociation"; it was not given a duplicate variant. |
| `SharedContext` | (Phase 2.0, reserved; no Phase 2.4 rule produces this.) |

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

| Rule (`rule_id`) | Kind | Trigger | Confidence |
| --- | --- | --- | --- |
| `scripture_sermon_v1` | `ScriptureSermon` | Sermon's `"Supporting Scripture: ..."` reference matches a Bible finding's book+chapter | 0.75 (chapter only), **0.95** (exact book+chapter+verse) |
| `theme_scripture_v1` | `ThemeScripture` | A sermon theme finding at `Immediate`/`Near` proximity to a Bible finding | 0.7 (Immediate), 0.5 (Near) |
| `sermon_music_v1` | `SermonMusic` | A Sermon finding at `Immediate` proximity to a Music finding, or `Near` proximity **and** transition/conclusion/prayer-shaped | 0.85 (Immediate), 0.7 (Near + transition-shaped) |
| `theme_music_v1` | `ThemeMusic` | A sermon theme finding at `Immediate`/`Near` proximity to a Music finding - proximity only, no lyric/title matching | 0.55 (Immediate), 0.4 (Near) |
| `scripture_music_v1` | `ScriptureMusic` | A Bible finding and a Music finding **share a transcript segment** (`Immediate` only - see the conservatism note below) | 0.8 |
| `service_transition_v1` | `ServiceTransition` | A conclusion/transition-shaped Sermon finding within 120s of a `SERVICE_ENDED`/`SERVICE_PAUSED`/`SERMON_STATE_CHANGED` service event | 0.55 |
| `temporal_association_v1` | `TemporalProximity` | Fallback: any cross-domain pair at `Immediate`/`Near` proximity not already claimed by a stronger rule above | 0.35 (Immediate), 0.25 (Near) |

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
2. **0.85** - Sermon and Music findings share a transcript segment.
3. **0.8** - Bible and Music findings share a transcript segment.
4. **0.75** - Sermon and Bible findings share a scripture chapter (no verse match).
5. **0.7** - Sermon and Music at `Near` proximity, transition-shaped; theme and Bible at `Immediate`.
6. **0.55** - Theme and Bible at `Near`; a sermon conclusion coinciding with a service transition; theme and Music at `Immediate`.
7. **0.4** - theme and Music at `Near`.
8. **0.35 / 0.25** - the temporal-only fallback (`Immediate`/`Near`), always the lowest tier: proximity alone is weak evidence.

## Assertion level

Every correlation is `AssertionLevel::Inferred` - a correlation is always
CIP's own derived judgment that two findings relate, never a verbatim
observation (`Observed`), a specific proposal for review in the sense
`Suggested` findings are, and **never** `Generated`. Phase 2.4 introduces
no generated content of any kind.

## No engine-to-engine calls

`CrossDomainCorrelationEngine` holds no reference to `BibleIntelligenceEngine`,
`MusicIntelligenceEngine`, or `SermonIntelligenceEngine`, and never calls
`IntelligenceEngine::analyze` on anything. It only reads the
`IntelligenceContext` the Tauri orchestration layer already built. This is
the same Phase 2.0 rule ("engines never call each other") applied to the
one new layer that sits above all of them.

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
are part of the reused enum but nothing in Phase 2.4 drives them.

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

All five new commands are additive; none is wired into
`pipeline.rs::handle_final_transcript` or any other automatic path.

| Command | Effect |
| --- | --- |
| `analyze_bible_transcript(text)` | The Bible-finding bridge described above. |
| `analyze_cross_domain()` | Explicit operator/diagnostic action: builds the real cross-domain context (every domain's queued findings, via the same context-building helper Music's own commands use) and queues any new correlations. |
| `list_cross_domain_correlations()` | Correlations still awaiting a decision (`Detected`/`Reviewed`), for the active service. |
| `review_cross_domain_correlation(id)` | Informational-only review - never required before dismissal. |
| `dismiss_cross_domain_correlation(id)` | Explicit operator dismissal - changes only this correlation's own status; has no way to alter a source finding, the transcript, or the active Scripture context. |

The Live Church Brain's "Cross-Domain Intelligence" panel is read-only
except for these two operator actions: correlations only ever appear after
an explicit "Run cross-domain analysis" click (or arrive via the
`CROSS_DOMAIN_CORRELATION_DETECTED` event from a prior such call), never
automatically as a side effect of a transcript segment arriving.

## Events

Three new `AppEvent` variants, each carrying the updated `IntelligenceCorrelation`:
`CROSS_DOMAIN_CORRELATION_DETECTED`, `CROSS_DOMAIN_CORRELATION_REVIEWED`,
`CROSS_DOMAIN_CORRELATION_DISMISSED` - mirroring the `*_FINDING_DETECTED`/
`*_ACCEPTED`/`*_REJECTED` shape Music and Sermon already established, so
the frontend's event-handling pattern needs no new shape to learn.

## Database: no new tables

`CorrelationQueue` is in-memory only, exactly like `FindingQueue` - a
correlation is derived from findings that themselves already carry
provenance (their own `IntelligenceFinding.id`s), so nothing here needs to
survive a restart. `dismiss_cross_domain_correlation` records a timeline
entry (reusing `audit_events`, the same mechanism every other phase's
operator actions already use) - no new migration.

## Frontend

`domain/intelligence.ts` mirrors the extended `IntelligenceCorrelation`/
`CorrelationKind` shape field-for-field with the Rust structs (camelCase,
matching serde's `rename_all`). `lib/commands.ts` and `lib/liveEvents.ts`
add typed wrappers for the five commands and three events above, guarded
by the same `isTauriRuntime()`/`TauriUnavailableError` discipline every
other command/event wrapper uses. The Cross-Domain Intelligence panel in
`LiveChurchBrain.tsx` is the only new UI surface.

## Performance

Measured directly (`std::time::Instant`, release build, this machine, one
run against a deliberately adversarial dataset designed to maximize the
number of matching pairs - a throwaway test file, deleted before commit,
matching the Phase 1.5/2.0-2.3 measurement methodology):

| Findings | Correlations produced | `analyze()` time |
| --- | --- | --- |
| 20 | 136 | ~0.5ms |
| 100 | 1,216 | ~4.4ms |
| 1,000 | 68,492 | ~0.43s |

The initial 1,000-finding measurement was **119 seconds** - `dedup()`'s
original implementation scanned every already-kept correlation for each
new candidate (O(n²) in the number of *produced* correlations, not input
findings), and the adversarial dataset produces tens of thousands of
correlations. It was rewritten to a hash-keyed single pass (see
Deduplication above), which is the number reported in the table.

In production this scenario cannot occur: `IntelligenceContext` is always
built with `ContextBounds::default()` (20 recent findings) by the Tauri
layer - the 1,000-finding case is a stress test of the algorithm's
scaling, not a state the running application can reach. At the actual
production bound (20 findings), `analyze()` costs well under a
millisecond even against an adversarial worst case.

## Offline guarantee

Phase 2.4 adds zero new dependencies to any `Cargo.toml` and zero new
frontend dependencies - `cargo tree -p cip-core-intelligence` and
`cargo tree -p cip-desktop` are unchanged from Phase 2.3. `core/intelligence`'s
normal dependency tree still carries no network-related crate; see
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

## NOT AVAILABLE / NOT VERIFIED

- No semantic or theological reasoning of any kind - "Amazing Grace" and
  Romans 8 are never correlated merely because both are about grace; only
  explicit transcript-position evidence (shared/near segments) or an
  explicit shared Scripture reference ever produces a correlation.
- No automatic presentation, preparation, or projection triggered by any
  correlation, at any confidence level.
- No cloud service, LLM, embeddings, vector database, or graph database of
  any kind.
- No new persistence: a correlation queue does not survive an application
  restart (matching `FindingQueue`'s own Phase 2.0 decision).
- `TemporalTier::Recent` (±10 segments) is computed but never used to gate
  any rule - proximity beyond `Near` is not verified as meaningful
  evidence for anything in this phase.
- No lyric-content or song-title matching against sermon/theme text of any
  kind (`rule_theme_music`'s explicit scope limit).
- The correlation queue is not exposed as a `list_music_findings`-style
  per-domain filter; `list_cross_domain_correlations` returns every
  pending correlation for the active service regardless of which domains
  it connects.

[`IntelligenceEngine`]: /core/intelligence/src/engine.rs
[`IntelligenceCorrelation`]: /core/intelligence/src/correlation.rs
[`CorrelationKind`]: /core/intelligence/src/correlation.rs
[`CorrelationQueue`]: /core/intelligence/src/cross_domain.rs
[`FindingQueue`]: /core/intelligence/src/queue.rs
