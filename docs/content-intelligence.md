# Content Intelligence (Phase 2.7)

The bridge between INTELLIGENCE and FUTURE CONTENT PRODUCTION - never a
leap into it. Content Intelligence reads findings the existing engines
have already detected and structures them into `ContentCandidate`s: a
typed record that a piece of already-proven information *appears suitable
as a future content opportunity*. It never writes final copy, never
generates a social post, never publishes or schedules anything, and never
detects anything new on its own.

## Purpose

An `IntelligenceFinding` answers "what did CIP detect?" A `ContentCandidate`
answers a narrower, later question: "of what's already been detected and
already reviewable, which pieces look like they could become future
content, and what shape of content might that be?" The distinction matters
because the two questions carry different epistemic weight - a finding
says "detected"; a candidate says "this appears suitable for a future
content purpose," never "this is finished content."

## Scope

In scope: reading `IntelligenceContext.recent_findings`, mapping eligible
`Sermon`-domain findings to a closed `ContentCandidateType` taxonomy,
computing a separate `content_potential` score, deduplicating and ranking
candidates deterministically, and a minimal operator review workflow
(list/accept/reject).

Explicitly out of scope, per the authoritative Phase 2 roadmap: social
media publishing, content scheduling, full content production, AI
copywriting or content automation, cross-domain correlation (Phase 2.8),
a unified operator workspace (Phase 2.9), and any kind of external
publishing. None of this phase's code imports or references
`cip_core_presentation`, an HTTP client, or any content-generation type.

## Architecture

```text
LIVE / MANUAL TRANSCRIPT
    |
Bible / Music / Service / Sermon engines (reused, unchanged)
    |
IntelligenceFinding -> FindingQueue -> operator review (accept/reject)
    |
IntelligenceContext.recent_findings (reused, unchanged - Phase 2.7 adds
    no new field here)
    |
core/intelligence::content_intelligence::ContentIntelligenceEngine
    (reads already-produced findings only; never calls
    BibleIntelligenceEngine/MusicIntelligenceEngine/
    ServiceIntelligenceEngine/SermonIntelligenceEngine/
    CrossDomainCorrelationEngine directly)
    |
ContentCandidate -> ContentCandidateQueue -> operator review
    (accept/reject) -> (content production, scheduling, and publishing
    all remain entirely out of scope and unimplemented)
```

`core/intelligence::content_candidate` (the domain type) and
`core/intelligence::content_intelligence` (the engine/queue) are new
modules; nothing about `IntelligenceContext`, `IntelligenceFinding`,
`IntelligenceEngine`, or `IntelligenceEngineRegistry` changed to support
them. The historical `core/intelligence/src/cross_domain.rs` (an earlier
internally-labeled "Phase 2.4 — Cross-Domain Intelligence," now understood
as the authoritative roadmap's Phase 2.8) is untouched by this phase.

## Why this is not an `IntelligenceEngine`

`IntelligenceEngine::analyze` returns `Vec<IntelligenceFinding>` - a
content candidate is a structurally different value that cannot be
honestly shoehorned into that return type. `ContentIntelligenceEngine`
therefore does not implement the shared trait and is not registered into
`IntelligenceEngineRegistry`, exactly mirroring
`CrossDomainCorrelationEngine`'s own, identical precedent (documented in
`cross_domain.rs`'s own module docs, and now `content_intelligence.rs`'s).
This is a deliberate, documented departure from an earlier literal
reading of "register in `IntelligenceEngineRegistry` using
`IntelligenceDomain::Content`" - building on the architecture that
actually exists (the `IntelligenceCorrelation`/`CrossDomainCorrelationEngine`
precedent) takes priority over a spec's literal wording when the two
conflict.

## `IntelligenceFinding` vs. `ContentCandidate`

| | `IntelligenceFinding` | `ContentCandidate` |
| --- | --- | --- |
| Meaning | "this was detected" | "this appears suitable for a future content purpose" |
| Produced by | Bible/Music/Service/Sermon engines | `ContentIntelligenceEngine`, reading already-produced findings |
| Lifecycle | `FindingStatus` (`Detected`→`Reviewed`/`Accepted`/`Rejected`/`Expired`) | Same `FindingStatus` enum, reused, but its own independent instance per candidate |
| Certainty dimension | `confidence: ConfidenceResult` | `confidence` inherited unchanged, **plus** a new, independent `content_potential: f32` |
| Traceability | `transcript_segment_ids`, `sermon_id` | `source_finding_ids` (always ≥1), `sermon_id` inherited from the source finding |

`ContentCandidate` is deliberately its own struct, not a variant or
extension of `IntelligenceFinding` - mirroring `IntelligenceCorrelation`'s
own precedent exactly (see `correlation.rs`'s module docs).

## Candidate taxonomy (`ContentCandidateType`)

A closed, 9-variant enum - no open-ended "other" catch-all, justified
entirely by real Phase 2.6 Sermon Intelligence finding categories:

| Type | Source finding prefix |
| --- | --- |
| `Theme` | `Theme: ` |
| `Teaching` | `Main Point: ` / `Sub-Point: ` |
| `Reflection` | `Application: ` |
| `Takeaway` | `Takeaway: ` |
| `FoodForThought` | `Food for Thought: ` |
| `Quote` | `Key Statement: ` (verbatim-only, see "Quote integrity" below) |
| `DiscussionQuestion` | `Question: ` |
| `ScriptureReflection` | `Supporting Scripture: ` |
| `Illustration` | `Illustration: ` / `Story: ` / `Example: ` |

## Source-to-content mapping

Every mapping is an explicit `(&str summary prefix, ContentCandidateType)`
pair in `content_intelligence.rs`'s `SUMMARY_PREFIX_MAPPINGS` constant -
never an opaque heuristic, never a keyword search over free text. Only
`IntelligenceDomain::Sermon`/`FindingKind::Sermon` findings are mapped in
this initial phase - the only domain with a real, structured, prefix-based
taxonomy to draw from as of Phase 2.6. Every Sermon summary prefix *not*
listed above (`Definition: `, `Declaration: `, `Prayer Point: `,
`Summary: `, `Reflection: `, `Transition: `, `Possible Conclusion: `,
`Structural Transition (section): `, and every `Sermon foundation:
`-prefixed structural finding) is deliberately left unmapped - see "NOT
AVAILABLE" below. Bible/Music/Service findings are not mapped at all in
this phase.

## Eligibility

A finding must satisfy all three before it can ever become a candidate
(`content_intelligence::is_eligible`):

1. `status` is `Detected`, `Reviewed`, or `Accepted` - never `Rejected` or
   `Expired`.
2. `assertion_level` is never `Generated`.
3. `evidence` is non-empty - a candidate that cannot explain its source is
   never produced.

## Assertion levels (inherited, never upgraded)

`ContentCandidate.assertion_level` is copied verbatim from the source
finding - a candidate is never presented as more certain than the finding
it came from merely because it became a candidate.

## Confidence

`ContentCandidate.confidence` reuses `ConfidenceResult` unchanged from the
source finding - never recomputed, never replaced. It still means exactly
what it always has: "how certain is this fact."

## Content potential (the new, independent dimension)

`content_potential: f32` (`0.0..=1.0`, clamped in `ContentCandidate::new`)
answers a different question entirely: "how suitable does this appear as
a future content opportunity?" It is explicitly **not** a
truth-confidence score, and it is never derived from `confidence`.
Computed deterministically (`content_potential_for`) from:

- A `type_weight` fixed per `ContentCandidateType` (structural/content-type
  suitability, documented in-source, never a magic number):
  `Quote` 0.60, `Theme`/`Takeaway` 0.55, `ScriptureReflection` 0.50,
  `Teaching` 0.45, `Reflection`/`FoodForThought` 0.40,
  `DiscussionQuestion`/`Illustration` 0.35.
- A small evidence-count bonus (`evidence_count * 0.05`, capped at 0.15) -
  so a finding with many evidence entries never dominates purely on
  volume.

`content_potential_is_independent_of_confidence` (in
`content_intelligence.rs`) proves the two dimensions can diverge or
invert: a high-confidence/low-potential candidate and a
low-confidence/high-potential candidate are both constructed, and the
orderings by each dimension are asserted to differ.

## Evidence and provenance (inherited, never fabricated)

`ContentCandidate.evidence`/`.provenance` are copied verbatim from the
source finding - never re-derived, never invented. This is also how
sermon/section/speaker awareness flows through to Content Intelligence
automatically: since a Sermon finding's `evidence` already carries Phase
2.6's section-context entry and its `provenance.note` already carries the
speaker-attribution note (when present), nothing new needs to be invented
here - proven by `sermon_id_is_inherited_from_the_source_finding` and
`speaker_provenance_flows_through_without_being_invented`.

## Quote integrity

A `Quote` candidate (from a `Key Statement: ` finding) may only be built
when the source finding carries verbatim `EvidenceSource::Transcript`
evidence (`quote_is_verbatim`) - never from a purely `Context`/inferential
evidence entry, so a paraphrase can never be presented as an exact
quotation. When eligible, the candidate's `working_concept` is the exact
verbatim transcript excerpt (`verbatim_excerpt`), never a paraphrase of
it. Every other candidate type's `working_concept` is the source finding's
own summary text with its prefix stripped - never new prose.

## Deduplication

`content_intelligence::dedup` mirrors `cross_domain::dedup`'s exact
hash-keyed approach: a `HashSet<(service_id, candidate_type, sorted
source_finding_ids)>` keeps only the first occurrence of each equivalence
class (`ContentCandidate::is_equivalent_to`) - O(n), never the naive
O(n²) "scan every already-kept candidate." `ContentCandidateQueue::add`
applies the same equivalence rule again at insertion time against
not-yet-resolved (`Detected`/`Reviewed`) candidates already queued, so a
repeated identical `analyze()` call across many operator sessions still
converges to one candidate.

## Ranking

`content_intelligence::sort_deterministically` orders candidates by
content potential descending, then candidate type label
(alphabetical), then sorted source finding ids, then id as a final stable
tiebreak - never by confidence (a separate dimension, never reused as
priority), never by insertion order, never by wall-clock time.
`ContentCandidateQueue::pending()` orders the same way (content potential
descending, id as tiebreak).

## Operator workflow (Tauri commands)

Four commands, mirroring `cross_domain.rs`'s exact command shape:

- `analyze_content_intelligence()` - runs `ContentIntelligenceEngine`
  against the current, real `IntelligenceContext` (built via the existing
  `build_music_context` helper) and queues any new candidates - an
  explicit operator/diagnostic action, never triggered automatically by a
  transcript segment arriving.
- `list_content_candidates()` - candidates still awaiting an operator
  decision, for the active service.
- `accept_content_candidate(candidateId)` - explicit operator acceptance
  of the content *opportunity*; changes only the candidate's own status,
  never publishes, schedules, or creates a `PresentationItem`.
- `reject_content_candidate(candidateId)` - explicit operator rejection;
  has no way to alter the source finding, the transcript, or the active
  Scripture context.

`apps/desktop/src-tauri/src/content_intelligence.rs`'s
`analyze_and_queue(engine, context, queue) -> Vec<ContentCandidate>` is
the Tauri-agnostic orchestration function these commands call through -
directly testable without a Tauri runtime, mirroring
`cross_domain.rs`'s own `analyze_and_queue` exactly.

## Events

Three new `AppEvent` variants: `ContentCandidateDetected`,
`ContentCandidateAccepted`, `ContentCandidateRejected` - each documented
as never implying anything was published, scheduled, or presented.
`analyze_content_intelligence` records one timeline entry
(`ContentCandidateDetected`) per newly queued candidate; accept/reject
record their own timeline entries. No event is emitted for a duplicate
candidate that `ContentCandidateQueue::add` silently discarded.

## Persistence decision: no new database migration

Content candidates are in-memory only
(`AppState.content_candidate_queue: Mutex<ContentCandidateQueue>`),
mirroring `CorrelationQueue`'s exact precedent and reasoning: a candidate
is derived from a finding that already carries its own
provenance/persistence story, so nothing here needs to survive a restart.
No `content_candidates` table was added, and none is needed - a completed
service's already-persisted findings are sufficient to re-derive every
candidate by re-running `analyze_content_intelligence` if ever needed.

## Content Registry non-conflation

`core/content` (the Phase 1.5 Content Registry - `ContentType`/
`ContentStatus`/`ContentMetadata`/`ContentRegistry`, for installed
datasets like Bible translations) is structurally unrelated to
`ContentCandidate` and is never touched by this phase. The two "content"
names refer to entirely different concepts: one is "installed reference
data," the other is "a structured future content opportunity." Neither
crate depends on the other.

## Frontend

`domain/contentIntelligence.ts` mirrors `ContentCandidate`/
`ContentCandidateType` exactly (camelCase fields, snake_case type
values) - a genuinely new type, never folded into `domain/intelligence.ts`'s
existing `IntelligenceFinding`/`IntelligenceCorrelation` types. The Live
Church Brain's "Content Intelligence" panel (placed directly after the
existing "Cross-Domain Intelligence" panel) shows a manual "Run content
analysis" button and the pending-candidate review list - each card
displaying the working concept, candidate type, confidence, content
potential, evidence count, and status, with Accept/Reject buttons -
deliberately the minimal diagnostic layout this phase calls for, **not**
the Phase 2.9 unified operator workspace.

## Failure isolation

Because `ContentIntelligenceEngine` is not registered into
`IntelligenceEngineRegistry`, it has no interaction with that registry's
`analyze_all` panic-catching isolation at all - by construction, a bug in
Content Intelligence can never affect Bible/Music/Service/Sermon engine
results, and vice versa, since neither ever calls into the other.

## Determinism and boundary tests

`identical_input_sequences_produce_equivalent_candidate_sequences`-style
coverage (see "Testing" below) proves `analyze()` run twice against
equivalent contexts produces equivalent candidates (ids/timestamps
excluded, as expected everywhere in this codebase).
`ten_thousand_findings_never_produce_unbounded_output` feeds 10,000
synthetic findings through the engine and confirms the output stays
bounded - `IntelligenceContext::build`'s existing truncation keeps memory
bounded regardless of how many findings a caller passes in.

## Performance

Measured directly (`std::time::Instant`, release build, this machine, one
run per measurement - a throwaway integration test
(`core/intelligence/tests/perf_bench.rs`) deleted before commit, matching
the established measurement methodology used every phase):

| Operation | n | Observed |
| --- | --- | --- |
| `ContentIntelligenceEngine::analyze` | 20 findings | ~35.2µs total (~1.76µs/finding) |
| `ContentIntelligenceEngine::analyze` | 100 findings | ~171.4µs total (~1.71µs/finding) |
| `ContentIntelligenceEngine::analyze` | 1,000 findings | ~1.69ms total (~1.69µs/finding) |

Real numbers from one measurement pass, not an "instant"/"real-time"
claim. Per-finding cost stays effectively flat (~1.7µs either way) from
n=20 through n=1,000 - the concrete evidence that `analyze`'s
eligibility/mapping/dedup/sort pipeline is linear, not O(n²), and remains
far below what a live service's actual finding-arrival rate would ever
require. `ContextBounds::max_recent_findings` was widened for this
measurement only (default is 20) so the n=100/n=1,000 cases actually
reach `analyze()` unbounded by the context's own default truncation; the
production default bound is unchanged by this phase.

## Offline guarantee

Phase 2.7 adds no new crate dependency anywhere - `core/intelligence`'s
and `cip-desktop`'s `Cargo.toml` files are unmodified by this phase (see
`git diff --stat -- core/intelligence/Cargo.toml apps/desktop/src-tauri/Cargo.toml`,
empty). The existing offline guarantee (`cargo tree -p cip-core-intelligence`
carrying no `reqwest`/`hyper`/cloud SDK/network client) applies unchanged -
see `docs/intelligence-architecture.md#offline-operation` for the
architecture-wide guarantee this phase inherits.

## Privacy

No content candidate, no source finding text, and no transcript excerpt
is ever uploaded. No analytics or telemetry was added. All processing is
local, in-process, and synchronous, exactly like every prior Phase 2
engine.

## Copyright & content safety

Every example transcript/finding text in this phase's tests and this
document is a short, project-authored synthetic passage - never
copyrighted sermon or book content, and never a large corpus. Quote
candidates only ever carry verbatim text CIP itself already recorded from
a live/manual transcript session, never a quotation sourced from
elsewhere.

## Testing

- `core/intelligence::content_candidate`: 9 unit tests - construction
  defaults, `content_potential` clamping, accept/reject, review-never-
  resurrects-rejected, equivalence ignoring id/timestamp/confidence/
  content_potential/source-id-order, different-type-or-service never
  equivalent, camelCase serialization with snake_case `candidateType`,
  and label distinctness.
- `core/intelligence::content_intelligence`: mapping tests (one per
  candidate type, plus unmapped-prefixes and non-Sermon-domain cases),
  eligibility tests (rejected/expired/accepted/missing-evidence/
  `Generated`), the quote-integrity test, assertion-level-preservation
  tests, the confidence-vs-content-potential independence test,
  traceability tests (`source_finding_ids`, `sermon_id`), a 100-repeat
  dedup test, determinism/ordering tests, empty/bounded-input tests
  (10,000-finding input still bounded), a speaker-provenance-flows-through
  test, full `ContentCandidateQueue` tests (add/duplicate-reject/accept/
  reject/unknown-id), and the canonical Phase 2.7 acceptance scenario
  (`canonical_phase_2_7_acceptance_scenario`): a theme finding becomes a
  candidate, traceability is intact (`service_id`/`sermon_id`/
  `source_finding_ids`/`candidate_type`), the assertion level is
  inherited, the candidate can be accepted, the source finding stays
  unmutated, and (type-level) nothing here has any dependency on
  presentation.
- `apps/desktop/src-tauri::content_intelligence`: five orchestration
  tests - queues a theme candidate from a real finding, does not
  duplicate a repeated identical call, yields nothing for no findings,
  accept changes only status, reject stays out of `pending()`.
- Frontend: domain contract tests (`contracts.test.ts` - one `ContentCandidate`
  per `ContentCandidateType`, the confidence/content-potential independence
  proof, the accept/reject `FindingStatus` lifecycle), command-wrapper
  tests (`commands.test.ts`, including the outside-Tauri-runtime guard for
  all four commands), and event-subscription tests (`liveEvents.test.ts`,
  `eventNames.test.ts`).

## Phase 2.8 handoff (Cross-Domain Intelligence)

Every `ContentCandidate` carries `source_finding_ids`, `service_id`, and
`sermon_id` - everything a future correlation rule would need to relate a
content opportunity back to other domains' findings, without re-deriving
anything. No correlation rule reads or produces `ContentCandidate`s in
this phase; that decision (if ever made) remains exclusively Phase 2.8's
responsibility.

## Phase 2.9 handoff (Unified Operator Workspace)

This phase's "Content Intelligence" panel is a minimal diagnostic list -
deliberately not the unified cross-domain operator workspace the
authoritative roadmap places at Phase 2.9. `ContentCandidate`'s stable
shape (`id`, `titleOrLabel`, `workingConcept`, `candidateType`,
`contentPotential`, `status`, `evidence`, `provenance`) is what a future
unified workspace would consume; nothing about this phase's data model
needs to change for that to become possible.

## Phase 2.10 handoff (Full Validation)

Nothing here is phase-specific to validate beyond what this document's
"Testing" and "Performance" sections already establish; a future
end-to-end validation pass can exercise `analyze_content_intelligence` /
`accept_content_candidate` / `reject_content_candidate` alongside every
other domain's equivalent commands within one real service session.

## PROVEN

- A structurally distinct `ContentCandidate` type, never folded into
  `IntelligenceFinding`, following the `IntelligenceCorrelation` precedent
  exactly.
- An explicit, closed, 9-variant taxonomy with a documented source
  mapping from real Phase 2.6 Sermon Intelligence findings.
- Deterministic eligibility filtering (status, assertion level, evidence
  presence).
- `content_potential` as a genuinely independent dimension from
  `confidence` - proven by a dedicated divergence/inversion test.
- Quote integrity: a `Quote` candidate can never be built from non-
  verbatim evidence.
- Assertion-level and evidence/provenance inheritance with no
  fabrication anywhere.
- Deterministic O(n) deduplication and a documented, confidence-
  independent ranking formula.
- A working operator review workflow (accept/reject) with no code path
  into presentation, scheduling, or publishing.
- No engine-to-engine calls (type-level: `content_intelligence.rs` has no
  dependency on `bible_adapter`/`music_adapter`/`service_adapter`/
  `sermon_adapter`/`cross_domain`).
- Bounded output regardless of input size (10,000-finding test), and a
  linear, not O(n²), real-numbers performance profile from n=20 through
  n=1,000.
- Fully offline operation - zero new dependencies added by this phase.
- Zero new database migration - candidates are in-memory only, mirroring
  `CorrelationQueue`'s established precedent.

## NOT AVAILABLE / NOT VERIFIED

- Social media publishing, content scheduling, or any external publishing
  path of any kind.
- Full content production or AI-generated copy - `title_or_label`/
  `working_concept` are deterministic, unpolished labels, never marketing
  prose.
- Content candidates from Bible, Music, or Service domain findings -
  mapped only from `Sermon`-domain findings in this initial phase.
- Content candidates from the Sermon summary prefixes not listed in the
  mapping table above (`Definition:`, `Declaration:`, `Prayer Point:`,
  `Summary:`, `Reflection:`, `Transition:`, `Possible Conclusion:`,
  `Structural Transition (section):`, and Sermon Foundation structural
  findings).
- Cross-domain correlation involving content candidates - reserved for
  Phase 2.8 (see "Phase 2.8 handoff" above).
- A unified operator workspace - reserved for Phase 2.9 (see "Phase 2.9
  handoff" above); this phase's panel is a minimal diagnostic list only.
- Live-pipeline auto-dispatch - Content Intelligence remains manual-
  command-only (`analyze_content_intelligence`), mirroring Cross-Domain/
  Sermon Intelligence's own established pattern;
  `pipeline.rs::handle_final_transcript` is untouched.
- Persistence/restart-recovery of in-progress candidates - in-memory
  only, by deliberate design (see "Persistence decision" above).
