# Intelligence Architecture (Phase 2.0)

This document explains `core/intelligence` - the shared contracts future
Bible/Music/Sermon/Content/Cross-Domain intelligence engines are built on.
It does **not** implement Music, Sermon, or Content intelligence; those
remain **PLANNED / NOT IMPLEMENTED**. The only real engine in this phase is
a thin compatibility adapter over the Bible Intelligence Core Phase 1.1
already built - see "Bible compatibility" below.

## 1. Phase 2 vision

Phase 1 built one working chain:

```
AUDIO -> SPEECH -> TRANSCRIPT -> BIBLE INTELLIGENCE -> SUGGESTION -> OPERATOR -> PRESENTATION
```

Phase 2 generalizes the middle of that chain into a shared architecture
multiple independent intelligence domains can sit behind, without ever
calling each other directly:

```
Transcript -> IntelligenceContext -> IntelligenceInput
   -> independent IntelligenceEngine instances (Bible, Music, Sermon, ...)
   -> IntelligenceResult -> IntelligenceFinding -> operator workflow
```

The central principle: one unified, bounded context; multiple independent
engines; shared evidence/confidence/provenance; failure isolation; human
control. Phase 2.0 establishes exactly this - the contracts, not the
engines.

## 2. Intelligence domains

`IntelligenceDomain` (`core/intelligence/src/domain.rs`) is a closed enum:
`Bible`, `Music`, `Service`, `Sermon`, `Content`, `CrossDomain`. It answers
*which* intelligence domain produced a finding. Only `Bible` has a real
engine registered anywhere in this codebase - see section 24.

## 3. `IntelligenceFinding`

The one thing every engine produces:

```rust
pub struct IntelligenceFinding {
    pub id: Uuid,
    pub service_id: Uuid,
    pub domain: IntelligenceDomain,
    pub kind: FindingKind,
    pub assertion_level: AssertionLevel,
    pub status: FindingStatus,
    pub priority: IntelligencePriority,
    pub confidence: ConfidenceResult,
    pub summary: String,
    pub transcript_segment_ids: Vec<Uuid>,
    pub evidence: Vec<EvidenceSource>,
    pub provenance: IntelligenceProvenance,
    pub engine_id: String,
    pub engine_version: String,
    pub created_at: DateTime<Utc>,
}
```

Deliberately not coupled to SQLite or any storage row type - engines
construct and return these; nothing reads one back from a database table
in Phase 2.0 (see section 20).

## 4. Finding kinds

`FindingKind` (`Scripture`, `Music`, `ServiceState`, `Sermon`, `Content`,
`Correlation`) answers *what shape* of thing was found - a separate axis
from `IntelligenceDomain` (*which engine* produced it). A future
`CrossDomainEngine` finding is still, structurally, a `Correlation`-kind
finding; a `BibleEngine` finding is always `Scripture`-kind. These two
enums are never merged.

## 5. Status lifecycle

`FindingStatus`: `Detected -> Reviewed -> Accepted`/`Rejected`, or
`Expired` from either open state. A **separate** state machine from
`cip_core_presentation::PresentationItemStatus` - accepting a finding
changes only this struct's `status`; it has no field or method capable of
constructing a `PresentationItem`, so it is structurally incapable of
side-effecting presentation state (`core/intelligence` has no dependency
on `cip-core-presentation` at all - see `Cargo.toml`). See
`finding::tests::accepting_a_finding_has_no_presentation_side_effect` and
`queue::tests::accepting_a_finding_never_creates_a_presentation_item`.

## 6. Observed / Inferred / Suggested / Generated

The mandatory epistemic-state distinction, `AssertionLevel`:

```text
OBSERVED   - what was actually said (the transcript text itself)
INFERRED   - a state CIP derived (e.g. active Scripture context = Romans 8)
SUGGESTED  - a specific proposal for human review (e.g. Romans 8:28)
GENERATED  - synthesized content - reserved, not produced by anything here
```

The Bible compatibility adapter maps a `Chapter` detection (context
established, no verse) to `Inferred`, and a `Direct`/`Verse`/`Sequential`
detection (a concrete, validated verse) to `Suggested` - exactly mirroring
what a Phase 1 `Suggestion` already represented. Nothing in this crate
produces `Generated` findings; the level exists so a future engine cannot
silently collapse generated content into `Suggested`.

## 7. Evidence

`EvidenceSource` (`core/intelligence/src/evidence.rs`) explains *why* a
finding was produced - a tagged enum, not one struct with dozens of
unrelated optional fields:

```rust
pub enum EvidenceSource {
    Transcript { segment_ids: Vec<Uuid>, excerpt: String },
    Content { content_id: String },
    Context { description: String },
    Temporal { description: String },
    AnotherFinding { finding_id: Uuid },
    ServiceEvent { description: String },
    OperatorAction { description: String },
}
```

## 8. Provenance

`IntelligenceProvenance` is deliberately thin: `{ content_id: Option<String>,
note: Option<String> }`. It references the Phase 1.5 Content Registry by
id (`"bible:KJV"`) rather than re-implementing its licensing model - "do
not invent a second licensing system." `content_id: None` means this
finding has no content-registry-backed source, never a guess.

## 9. Confidence

Reused directly: `IntelligenceFinding.confidence: cip_core_confidence::ConfidenceResult`.
No second confidence system was created.

## 10. Priority

`IntelligencePriority` (`Low`/`Normal`/`High`/`Critical`) is **not** a
restatement of confidence. `baseline_priority(kind, confidence)` is
deterministic: confidence sets the floor (`Low` confidence never exceeds
`Low` priority), and `kind` can raise it from there -
`ServiceState`/`Correlation` findings start at `High` regardless of
confidence bucket, since they describe the service itself, not just a
candidate piece of content. No machine learning, no randomness. See
`domain::tests::priority_is_not_a_restatement_of_confidence` for the exact
proof (a lower-confidence `ServiceState` finding outranks a
higher-confidence `Scripture` one).

## 11. `IntelligenceContext`

The bounded, explicit slice of live-service state an engine may see -
never "the database," never unrestricted application state:

```rust
pub struct IntelligenceContext {
    pub service_id: Uuid,
    pub service_status: Option<ServiceStatus>,
    pub current_transcript_segment: Option<TranscriptSegment>,
    pub recent_transcript_segments: Vec<TranscriptSegment>,
    pub active_scripture_context: Option<ScriptureContext>,
    pub recent_findings: Vec<IntelligenceFinding>,
    pub recent_service_events: Vec<ServiceEventSummary>,
    pub content_metadata: Vec<ContentMetadata>,
    pub bounds: ContextBounds,
}
```

`ScriptureContext`/`TranscriptSegment`/`ServiceStatus`/`ContentMetadata`
are reused exactly from `core/bible`/`core/ai`/`core/service`/`core/content`
- not duplicated. Four conceptual context-window levels
(`ContextWindow::Current`/`ShortTerm`/`MediumTerm`/`Service`) name which
slice of this a caller means; `IntelligenceContext::window()` resolves one.

## 12. Context bounds

`ContextBounds` fixes three limits, defaulting to 20 each:

| Bound | Default | Why |
| --- | --- | --- |
| `max_recent_transcript_segments` | 20 | Matches `TRANSCRIPT_LIMIT` already used by the Live Church Brain frontend for the same reason - enough real conversational context, never a whole service's transcript. |
| `max_recent_findings` | 20 | Enough for a later-running engine to see nearby findings without accumulating a whole service's history. |
| `max_recent_service_events` | 20 | Same reasoning, scoped to service-lifecycle/operator-action events. |

`IntelligenceContext::build()` truncates every bounded collection to these
limits regardless of input size. Proven directly: feeding 10,000 synthetic
transcript segments in still produces a context holding exactly 20 -
`context::tests::ten_thousand_transcript_segments_never_produce_an_unbounded_context`.

## 13. `IntelligenceInput`

What one `analyze` call is actually about: `{ service_id, transcript_segment,
runtime: RuntimeCapabilities }`. `RuntimeCapabilities` is deliberately
minimal (`{ offline_only: bool }`, defaulting `true`) - CIP is always
offline-only, so that is the one fact worth carrying; not a speculative
capability-negotiation system for engines that don't exist yet.

## 14. `IntelligenceResult`

`{ findings: Vec<IntelligenceFinding>, processing_ms: Option<u64> }` -
always zero, one, or many findings, never a mutation of anything outside
itself. `analyze()` returns `Result<IntelligenceResult, IntelligenceError>`,
so a recoverable failure is a typed `Err`, never silently swallowed into
"no findings."

## 15. `IntelligenceEngine`

```rust
pub trait IntelligenceEngine: Send + Sync {
    fn identity(&self) -> EngineIdentity;
    fn capability(&self) -> EngineCapability;
    fn analyze(&self, input: &IntelligenceInput, context: &IntelligenceContext)
        -> Result<IntelligenceResult, IntelligenceError>;
}
```

Synchronous and deterministic by design - no executor, no message broker,
no async complexity. Engines must never call one another directly; the
only channel between them is whatever the orchestrator puts into
`context.recent_findings`. `EngineIdentity { domain, engine_id, engine_version }`
is a stable identity, not a model registry.

## 16. Engine registry

`IntelligenceEngineRegistry` is in-process only - no dynamic plugin
loading, no network-loaded engines. `register()` rejects a duplicate
domain rather than silently replacing an engine (the same "never silently
overwrite" discipline `docs/bible-datasets.md`'s importer already
follows). `resolve(domain)` returns `None` for a domain with no registered
engine - never a placeholder.

## 17. Engine capabilities

`EngineCapability`: `Available` / `Unavailable` / `Disabled` / `Error`.
Distinguishes "not installed" (`Unavailable` - e.g. every domain but Bible
in Phase 2.0) from "installed but turned off" (`Disabled`) from "installed
but currently broken" (`Error`). `registry.capabilities()` only lists
domains with a registered engine at all - a diagnostic UI checks presence,
not a placeholder value, to tell "not registered" apart from "registered
but currently unavailable."

## 18. Engine failure isolation

`IntelligenceEngineRegistry::analyze_all()` calls every `Available`
registered engine and returns one `EngineOutcome` per engine, always -
including when an engine panics. Each call is wrapped in
`std::panic::catch_unwind`, converting a panic into an
`IntelligenceError::EngineFailed`, so one engine's failure - even a hard
panic - can never take down another engine's result or the calling
service. Proven directly:
`registry::tests::a_failing_engine_does_not_affect_a_succeeding_one` and
`registry::tests::a_panicking_engine_is_isolated_and_never_propagates`.

## 19. Correlation

`IntelligenceCorrelation { id, source_finding_ids, kind, confidence,
evidence, created_at }` is the foundation for cross-domain correlation -
deliberately minimal, no graph algorithms. `CorrelationKind` has
`TemporalProximity`/`SharedContext`/`Other(String)`. Nothing in this
codebase constructs a real correlation yet; this is the shape a future
`CrossDomainEngine` will use.

## 20. Finding queue

`FindingQueue` is an **in-memory only** domain-level abstraction -
deliberately not persisted. Nothing yet needs a finding to survive a
restart the way suggestions/presentation items already do (those have
their own tables); adding an `intelligence_findings` table with no real
writer would be exactly the kind of speculative schema this phase's spec
explicitly warns against. Responsibilities: `add` (with deterministic
duplicate handling - two findings from the same service/domain/kind/summary
are the same finding while one is still pending review),
`pending()` (priority-then-confidence ordered), `review`/`accept`/`reject`,
`expire_older_than`. It cannot prepare a presentation, project a slide,
publish content, or modify the transcript - it has no dependency on any of
those types at all.

## 21. Temporal model

Every finding traces to its originating transcript segment(s)
(`transcript_segment_ids`) and its own `created_at`. No second clock
system was invented - `created_at` uses the same `chrono::DateTime<Utc>`
convention every other Phase 1 domain type already uses, and
`ServiceEventSummary.occurred_at` reuses whatever timestamp the caller's
own timeline/event source already recorded.

## 22. Human-in-the-loop boundary

Nothing in `core/intelligence` can automatically approve, reject, prepare,
project, publish, or mutate a transcript. `FindingStatus` only advances
through an explicit call to `review()`/`accept()`/`reject()`/`expire()` -
methods `core/intelligence`'s own code never calls itself. Accepting a
finding still only ever moves `status` on the struct itself; there is no
code path from an `IntelligenceFinding` to a `PresentationItem` anywhere
in this crate (see section 5).

## 23. Offline operation

`core/intelligence` depends on nothing beyond `serde`/`chrono`/`uuid`/
`thiserror` and the existing `core/*` domain crates
(`cip-core-confidence`/`cip-core-bible`/`cip-core-ai`/`cip-core-service`/
`cip-core-content`) - no Tauri, no React, no SQLite implementation, no
`cpal`, no `whisper-rs`, no network client. Verified structurally: `cargo
tree -p cip-core-intelligence` shows only those crates, and a
whole-workspace scan for `reqwest`/`hyper`/`ureq`/`tungstenite` finds none.

## 24. Bible compatibility (the one real engine)

`BibleIntelligenceEngine` (`core/intelligence/src/bible_adapter.rs`) is the
"adapter/boundary" this phase's spec asks for - it does **not**
reimplement Bible Intelligence. Every `analyze()` call delegates entirely
to the unchanged `cip_core_service::process_transcript_segment` (the same
function Phases 1.1-1.5 already use) and translates its output into
findings. `core/bible` and `core/service` were not touched by this phase
at all - confirmed by `git diff --stat core/bible core/service` showing no
changes - and every existing `core/service::bible_intelligence` test still
passes unmodified, proving the Romans 8 -> verse 28/31/18 and
Romans 8 -> John 3 -> verse 16 regression behavior is exactly preserved.

Registered in the desktop app (`apps/desktop/src-tauri/src/intelligence.rs`)
against a real, dedicated `SqliteBibleProvider` connection - exercised only
by the `get_intelligence_capabilities` diagnostic command and this
module's own tests; the live transcript pipeline
(`pipeline.rs::handle_final_transcript`) is completely unchanged and does
not call into it.

### Future Music / Sermon / Content engines - PLANNED / NOT IMPLEMENTED

`IntelligenceDomain::Music`/`Sermon`/`Content` and the matching
`FindingKind` variants reserve the shape those engines will occupy. No
song recognition, audio fingerprinting, sermon summarization, topic
extraction, or content generation exists anywhere in this codebase. A
future engine for any of these domains implements `IntelligenceEngine`,
gets registered in `IntelligenceEngineRegistry`, and receives the same
bounded `IntelligenceContext` the Bible engine does - never a direct call
to `BibleIntelligenceEngine` or any other engine.

## Performance

Measured directly (`std::time::Instant`, release build, this machine, one
run - not a formal benchmark harness, using a throwaway test file deleted
before commit, matching the Phase 1.5 measurement methodology):

| Operation | Observed |
| --- | --- |
| `IntelligenceContext::build` from 10,000 synthetic transcript segments (truncating to 20) | ~295µs |
| One `IntelligenceFinding::new` construction | ~438ns |
| `IntelligenceEngineRegistry::resolve` | ~11ns |
| `IntelligenceEngineRegistry::analyze_all` (one registered engine) | ~554ns |
| `FindingQueue::add` (average, building up to 1,000 queued findings - each add does an O(n) duplicate scan) | ~2.4µs |
| `FindingQueue::pending()` over 1,000 findings | ~4.9µs |

Real numbers from one measurement pass, not "instant"/"real-time" claims.
Every operation here is sub-millisecond even at these synthetic scales,
far above what a live service actually produces per transcript segment.

## Verifying this architecture

```sh
cargo test -p cip-core-intelligence   # 56 tests: every type + the
                                       # architectural acceptance scenarios
cargo test -p cip-desktop intelligence::   # real-app-state wiring tests
cargo test -p cip-core-service              # unchanged Bible regression suite
```

See `docs/architecture.md#domains-core` for where `core/intelligence` sits
in the overall crate layout, and `docs/live-service.md` for how this
relates to the operator workflow it sits above without changing.
