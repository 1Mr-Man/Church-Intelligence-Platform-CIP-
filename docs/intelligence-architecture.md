# Intelligence Architecture (Phase 2.0, extended in Phase 2.1 and 2.3)

This document explains `core/intelligence` - the shared contracts
Bible/Music/Sermon/Content/Cross-Domain intelligence engines are built
on. Phase 2.0 established the architecture with exactly one real engine:
a thin compatibility adapter over the Bible Intelligence Core Phase 1.1
already built - see section 24. Phase 2.1 added the first real *second*
engine, Music - see section 25 and
[`docs/music-intelligence.md`](music-intelligence.md). Phase 2.3 added a
third, Sermon - see section 26 and
[`docs/sermon-intelligence.md`](sermon-intelligence.md). A fourth engine,
Service Intelligence, was added under this repository's authoritative
Phase 2 roadmap's actual Phase 2.4 - see
[`docs/service-intelligence.md`](service-intelligence.md). The roadmap's
actual Phase 2.5, Sermon Intelligence Foundation, does not add a fifth
engine - it extends `IntelligenceContext` additively instead (a new
`with_sermon_context` builder method, never a required constructor
argument, so every existing call site is unchanged) - see
[`docs/sermon-foundation.md`](sermon-foundation.md)'s "Sermon context"
and "Engine boundary" sections. Content Intelligence and the formal,
roadmap Phase 2.8 Cross-Domain Intelligence validation remain
**PLANNED / NOT IMPLEMENTED**; an earlier cross-domain correlation rule
engine was already built under an internal label that also read
"Phase 2.4" (see [`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md))
and is reserved for that future Phase 2.8 validation - that historical
label is not rewritten. `core/sermon`'s own semantic detection modules
(section 26 above) were similarly built under an internal label that
read "Phase 2.3" and are understood as Phase 2.6-equivalent under this
roadmap - also not rewritten.

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
*which* intelligence domain produced a finding. `Bible` (section 24), as
of Phase 2.1 `Music` (section 25), and as of Phase 2.3 `Sermon` (section
26) have real engines registered; `Service`/`Content`/`CrossDomain`
remain reserved shape only. Phase 2.4's `CrossDomainCorrelationEngine`
(section 19) does not change this: it produces `IntelligenceCorrelation`s,
not findings, so `IntelligenceDomain::CrossDomain` still tags nothing -
a correlation's `domains` field instead lists whichever *source* domains
(Bible/Music/Sermon/Service) its underlying findings actually came from.

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
evidence, created_at }` was the Phase 2.0 foundation for cross-domain
correlation - deliberately minimal, no graph algorithms.

As of **Phase 2.4**, this foundation has a real, deterministic consumer:
`core/intelligence::cross_domain::CrossDomainCorrelationEngine`, which
reads `IntelligenceContext.recent_findings` and derives correlations
between Bible/Music/Sermon findings. It is not registered into the
`IntelligenceEngineRegistry` below (its output type doesn't fit
`IntelligenceEngine::analyze`'s `Vec<IntelligenceFinding>` shape) and calls
no other engine directly - the only channel between domains remains
`IntelligenceContext`, exactly as this section always required.
`IntelligenceCorrelation` was extended additively (`service_id`, `domains`,
`assertion_level`, `status`, `summary`, `rule_id`, `rule_version` added;
every Phase 2.0 field kept), and `CorrelationKind` gained new domain-pair
variants (`ScriptureSermon`, `ScriptureMusic`, `SermonMusic`,
`ThemeScripture`, `ThemeMusic`, `ServiceTransition`) alongside the
original `TemporalProximity`/`SharedContext`/`Other(String)`. See
[`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md) for
the full rule catalogue, confidence hierarchy, and design rationale.

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
`cip-core-content`/`cip-core-music`, the last added in Phase 2.1) - no
Tauri, no React, no SQLite implementation, no `cpal`, no `whisper-rs`, no
network client. Verified structurally: `cargo tree -p cip-core-intelligence`
shows only those crates, and a whole-workspace scan for
`reqwest`/`hyper`/`ureq`/`tungstenite` finds none. `core/music` itself is
held to the same standard - see
[`docs/music-intelligence.md`](music-intelligence.md#offline-guarantee).

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

## 25. Music (the second real engine, Phase 2.1)

`MusicIntelligenceEngine` (`core/intelligence/src/music_adapter.rs`) is
Phase 2.1's proof that this architecture generalizes beyond Bible: a
second, independently-registered `IntelligenceEngine` that never calls
`BibleIntelligenceEngine` (or vice versa), sharing only what the
orchestrator puts into `IntelligenceContext`. Like the Bible adapter, it
does not reimplement recognition logic itself - every candidate comes
from `cip_core_music::search_songs`, a deterministic, documented,
explainable matcher (title/alias/hymn-number/lyric text - explicitly
**not** audio fingerprinting; see
[`docs/music-intelligence.md`](music-intelligence.md#what-song-recognition-means-here-honestly)
for exactly what is and isn't implemented).

Registered in the desktop app (`apps/desktop/src-tauri/src/music.rs`)
against a real, dedicated `SqliteMusicProvider` connection, alongside the
Bible engine in the same `IntelligenceEngineRegistry`. Exercised by the
manual `analyze_music_transcript` command (the Music counterpart to
`process_test_transcript`) and by `get_intelligence_capabilities`; the
live audio/speech transcript pipeline still only calls Bible detection,
unchanged.

Full details - the confidence hierarchy, text normalization policy,
free-text dispatch heuristic, song continuity, and ambiguity handling -
live in [`docs/music-intelligence.md`](music-intelligence.md), not
duplicated here.

### Acoustic recognition (Phase 2.2)

`MusicIntelligenceEngine::analyze_acoustic` (an inherent method, not part
of the shared `IntelligenceEngine` trait - it takes an `AudioSegment`,
which does not fit `IntelligenceInput`'s shape) adds a second, real
recognition path: audio-fingerprint/embedding recognition, fused with
lyric/title evidence rather than competing with it. Same non-negotiable
rule as the rest of this architecture: it never calls Bible logic, and
Bible never calls it - both still only ever share what's in
`IntelligenceContext`. Full details - segmentation, the signal-quality
gate, the recognizer contract and its three implementations, evidence
fusion, ambiguity/continuity/transitions, "Current Song," and honest
`Unavailable` reporting - live in
[`docs/acoustic-music.md`](acoustic-music.md).

## 26. Sermon (the third real engine, Phase 2.3)

`SermonIntelligenceEngine` (`core/intelligence/src/sermon_adapter.rs`) is
Phase 2.3's proof that this architecture generalizes to a third,
completely independent domain: it never calls `BibleIntelligenceEngine`
or `MusicIntelligenceEngine` (or vice versa), sharing only what the
orchestrator puts into `IntelligenceContext` - specifically,
`active_scripture_context` for its one cross-linking case (see below).
Like the Bible/Music adapters, it does not implement detection logic
itself - every `SermonDetection` comes from `cip_core_sermon`, a pure,
deterministic, phrase-anchored detector with no dependency on
`core/intelligence` or any other domain crate.

Unlike Bible/Music, `SermonIntelligenceEngine` needs no external provider
or database connection at all - it is fully in-process, deterministic
logic. It is registered twice in the desktop app
(`apps/desktop/src-tauri/src/sermon.rs`): once into the shared
`IntelligenceEngineRegistry` (diagnostics/failure-isolation symmetry
only) and once as `AppState.sermon_engine` (the real, stateful instance
every command actually uses) - see
[`docs/sermon-intelligence.md`](sermon-intelligence.md#operator-workflow-tauri-commands)
for why these are deliberately separate instances. Exercised by the
manual `analyze_sermon_transcript` command; the live audio/speech
transcript pipeline still only calls Bible detection, unchanged.

Full details - the sermon taxonomy, theme-evidence policy, structure
tracking, Scripture cross-linking, and the operator workflow - live in
[`docs/sermon-intelligence.md`](sermon-intelligence.md), not duplicated
here.

### Remaining Content / CrossDomain engines - PLANNED / NOT IMPLEMENTED

`IntelligenceDomain::Content`/`CrossDomain` and the matching
`FindingKind` variants still only reserve the shape those engines will
occupy. No content-domain intelligence or real cross-domain correlation
logic exists anywhere in this codebase. A future engine for either of
these domains implements `IntelligenceEngine`, gets registered in
`IntelligenceEngineRegistry`, and receives the same bounded
`IntelligenceContext` Bible, Music, and Sermon already do - never a
direct call to any existing engine.

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

See [`docs/music-intelligence.md`](music-intelligence.md#performance) for
the Phase 2.1 Music-specific measurements (matcher, engine, and the full
real-SQLite orchestration path),
[`docs/acoustic-music.md`](acoustic-music.md#performance) for the Phase
2.2 acoustic-pipeline measurements (segmentation, the signal-quality
gate, fusion, and the full `analyze_acoustic` path), and
[`docs/sermon-intelligence.md`](sermon-intelligence.md#performance) for
the Phase 2.3 Sermon-specific measurements (detection, and the full
`analyze` path).

Real numbers from one measurement pass, not "instant"/"real-time" claims.
Every operation here is sub-millisecond even at these synthetic scales,
far above what a live service actually produces per transcript segment.

## Verifying this architecture

```sh
cargo test -p cip-core-intelligence   # every type + the architectural
                                       # acceptance scenarios (Bible,
                                       # Music, and Sermon - Phase 2.0,
                                       # 2.1, 2.3)
cargo test -p cip-core-sermon               # pure sermon detection/theme/structure tests
cargo test -p cip-desktop intelligence::   # real-app-state wiring tests
cargo test -p cip-desktop music::           # Music orchestration + degradation tests
cargo test -p cip-desktop sermon::          # Sermon orchestration + operator-workflow tests
cargo test -p cip-core-service              # unchanged Bible regression suite
```

See `docs/architecture.md#domains-core` for where `core/intelligence` sits
in the overall crate layout, and `docs/live-service.md` for how this
relates to the operator workflow it sits above without changing.
