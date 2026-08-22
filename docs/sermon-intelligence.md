# Sermon Intelligence (Phase 2.3)

Deterministic, offline structural and meaning detection over a pastor's
live (or manually entered) transcript: sermon theme, main/sub-points,
definitions, key statements, declarations, questions, illustrations/
stories/examples, applications, prayer points, reflections, transitions,
and conclusion signals - continuously updated as the sermon progresses,
never fabricated.

## The core discipline: OBSERVED ≠ INFERRED ≠ SUGGESTED ≠ GENERATED

Phase 2.3 reuses Phase 2.0's `AssertionLevel` exactly (see
[`docs/intelligence-architecture.md`](intelligence-architecture.md)) and
applies it strictly:

- **Observed** - a phrase-anchored structural detection (main point,
  sub-point, definition, key statement, declaration, question,
  illustration/story/example, application, prayer point, summary,
  scripture quotation). The trigger phrase is verbatim transcript text, so
  the *fact the pastor said this* is a direct observation.
- **Inferred** - a theme candidate, a Scripture cross-link, a reflection
  classification, a transition, a conclusion signal. Each is CIP's own
  derived judgment about structure or purpose - never presented as
  something the pastor stated verbatim.
- **Suggested** - not used by the Sermon engine (its findings are either
  direct observations or explicit inferences, never "candidate content
  recommended for approval" the way a Bible verse reference is).
- **Generated** - never produced. Phase 2.3's engine has no code path that
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
IntelligenceContext (reused, Phase 2.0 - unchanged)
    |
    +--> Bible Engine (reused, unchanged)
    +--> Music Engine (reused, unchanged)
    +--> Sermon Engine (new, Phase 2.3)
             |
        core/sermon (pure detection/theme/structure/state - no
        IntelligenceFinding/AssertionLevel coupling)
             |
        core/intelligence::sermon_adapter (translates detections into
        IntelligenceFinding, assigns AssertionLevel/confidence/priority,
        cross-links Scripture via context.active_scripture_context)
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

A closed, 18-variant enum - no open-ended "other" catch-all:

| Kind | Detected by | Assertion level |
| --- | --- | --- |
| `Theme` | `ThemeTracker` evidence accumulation (never per-segment) | Inferred |
| `MainPoint` | explicit phrase ("my first point is...", "the second thing is...") | Observed |
| `SubPoint` | explicit phrase ("under this point...", "firstly...") | Observed |
| `ScriptureReference` | cross-link only, from `context.active_scripture_context` | Inferred |
| `ScriptureQuotation` | literal quotation marks in the transcript | Observed |
| `Definition` | explicit phrase ("by X I mean...", "the meaning of...") | Observed |
| `KeyStatement` | explicit phrase ("the truth is...", "never forget...") | Observed |
| `Declaration` | explicit phrase ("I declare...", "in Jesus' name...") | Observed |
| `Question` | a literal `?` in the segment text | Observed |
| `Illustration` | explicit phrase ("imagine...") | Observed |
| `Story` | explicit phrase ("I remember when...", "there was a man...") | Observed |
| `Example` | explicit phrase ("let me give you an example...", "for instance...") | Observed |
| `Application` | explicit phrase ("you need to...", "we must...") | Observed |
| `PrayerPoint` | explicit phrase ("let's pray...", "pray that...") | Observed |
| `Summary` | explicit phrase ("to summarize...") | Observed |
| `Reflection` | a reflective question shape ("what would...") alongside a `?` | Inferred |
| `Transition` | a real `SermonState` change between segments | Inferred |
| `Conclusion` | explicit phrase ("in conclusion...", "finally...") | Inferred |

**Why Illustration/Story/Example share one detector.** The three are
distinct taxonomy entries (matching spec section 6's "at minimum" list),
but they are all "an illustrative example offered as evidence," so
`core/sermon::detection` runs one set of trigger phrases and picks the
specific kind by which phrase matched, rather than three independent
detectors that would have to agree on overlapping input. This is
documented here, not left implicit, per the "document each category"
instruction.

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
  builds a real `IntelligenceContext`, calls `AppState.sermon_engine`
  directly, queues findings, emits change events.
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
(`SermonElementKind`, `SermonState`, `SermonPoint`, `SermonSubPoint`,
`ThemeCandidate`, `SermonStateSnapshot`) - sermon findings themselves reuse
the existing `IntelligenceFinding` type (`domain: "sermon"`,
`kind: "sermon"`), never a second finding shape. The Live Church Brain's
"Sermon Intelligence" panel shows current state/theme/main point, the full
recorded point/sub-point structure, a manual test-mode transcript entry
box (reusing the exact same `SermonIntelligenceEngine` real input would),
and the pending-findings review list with Accept/Reject - matching the
Music Intelligence panel's established layout and interaction pattern.

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
run - a throwaway test file deleted before commit, matching the Phase
1.5/2.0/2.1/2.2 measurement methodology):

| Operation | Observed |
| --- | --- |
| `cip_core_sermon::detect_elements` (one ~100-character segment, all 44 shapes + question/reflection/quotation checks) | ~5.9µs/call |
| `SermonIntelligenceEngine::analyze` (steady state, one long-running engine, theme window filling up) | ~25.1µs/call |
| `SermonIntelligenceEngine::analyze` (fresh engine per call, no accumulated state) | ~12.5µs/call |

Real numbers from one measurement pass, not an "instant"/"real-time"
claim. Every path here is well under a millisecond even for a segment
that triggers most of the detector shapes at once - far below what a live
service's actual segment-arrival rate would ever require.

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

- `core/sermon`: 36 unit tests across taxonomy, detection (every taxonomy
  category, false positives, determinism), theme (repetition-alone
  rejection, evolution, boundedness, stopword filtering), structure
  (append-only, sub-point hierarchy), and state inference.
- `core/intelligence::sermon_adapter`: 17 tests, including the canonical
  Phase 2.3 acceptance scenario (a ten-segment synthetic sermon asserting
  every taxonomy category fires, plus a mechanical proof that every
  finding's transcript evidence is a verbatim substring of the segment
  that produced it), false-positive tests, context retention/replacement,
  refinement-vs-duplicate, Scripture cross-linking (with and without an
  active context), operator-safety (no `PresentationItem`-capable field),
  determinism, and the 10,000-segment boundary test.
- `core/intelligence::registry`: one additional test proving the real
  `SermonIntelligenceEngine` survives a sibling engine's failure inside
  `analyze_all`.
- `apps/desktop/src-tauri::sermon`: registration, `analyze_and_queue`
  (finding production, dedup), and the accept/reject operator-workflow
  proof.
- Frontend: domain contract tests (`contracts.test.ts`), command-wrapper
  tests (`commands.test.ts`, including the outside-Tauri-runtime guard for
  every new command), and event-subscription tests (`liveEvents.test.ts`).

## PROVEN

- Deterministic sermon structure extraction (main points, sub-points,
  never rewriting earlier history).
- Theme detection requiring both repetition and structural evidence, with
  honest, capped confidence and label evolution.
- Point/sub-point detection from explicit trigger phrases.
- Scripture linkage that never duplicates Bible detection, sourced
  entirely from the existing Bible engine's context.
- Definitions, key statements, declarations (with the future-tense
  exclusion), questions, illustrations/stories/examples, applications,
  prayer points, reflections, transitions, and conclusion signals.
- A working operator review workflow (accept/reject) with no code path
  into presentation.
- Failure isolation alongside Bible/Music in the shared registry.
- Bounded memory/context regardless of service length (10,000-segment
  test).
- Fully offline operation (`serde` + `regex` only).
- Deterministic, reproducible output for identical input.

## NOT AVAILABLE / NOT VERIFIED

- Human-level semantic understanding of a sermon's meaning.
- Perfect theme interpretation - the theme tracker is a bounded
  repetition/structural-evidence heuristic, not comprehension.
- Perfect illustration/story/example recognition - only the phrase-anchored
  trigger set implemented here; a story introduced without any of those
  phrases is not detected.
- Perfect sermon summarization - `Summary`/`Conclusion` are phrase-anchored
  signals, not generated summaries (Phase 2.3 never generates text).
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
