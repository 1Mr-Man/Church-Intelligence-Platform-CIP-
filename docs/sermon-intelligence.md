# Sermon Intelligence (Phase 2.6)

Deterministic, offline structural and meaning detection over a pastor's
live (or manually entered) transcript: sermon theme, main/supporting
points, definitions, key statements, declarations, questions,
illustrations/stories/examples, applications, prayer points, reflections/
food-for-thought prompts, takeaways, transitions, and conclusion signals -
continuously updated as the sermon progresses, never fabricated. Since
Phase 2.6, every finding is additionally aware of the active Phase 2.5
Sermon Foundation (its sermon id, current section, and explicitly
assigned speaker) without ever calling into that layer or any other
engine.

> **Roadmap note.** This repository's authoritative Phase 2 roadmap places
> Sermon Intelligence Foundation at 2.5 and formal Sermon Intelligence at
> 2.6. This document's own original "Phase 2.3" heading was a historical
> label from before that roadmap was adopted; the engine it describes
> (`SermonIntelligenceEngine`, `engine_id: "sermon-core"`) was already the
> real Phase 2.6-equivalent engine, and Phase 2.6's own work extended it
> in place rather than building a second one - see
> [`docs/sermon-foundation.md`](sermon-foundation.md) for the Phase 2.5
> foundation this document's engine now observes (read-only) via
> `IntelligenceContext.active_sermon`/`current_sermon_section`.

## The core discipline: OBSERVED ≠ INFERRED ≠ SUGGESTED ≠ GENERATED

Reuses Phase 2.0's `AssertionLevel` exactly (see
[`docs/intelligence-architecture.md`](intelligence-architecture.md)) and
applies it strictly:

- **Observed** - a phrase-anchored structural detection (main point,
  supporting point, definition, key statement, declaration, question,
  illustration/story/example, application, prayer point, summary,
  takeaway, scripture quotation). The trigger phrase is verbatim
  transcript text, so the *fact the pastor said this* is a direct
  observation.
- **Inferred** - a theme candidate, a Scripture cross-link, a reflection/
  food-for-thought classification, a transition (state-based or Phase 2.5
  section-candidate), a conclusion signal. Each is CIP's own derived
  judgment about structure or purpose - never presented as something the
  pastor stated verbatim.
- **Suggested** - not used by the Sermon engine (its findings are either
  direct observations or explicit inferences, never "candidate content
  recommended for approval" the way a Bible verse reference is).
- **Generated** - never produced. The engine has no code path that
  synthesizes new content; every `summary` traces back to the transcript
  text (or, for a theme, to concept evidence extracted *from* transcript
  text).

Example from the mission statement this phase implements: if the pastor
says "Prayer changes our perspective," CIP may record an **Observed** key
statement with that exact text, and may separately **infer** a candidate
theme of "prayer." It must never record a fabricated statement like
"Pastor said: 'Prayer is the foundation of Christian maturity'" unless
those words were actually said.

## Architecture

```text
LIVE TRANSCRIPT
    |
Phase 2.5 Sermon Foundation (active_sermon/current_sermon_section, read-only)
    |
IntelligenceContext (reused, Phase 2.0/2.5 - unchanged this phase)
    |
    +--> Bible Engine (reused, unchanged)
    +--> Music Engine (reused, unchanged)
    +--> Service Engine (reused, unchanged)
    +--> Sermon Engine (Phase 2.6 - extends the same instance built in
    |        Phase 2.3)
             |
        core/sermon (pure detection/theme/structure/state - no
        IntelligenceFinding/AssertionLevel coupling; Phase 2.6 adds
        Takeaway/FoodForThought detection + a logistics-question filter +
        a state->Phase-2.5-section candidate mapping, still no coupling)
             |
        core/intelligence::sermon_adapter (translates detections into
        IntelligenceFinding, assigns AssertionLevel/confidence/priority,
        cross-links Scripture via context.active_scripture_context,
        attaches sermon_id/section-evidence/speaker-note from
        context.active_sermon/current_sermon_section - Phase 2.6)
             |
        IntelligenceFinding -> FindingQueue -> operator review
        (accept/reject) -> (presentation remains entirely downstream,
        untouched by anything here)
```

`core/sermon` is a pure domain crate exactly like `core/bible`/`core/music`:
no dependency on `core/intelligence`, no dependency on any other domain
crate, no dependency on Tauri/SQLite/a network client. The dependency
direction is one-way (`core/intelligence` depends on `core/sermon`, never
the reverse) - the same architectural rule Phase 2.1/2.2 established for
Music.

`core/intelligence::sermon_adapter::SermonIntelligenceEngine` mirrors
`bible_adapter.rs`'s split exactly: the domain crate answers "what
sermon-shaped things are present in this text," the adapter decides what
any of that *means* epistemically and packages it as `IntelligenceFinding`s.

## The sermon taxonomy (`core/sermon::SermonElementKind`)

A closed, 19-variant enum - no open-ended "other" catch-all. `SubPoint`
below is this taxonomy's own name for what the Phase 2.6 spec calls
"SupportingPoint" - reused rather than duplicated (see "Why no separate
SupportingPoint variant" below).

| Kind | Detected by | Assertion level |
| --- | --- | --- |
| `Theme` | `ThemeTracker` evidence accumulation (never per-segment) | Inferred |
| `MainPoint` | explicit phrase ("my first point is...", "the second thing is...") | Observed |
| `SubPoint` (= spec's "SupportingPoint") | explicit phrase ("under this point...", "firstly...") | Observed |
| `ScriptureReference` | cross-link only, from `context.active_scripture_context` | Inferred |
| `ScriptureQuotation` | literal quotation marks in the transcript | Observed |
| `Definition` | explicit phrase ("by X I mean...", "the meaning of...") | Observed |
| `KeyStatement` | explicit phrase ("the truth is...", "never forget...") | Observed |
| `Declaration` | explicit phrase ("I declare...", "in Jesus' name...") | Observed |
| `Question` | a literal `?` in the segment text, filtered for logistics (Phase 2.6) | Observed |
| `Illustration` | explicit phrase ("imagine...") | Observed |
| `Story` | explicit phrase ("I remember when...", "there was a man...") | Observed |
| `Example` | explicit phrase ("let me give you an example...", "for instance...") | Observed |
| `Application` | explicit phrase ("you need to...", "we must...") | Observed |
| `PrayerPoint` | explicit phrase ("let's pray...", "pray that...") | Observed |
| `Summary` | explicit phrase ("to summarize...") | Observed |
| `Reflection` | a reflective question shape ("what would...") alongside a `?` | Inferred |
| `Transition` | a real `SermonState` change between segments | Inferred |
| `Conclusion` | explicit phrase ("in conclusion...", "finally...") | Inferred |
| `Takeaway` (Phase 2.6) | explicit phrase ("the takeaway is...", "if you remember one thing...") | Observed |
| `FoodForThought` (Phase 2.6) | a broader reflective-prompt shape ("what are you trusting...", "ask yourself...") alongside a `?` | Inferred |

In addition, a **section-transition candidate** (`"Structural Transition
(section): ..."`) is emitted whenever the internal `SermonState` changes
to something with a plausible Phase 2.5 `SermonSectionKind` equivalent
that the foundation's `current_sermon_section` does not already match -
always `Inferred`, and never itself a `SermonElementKind` (it is
read-only structural context, not a new detection over transcript text).

**Why Illustration/Story/Example share one detector.** The three are
distinct taxonomy entries (matching spec section 6's "at minimum" list),
but they are all "an illustrative example offered as evidence," so
`core/sermon::detection` runs one set of trigger phrases and picks the
specific kind by which phrase matched, rather than three independent
detectors that would have to agree on overlapping input. This is
documented here, not left implicit, per the "document each category"
instruction.

**Why no separate `SupportingPoint` variant.** The Phase 2.6 spec's
"SupportingPoint" and this taxonomy's pre-existing `SubPoint` describe the
identical concept - "a point nested under the current main point." Adding
a second variant for the same idea would violate this codebase's
established reuse-before-extend discipline and give operators two
differently-labeled findings for the same structural fact. `SubPoint` is
kept exactly as it already was; only the documentation-level mapping is
new.

**Why `ScriptureReference` is never detected by this crate.** Section
16/17 of the spec is explicit: "do NOT duplicate Scripture detection" -
the Bible Intelligence Engine remains the sole source of Scripture
references and text. `core/sermon` never parses "Romans 8" itself; the
adapter only cross-links a freshly recorded main point to whatever
`IntelligenceContext::active_scripture_context` the Bible engine already
established (see "Scripture integration" below).

## Deterministic-first, no semantic heuristics (a deliberate scope limit)

The spec permits (but does not require) "semantic/structural heuristics"
for main-point detection beyond explicit trigger phrases. Phase 2.3
deliberately implements **only** phrase-anchored detection - every
`SermonDetection` traces back to a specific matched phrase
(`matched_phrase`), never a purely statistical guess. This is a narrower
scope than the spec's optional allowance, chosen because a semantic
heuristic risks exactly the kind of fabricated structural claim the whole
phase exists to prevent. See "NOT AVAILABLE" below.

## Theme detection (`core/sermon::ThemeTracker`)

"Do NOT equate the most frequent word with the sermon theme" (spec
section 14). A theme candidate requires **both**:

1. Repetition across a bounded recent window (default: last 40 segments,
   `DEFAULT_THEME_WINDOW`) meeting `DEFAULT_REPETITION_THRESHOLD` (3).
2. At least one **structural mention** - the concept appearing in a
   segment that also produced a `MainPoint`, `KeyStatement`, `Definition`,
   or `Declaration` (`DEFAULT_STRUCTURAL_THRESHOLD`, 1).

Repetition alone never qualifies - it is supporting evidence only, exactly
as spec section 26 requires. A small, explicit stopword/generic-church-
language list (`IGNORED_WORDS`, a plain `&[&str]`, not a heavyweight NLP
dependency) filters out function words and ubiquitous church vocabulary
("church", "amen", "lord") before counting.

**Theme evolution.** When a second concept's weighted evidence score is
within 60% of the leading concept's, the label combines them (`"faith and
obedience"`) rather than silently dropping the emerging concept - this is
`ThemeCandidate::confidence`'s honest way of expressing "early: Faith /
middle: Faith + Obedience / later: Faith expressed through obedience"
without pretending certainty too early. Confidence is computed from
`0.5 + 0.05 * repetition_count + 0.1 * structural_mentions`, capped at
0.95 (never 1.0 - a theme is always an inference).

## Main/sub-point detection and structure (`core/sermon::SermonStructure`)

Append-only: `record_main_point`/`record_sub_point` never remove or edit a
previously recorded point. A second main point becomes the new "current"
point; the first remains fully reachable in `SermonStructure::points()` -
"must not rewrite history silently" (spec section 11), proven by the
`context_replacement_keeps_point_one_historical_and_never_rewrites_it`
test. A sub-point with no main point recorded yet is dropped rather than
inventing a parent ("avoid inventing hierarchy when evidence is weak,"
spec section 13).

## Scripture integration (never duplicated)

`SermonIntelligenceEngine` reads `IntelligenceContext.active_scripture_context`
(populated entirely by the unchanged Bible engine) and, only when a main
point is freshly recorded in the same `analyze()` call, emits one
additional `Inferred` finding cross-linking the point to that context
(`"Supporting Scripture: ROM 10:17"`). No Bible text or reference parsing
happens in `core/sermon` at all - the actual reference stays owned by the
Bible engine, exactly matching the worked example in spec section 47
(Main Point "Faith grows through hearing" ↔ Scripture "Romans 10:17").

`ScriptureQuotation` is even more conservative: it only fires on literal
quotation marks (`"`/`\u{201c}`/`\u{201d}`) already present in the
transcript text, extracting the quoted span verbatim. Real spoken
transcripts rarely contain punctuation-level quotation marks, so this
detector is expected to fire rarely in practice - a deliberate trade-off
against ever inventing a quotation boundary (spec section 17).

## Definitions, key statements, declarations, questions

Each is a small, explicit regex-anchored phrase set
(`core/sermon::detection::SHAPES`), mirroring `core/bible::detection`'s
established `regex` + `LazyLock` shape-array pattern. Declarations are
deliberately narrow: bare future-tense sentences ("You will...", "God
will...") are **never** classified as a declaration on their own (spec
section 20's explicit warning) - only "I declare...", "receive
it/this/your...", and "in Jesus' name..." qualify. Questions are the most
reliable detector in the module: any literal `?` in the segment text.

## Illustrations, stories, examples, applications, prayer points

Each is phrase-anchored (see the taxonomy table above). A reflective
question ("What would change in your life if you truly believed this?")
produces *both* a `Question` finding (Observed - a question was literally
asked) and a separate `Reflection` finding (Inferred - the classification
of its purpose), matching spec section 25's requirement that the
reflection label "remain clearly marked as an inference... not as
something the pastor explicitly called 'food for thought.'"

## Transitions and conclusion

A `Transition` finding is emitted only when the lightweight derived
`SermonState` (see below) actually changes between two consecutive
`analyze()` calls - "a transition is a finding only when evidence is
sufficient" (spec section 27), not a per-segment noise source. A
`Conclusion` finding is a phrase-anchored signal ("in conclusion...",
"finally...") and is always `Inferred`: it never terminates sermon
processing or prevents a pastor from returning to another point (spec
section 28) - there is no "sermon ended" state anywhere in this
architecture.

## Takeaways and Food for Thought (Phase 2.6)

**Takeaway** is phrase-anchored, `Observed`, and deliberately distinct
from `Summary` (a recap of what was said) and `KeyStatement` (a quotable
assertion): a takeaway specifically frames *what the pastor wants the
listener to carry away* ("the takeaway is...", "if you remember one
thing today...", "what I want you to remember...", "the bottom line
is..."). Never rewritten into polished prose - the summary is always the
verbatim segment text, per the same "no fabricated statement" rule every
other Observed kind follows.

**FoodForThought** is `Inferred`, conservative by construction, and kept
distinct from the pre-existing `Reflection` kind (whose narrower "what
would.../how would you..." shape still fires on its own) rather than
merged into it - the two together give an honest range from "clearly a
reflective classroom-style question" (`Reflection`) to "a broader
invitation to personal examination" (`FoodForThought`: "what are you
trusting...", "ask yourself...", "are you willing...", "what would it
look like if..."). Both require a literal `?` in the text; neither ever
fires on a bare imperative sentence with no question mark, and a
purely logistical question ("Can everyone hear me?", "Are you ready?",
"What page are we on?") produces **no** finding at all of any kind - see
"Logistics-question false-positive filter" below.

## Section-aware and speaker-aware attribution (Phase 2.6)

The Sermon engine now reads the Phase 2.5 Sermon Foundation's context
(`IntelligenceContext.active_sermon`/`current_sermon_section`) - never
mutates it, never lets it change *which* findings are detected (Phase 2.6
spec: "Section context must never force a finding," proven by
`section_context_never_changes_which_findings_are_detected_only_their_evidence`,
which runs the identical transcript segment through two different section
contexts and asserts the produced findings are identical apart from the
section-transition-candidate finding itself):

- **`sermon_id`** - every finding produced while a `Sermon` is active
  carries that sermon's id (`IntelligenceFinding.sermon_id`, a new,
  additive `Option<Uuid>` field - `None` for every other domain and for
  Sermon findings produced with no active sermon). Never guessed; taken
  directly from `context.active_sermon.id`.
- **Section evidence** - when a `current_sermon_section` is set, every
  finding gains one extra `EvidenceSource::Context` entry naming that
  section (`"sermon_section:MAIN_MESSAGE"`) - purely associative context
  for an operator reviewing the finding, never a reason to suppress or
  reclassify anything.
- **Speaker attribution** - when the active sermon has an explicitly
  assigned `Speaker` (Phase 2.5's `assign_sermon_speaker`, never
  biometric/inferred), every finding's `provenance.note` records
  `"speaker: <name> (<ROLE>)"`. Absent when no speaker was explicitly
  assigned - never a placeholder like "Unknown Speaker."

## Structural transition candidates (Phase 2.6)

Extends the pre-existing `Transition` finding (an internal `SermonState`
change) with a second, additional finding that proposes a candidate Phase
2.5 `SermonSectionKind` for the new state, via
`cip_core_sermon::candidate_section_for_state` - a small, pure,
partial mapping (`Introduction`→`Introduction`, `Teaching`/`MainPoint`→
`MainMessage`, `Illustration`→`Illustration`, `Conclusion`→`Conclusion`,
`Prayer`→`Prayer`; `Application`/`Unknown` map to `None` rather than
guessing a section with no honest equivalent). The finding
(`"Structural Transition (section): INTRODUCTION -> MAIN_MESSAGE"`) is
emitted only when the foundation's own `current_sermon_section` does not
already match the candidate, so an operator who is already tracking
sections accurately sees no redundant noise. This is a **read-only
recommendation** - nothing in `core/intelligence` ever writes back to
`SermonSection`/persisted section state; that remains the Sermon
Foundation/operator's exclusive responsibility (spec section 14's own
requirement), proven by `no_candidate_section_finding_when_the_foundation_already_agrees`
and `a_story_detection_proposes_a_candidate_illustration_section_when_the_foundation_disagrees`.

## Logistics-question false-positive filter (Phase 2.6)

A genuine gap found and fixed during the Phase 2.6 audit: the original
Phase 2.3 `Question` detector fired on any literal `?`, including purely
operational questions ("Can everyone hear me?", "Are you ready?", "What
page are we on?") that are not sermon content at all. `detect_elements`
now checks a small `LOGISTICS_QUESTION_PATTERN` first and, when it
matches, produces **no** finding whatsoever for that segment (not merely
a suppressed `Question` classification) - proven by
`logistics_questions_are_never_a_sermon_finding` (`core/sermon`) and
`logistics_questions_never_produce_a_sermon_finding_through_the_full_engine`
(`core/intelligence`). A genuine teaching question ("Do you believe this
today?") is unaffected.

## Sermon state (`core/sermon::SermonState`)

A lightweight classification (`Introduction`/`Teaching`/`MainPoint`/
`Illustration`/`Application`/`Conclusion`/`Prayer`/`Unknown`), re-derived
fresh on every call from the most recently detected kinds - never a
persistent state machine, never an illegal-transition guard. The pastor
can move from `Application` back to `Teaching` and back to `MainPoint`
freely; nothing here prevents it (spec section 29).

## Engine identity and contract

`SermonIntelligenceEngine` implements the standard `IntelligenceEngine`
trait exactly like Bible/Music - `identity()` reports
`domain: Sermon, engine_id: "sermon-core", engine_version: "0.1.0"`,
`capability()` is always `Available` (no external dependency, unlike
Phase 2.2's acoustic engine), and `analyze()` receives an ordinary
`TranscriptSegment` via `IntelligenceInput` - no special-cased inherent
method the way `MusicIntelligenceEngine::analyze_acoustic` needed for raw
audio. It is registrable directly via
`IntelligenceEngineRegistry::register()`/`resolve()`.

## Deduplication and refinement (reused, not reimplemented)

Phase 2.3 adds no new deduplication logic. `IntelligenceFinding::is_equivalent_to`
(same `service_id`+`domain`+`kind`+`summary`) already gives the exact
behavior spec section 36 asks for: an identical repeated segment ("Faith"
said three times) produces the same summary each time and is discarded by
`FindingQueue::add` after the first; a genuinely different summary (Theme
"Faith" → "Faith and Obedience") is never equivalent, so it is correctly
treated as a refinement, not a duplicate.

## Operator workflow (Tauri commands)

Five commands - deliberately the minimum spec section 39 suggests, with
`refine_sermon_finding` omitted (see "Why no refine/correction command"
below):

- `analyze_sermon_transcript(text)` - the manual/test-mode harness,
  mirroring `analyze_music_transcript` exactly. Persists the segment,
  builds a real `IntelligenceContext` (via the shared `build_music_context`
  helper - generic despite its name, per its own docs - so it now
  additively attaches the Phase 2.5 Sermon Foundation's
  active_sermon/current_sermon_section/recent_sermon_segments, Phase
  2.6), calls `AppState.sermon_engine` directly, queues findings, emits
  change events.
- `list_sermon_findings()` - pending findings for the active service.
- `accept_sermon_finding(findingId)` / `reject_sermon_finding(findingId)` -
  ordinary `FindingQueue` status changes. Neither has any code path into
  `cip_core_presentation` - `sermon.rs` does not depend on that crate at
  all.
- `get_sermon_state()` - the read-only `SermonStateSnapshot`
  (state/theme/points), safe to poll at any time.

**Why no `refine_sermon_finding` correction command.** Spec section 39
says "reuse generic intelligence commands if the existing architecture
already supports them; do not create unnecessary duplicated commands."
Rejecting a mis-detected theme/point is itself the explicit, auditable
correction path (`SERMON_FINDING_REJECTED` in the timeline) - it never
rewrites transcript history, and the very next segment's detection
naturally supersedes it. Adding a second, bespoke "correct this finding"
command would duplicate what reject already provides.

**Two engine instances, on purpose.** `AppState.sermon_engine` (what every
command above actually uses) is a separate instance from the one
registered into `intelligence_registry` during `setup()`. This exactly
mirrors Phase 2.2's `AppState.acoustic_music_engine` vs. the registry's own
Music registration: the registry's copy exists only for
`get_intelligence_capabilities`/`IntelligenceEngineRegistry::analyze_all`
diagnostic/failure-isolation symmetry with Bible/Music; nothing in this
app ever calls `intelligence_registry.resolve(Sermon)` from a live
command, because a trait object cannot be downcast back to call
`SermonIntelligenceEngine`'s own inherent `snapshot()` method.

**Never wired into the live pipeline.** `pipeline.rs::handle_final_transcript`
is completely unchanged (architectural rule #1: "do not rewrite Phase 1").
Sermon Intelligence is manual-command-only, mirroring Music's Phase 2.1
lyric path exactly - the same manual test-mode entry point real audio
would eventually use.

## Events

Six new `AppEvent` variants, added only where operator-observable
(spec section 42's "exact naming should follow existing project
conventions"):

- `SermonFindingDetected` / `SermonFindingAccepted` / `SermonFindingRejected`
  - mirror the Music finding events exactly.
- `SermonStructureUpdated` (payload: the current `SermonPoint[]`),
  `SermonThemeChanged` (payload: `ThemeCandidate | null`),
  `SermonStateChanged` (payload: the new `SermonState`) - emitted only when
  `analyze_sermon_transcript` observes the snapshot actually change,
  never on every segment regardless of whether anything moved.

**Timeline discipline.** Per spec section 41 ("do NOT record every
automatic detection as a timeline event"), only `SermonFindingAccepted`/
`SermonFindingRejected` write an `audit_events` row. `SermonFindingDetected`
and the three structure/theme/state-change events are live UI signals
only - deliberately not persisted, since a real sermon can produce dozens
of detections per minute and the timeline would otherwise drown in
automatic noise.

## Database: no new migration

Sermon findings are in-memory only (`AppState.intelligence_findings`,
Phase 2.0's existing `FindingQueue`), exactly like Bible/Music findings -
"the intelligence architecture already intentionally supports in-memory
findings" (spec section 64). `SermonIntelligenceEngine`'s own theme/
structure/state accumulate in the engine instance itself, not a database
table. No `sermon_findings`/`sermon_points`/`sermon_themes` table was
added, and none is needed: nothing in Phase 2.3 requires restart recovery
of in-progress sermon structure, and a completed service's transcript
(already persisted) is sufficient to re-derive everything by replaying it
through `analyze_sermon_transcript` if ever needed.

## Frontend

`domain/sermon.ts` mirrors `cip_core_sermon`/`sermon_adapter`'s types
(`SermonElementKind` - now including `takeaway`/`food_for_thought`,
`SermonState`, `SermonPoint`, `SermonSubPoint`, `ThemeCandidate`,
`SermonStateSnapshot`) - sermon findings themselves reuse the existing
`IntelligenceFinding` type (`domain: "sermon"`, `kind: "sermon"`, now
carrying `sermonId: string | null`), never a second finding shape. The
Live Church Brain's "Sermon Intelligence" panel shows current state/theme/
main point, the full recorded point/sub-point structure, a manual
test-mode transcript entry box (reusing the exact same
`SermonIntelligenceEngine` real input would), and the pending-findings
review list with Accept/Reject; each finding card now also shows a short
category label (`sermonFindingCategory`, a pure display helper derived
from the summary prefix - e.g. "Takeaway", "Food for Thought", "Scripture")
alongside its OBSERVED/INFERRED marker, matching the Music Intelligence
panel's established layout and interaction pattern.

## Failure isolation

Reuses `IntelligenceEngineRegistry::analyze_all`'s existing panic-catching
isolation (`catch_unwind`) - no Sermon-specific code was needed. Proven in
`core/intelligence/src/registry.rs`'s
`the_real_sermon_engine_registers_and_analyzes_alongside_bible_and_a_failing_music_engine`
test: the real `SermonIntelligenceEngine`, a real Bible engine, and a
deliberately failing Music engine are registered together, and Bible/
Sermon results remain unaffected by Music's failure.

## Determinism and boundary tests

`identical_input_sequences_produce_equivalent_finding_sequences` runs the
same three-segment transcript through two fresh engine instances and
asserts identical `(domain, kind, assertion_level, summary)` sequences
(ids/timestamps excluded, as expected). `ten_thousand_segments_never_exhaust_memory_or_break_analysis`
feeds 10,000 synthetic segments through one engine and confirms it remains
operational and still produces a correct finding afterward - `ThemeTracker`'s
window (default 40) and `IntelligenceContext`'s existing bounds
(`DEFAULT_MAX_RECENT_TRANSCRIPT_SEGMENTS`) both keep memory bounded
regardless of service length.

## Performance

Measured directly (`std::time::Instant`, release build, this machine, one
run per measurement - a throwaway test module deleted before commit,
matching the established measurement methodology used every phase):

| Operation | Observed |
| --- | --- |
| `cip_core_sermon::detect_elements` (one ~100-character segment, all 49 shapes + question/reflection/food-for-thought/quotation checks + the logistics filter) | ~5.9µs/call |
| `SermonIntelligenceEngine::analyze` (steady state, one long-running engine, theme window filling up) | ~25.1µs/call |
| `SermonIntelligenceEngine::analyze` (fresh engine per call, no accumulated state) | ~12.5µs/call |
| `SermonIntelligenceEngine::analyze`, Phase 2.6 (with a real `Sermon`+`SermonSection` attached via `with_sermon_context` every call - n=100, steady state) | ~16.7µs/call |
| `SermonIntelligenceEngine::analyze`, Phase 2.6 (same, n=1000) | ~17.1µs/call |

Real numbers from one measurement pass, not an "instant"/"real-time"
claim. `n=100` and `n=1000` costing effectively the same per-call time
(~17µs either way) is the concrete evidence against an O(n²) regression
from the new `attach_sermon_foundation_context`/section-candidate logic -
still well under a millisecond even for a segment that triggers most of
the detector shapes at once, far below what a live service's actual
segment-arrival rate would ever require.

## Offline guarantee

`core/sermon`'s only dependencies are `serde` and `regex` (verified via
`cargo tree -p cip-core-sermon`) - no `reqwest`, no `hyper`, no cloud SDK,
no network client anywhere in its tree. `core/intelligence`'s normal
dependency tree likewise carries no network-related crate. Phase 2.3
requires no model download, no API key, and no network access of any
kind - see `docs/intelligence-architecture.md#offline-operation` for the
architecture-wide guarantee this phase inherits unchanged.

## Privacy

No sermon transcript is ever uploaded, and nothing here persists a full
sermon transcript beyond what the existing transcript pipeline already
persists (ordinary `transcript_segments` rows, unchanged since Phase 1.2).
No analytics or telemetry was added. All processing is local, in-process,
and synchronous.

## Future semantic engine boundary (not implemented)

Nothing in Phase 2.3 requires an LLM or embedding model. The boundary for
a future `LocalSemanticSermonEngine`/`ExternalSemanticSermonEngine` is
simply: implement `IntelligenceEngine` for the Sermon domain (as
`SermonIntelligenceEngine` already does) and register it instead of, or
alongside, the deterministic one. Nothing about `IntelligenceContext`,
`IntelligenceFinding`, or the registry contract needs to change for that
to be possible later - this phase deliberately does not implement it now.

## Copyright & content safety

Every example transcript in this phase's tests and this document is a
short, project-authored synthetic passage ("My first point is that faith
comes by hearing," "There was a man who planted a seed and waited
patiently") - never copyrighted sermon or book content, and never a large
corpus.

## Testing

- `core/sermon`: 75 unit tests across taxonomy, detection (every taxonomy
  category including Takeaway/FoodForThought, false positives including
  the logistics-question filter, determinism), theme (repetition-alone
  rejection, evolution, boundedness, stopword filtering), structure
  (append-only, sub-point hierarchy), state inference (including the new
  `candidate_section_for_state` mapping), and the pre-existing `foundation`
  submodule (Phase 2.5, untouched).
- `core/intelligence::sermon_adapter`: 28 tests, including both the
  original Phase 2.3-labeled acceptance scenario and a new Phase 2.6
  canonical acceptance scenario (a nine-segment synthetic sermon with a
  real `Sermon`/`SermonSection`/active Scripture context attached
  throughout, asserting Theme/MainPoint/Illustration/Scripture/Question/
  Application/KeyStatement/FoodForThought/Takeaway all fire, every finding
  carries the correct `sermon_id`, nothing is ever `Generated`, every
  evidence excerpt is verbatim, and the sequence is deterministic across
  re-runs), plus dedicated tests for sermon-id association, section-aware
  determinism, speaker-attribution (present/absent), the section-transition
  candidate (proposed and suppressed cases), Takeaway/FoodForThought
  assertion levels, the full-engine logistics-question false positive, and
  100-repeat dedup stability.
- `core/intelligence::registry`: one test proving the real
  `SermonIntelligenceEngine` survives a sibling engine's failure inside
  `analyze_all` (unchanged this phase).
- `apps/desktop/src-tauri::sermon`: registration, `analyze_and_queue`
  (finding production, dedup), the accept/reject operator-workflow proof,
  and a new Phase 2.6 integration test (`analyze_and_queue_carries_sermon_context_and_never_creates_a_presentation_item`)
  using a real in-memory SQLite database - persists a `ServiceSession` and
  `Sermon`/`SermonSection`, runs a full `analyze_and_queue` call with the
  foundation context attached, accepts the resulting finding, and asserts
  `persistence::list_presentation_items` is empty both before and after.
- `core/intelligence::finding`: a new test proving `sermon_id` starts
  `None` and is only ever set via the explicit `with_sermon_id` builder.
- Frontend: domain contract tests (`contracts.test.ts`, including the new
  `sermonId` field on every `IntelligenceFinding` literal), command-wrapper
  tests (`commands.test.ts`, including the outside-Tauri-runtime guard for
  every command), and event-subscription tests (`liveEvents.test.ts`).

## Cross-domain correlation (Phase 2.4/2.8)

Sermon findings - especially the `"Supporting Scripture: ..."` cross-link
and theme candidates - are the primary source the historical Phase 2.4
correlation engine reads to connect a sermon point back to a Bible
finding, or to a Music finding recognized nearby; the authoritative
roadmap now understands correlation itself as Phase 2.8 work. `core/sermon`
and `sermon_adapter.rs` are unchanged by any of this - the correlation
engine only reads already-produced `IntelligenceFinding`s from
`IntelligenceContext`, never calls into `SermonIntelligenceEngine`, and
this phase does not implement any new correlation rules (spec's explicit
"do not implement correlation rules in Phase 2.6"). See
[`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md).

## Phase 2.7 handoff (Content Intelligence) - fulfilled

Phase 2.7 (see [`docs/content-intelligence.md`](content-intelligence.md))
now transforms eligible Sermon Intelligence findings into
`ContentCandidate`s (e.g. a `Quote` candidate from an accepted
`KeyStatement`), reading the exact contract this section promised:

- Every finding is an ordinary `IntelligenceFinding` (`domain: Sermon`,
  `kind: Sermon`) with a stable `summary`, `assertionLevel`, `confidence`,
  `evidence`, `provenance`, `transcriptSegmentIds`, `sermonId`,
  `engineId`/`engineVersion`, and `createdAt` - nothing Phase 2.7-specific
  needs to be added to read every field it will need.
- `FindingStatus::Accepted` is the operator-review signal a content
  pipeline should gate on - never `Detected`/`Reviewed` (still pending
  human judgment).
- No content (a social post, a devotional, a quote image) is produced
  anywhere in this phase - proven by the same type-level argument used
  throughout this document (`core/intelligence`/`sermon.rs` have no
  dependency on `cip_core_presentation`, and nothing here imports or
  references any content-generation type).

## Phase 2.8 handoff (Cross-Domain Intelligence)

Every Sermon finding now carries everything a future correlation rule
needs to relate it to Bible/Music/Service findings without re-deriving
anything: `domain`, `kind`, `serviceId`, `sermonId` (new, Phase 2.6),
`evidence`, `confidence`, `assertionLevel`, `createdAt`, and
`engineId`/`engineVersion`. No correlation rule is implemented here - that
remains exclusively Phase 2.8's responsibility, per the spec's own
instruction and this repository's `cross_domain.rs`, which is untouched by
this phase.

## PROVEN

- Deterministic sermon structure extraction (main points, sub-points,
  never rewriting earlier history).
- Theme detection requiring both repetition and structural evidence, with
  honest, capped confidence and label evolution.
- Point/sub-point detection from explicit trigger phrases.
- Scripture linkage that never duplicates Bible detection, sourced
  entirely from the existing Bible engine's context.
- Definitions, key statements, declarations (with the future-tense
  exclusion), questions (filtered for logistics), illustrations/stories/
  examples, applications, prayer points, reflections, food-for-thought
  prompts, takeaways, transitions, and conclusion signals.
- Section-aware and speaker-aware attribution (`sermonId`, section
  evidence, speaker provenance note) sourced entirely from the Phase 2.5
  Sermon Foundation's read-only context, never forcing a detection.
- A read-only candidate-section suggestion alongside internal state
  transitions, never mutating persisted `SermonSection` state.
- A working operator review workflow (accept/reject) with no code path
  into presentation - proven with a real in-memory database in Phase 2.6.
- Failure isolation alongside Bible/Music/Service in the shared registry.
- No engine-to-engine calls (type-level: this module has no dependency on
  `bible_adapter`/`music_adapter`/`service_adapter`).
- Bounded memory/context regardless of service length (10,000-segment
  test), and no O(n²) regression from the Phase 2.6 additions (n=100 and
  n=1000 cost the same ~17µs/call).
- Fully offline operation (`serde`/`regex`/`chrono`/`uuid` only, verified
  via `cargo tree`).
- Deterministic, reproducible output for identical input, including with
  a real Sermon Foundation context attached.

## NOT AVAILABLE / NOT VERIFIED

- Human-level semantic understanding of a sermon's meaning.
- Perfect theme interpretation - the theme tracker is a bounded
  repetition/structural-evidence heuristic, not comprehension.
- Perfect illustration/story/example recognition - only the phrase-anchored
  trigger set implemented here; a story introduced without any of those
  phrases is not detected.
- Perfect sermon summarization - `Summary`/`Conclusion` are phrase-anchored
  signals, not generated summaries (this engine never generates text).
- LLM-quality semantic reasoning of any kind - explicitly out of scope for
  this deterministic-first phase (see "Future semantic engine boundary").
- Real-world accuracy across all preaching styles, languages, or
  delivery patterns - the detector set was designed and tested against
  English, common Western-evangelical structural phrasing; a differently-
  structured sermon (a different homiletic tradition, a different
  language) will produce fewer or no structural detections, honestly, not
  a wrong guess.
- Semantic/statistical main-point detection beyond explicit trigger
  phrases - deliberately not implemented (see "Deterministic-first"
  above), to avoid ever fabricating a structural claim.
- Automatic doctrinal/theological interpretation of any finding.
- Biometric speaker identification - speaker attribution is explicit-only,
  from Phase 2.5's operator-assigned `Speaker`, never inferred from voice.
- Multilingual intelligence beyond the implemented English rule set.
- Polished AI-generated sermon summaries or automatic social-media
  content - reserved for Phase 2.7 (see "Phase 2.7 handoff" above).
- Cross-domain correlation rules - reserved for Phase 2.8 (see "Phase 2.8
  handoff" above); this phase only ensures findings carry what Phase 2.8
  will need.
- Live-pipeline auto-dispatch - Sermon Intelligence remains manual-command-
  only (`analyze_sermon_transcript`), exactly as it was before Phase 2.6;
  `pipeline.rs::handle_final_transcript` is untouched.
