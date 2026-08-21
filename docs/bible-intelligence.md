# Bible Intelligence Core (Phase 1.1)

This document explains the Bible Intelligence Core added in Phase 1.1: the
pipeline that turns transcript text into scripture suggestions. Phase 1.0's
[`docs/architecture.md`](architecture.md) still describes the overall
system; this is the deep dive on one subsystem.

**Not in this phase:** real speech recognition, the full presentation
designer, sermon/song intelligence, and online lookups. This subsystem
consumes plain text and produces suggestions - nothing more.

## The pipeline

```
TRANSCRIPT TEXT
    v
TEXT NORMALIZATION        (cip-core-bible::normalize)
    v
SCRIPTURE REFERENCE DETECTION   (cip-core-bible::detection)
    v
SCRIPTURE CONTEXT MANAGEMENT    (cip-core-bible::DefaultScriptureContextManager)
    v
REFERENCE RESOLUTION      (against the active context)
    v
BIBLE VALIDATION          (cip-core-bible::BibleProvider)
    v
CONFIDENCE                (cip-core-confidence::ConfidenceResult)
    v
SCRIPTURE SUGGESTION      (cip-core-ai::Suggestion, always Pending)
```

`cip_core_service::process_transcript_segment` runs one transcript segment
through every stage and returns a `ProcessedSegment { detections,
suggestions }`. It is pure Rust with no I/O beyond the `BibleProvider` it's
given - no audio, no Tauri, no network.

```rust
let mut context = DefaultScriptureContextManager::new("KJV");
let result = process_transcript_segment(
    service_id,
    "Turn with me to Romans chapter 8.",
    "KJV",
    &bible_provider,
    &mut context,
);
// result.detections[0].kind == ReferenceKind::Chapter
// result.suggestions.is_empty() - no verse to suggest yet
```

### Why it lives in `core/service`, not `core/bible`

`core/bible` owns normalization, detection, and the Scripture Context
Manager - all pure Bible-domain logic with no dependency on `core/ai`.
`core/ai` owns `Suggestion`. Per the architecture's boundary rules, domain
crates don't depend on each other directly except through `core/service`,
which is the documented composition point. Producing a `Suggestion` from a
detected reference is exactly that kind of composition, so the
orchestrator (`bible_intelligence.rs`) lives there rather than creating a
`bible` <-> `ai` dependency in either direction.

## Text normalization

`cip_core_bible::normalize::normalize_text` converts spoken number words to
digits so the detector only ever has to match digit patterns:

```
"Romans chapter eight verse twenty-eight"  ->  "Romans chapter 8 verse 28"
"Romans eight twenty-eight"                ->  "Romans 8 28"
"Romans 8:28 says..."                      ->  "Romans 8:28 says..." (unchanged)
```

Whitespace-separated number words are normalized independently, and only a
hyphenated compound (`"twenty-eight"`) is treated as one number - this is
what keeps `"Romans eight twenty-eight"` (chapter 8, verse 28) from being
misread as a single run-on number. See the module docs in
`core/bible/src/normalize.rs` for the full rationale.

## Book name normalization

Every book name/abbreviation CIP recognizes is defined exactly once, in
`core/bible/src/book_alias.rs`'s `BOOKS` table - nothing else hard-codes a
book name. `canonicalize_book("Rom.")`, `canonicalize_book("rom")`, and
`canonicalize_book("Romans")` all resolve to the same canonical entry
(`code: "ROM"`, `name: "Romans"`). Extending recognized aliases means
adding to that one table.

## Reference detection

`cip_core_bible::detection::detect_candidates` is pure syntax over
already-normalized text: it finds `Book`, `Book chapter`, and
`Book chapter:verse` patterns (colon, "chapter X verse Y", "X verse Y", and
bare "X Y" forms) plus bare `verse N` fragments, in the order they appear,
and classifies each as `Direct`, `Chapter`, or `Verse`
(`cip_core_bible::ReferenceKind`). It does not know whether any of it is
real Bible content - that's a later stage - and multiple references in one
segment (`"Compare Romans 8:28 with John 3:16."`) are all detected
independently, in order.

## Scripture Context Manager

`DefaultScriptureContextManager` (`core/bible/src/context_manager.rs`)
implements the `ScriptureContextManager` interface boundary established in
Phase 1.0, modeling how pastors actually speak:

```
"Turn with me to Romans chapter 8."   -> ACTIVE CONTEXT: Romans 8
...several unrelated sentences...      -> (context untouched)
"Look at verse 28."                    -> resolves to Romans 8:28
"Now verse 31."                        -> resolves to Romans 8:31
"Go back to verse 18."                 -> resolves to Romans 8:18
"Now let's go to John chapter 3."      -> ACTIVE CONTEXT: John 3 (Romans 8 replaced)
"Verse 16."                            -> resolves to John 3:16
```

### The context model

`ScriptureContext` holds exactly the fields the context needs to represent
a chapter that may or may not have a verse resolved yet:

| Field            | Meaning                                                             |
| ------------------ | ---------------------------------------------------------------------- |
| `book`, `chapter`  | The active book/chapter.                                             |
| `last_verse`       | The most recently resolved verse in this context, or `None` - a bare chapter reference never invents one. |
| `confidence`       | How sure CIP is this context is right (raised to `Human`/1.0 by `confirm_active()`). |
| `established_at`   | When this context became active.                                    |
| `valid`            | Whether the book+chapter has been confirmed against a `BibleProvider` (see below). |

`recent_references()` (bounded, default 20, configurable via
`DefaultScriptureContextManager::with_history_capacity`) gives the recent
reference history, independent of the single active context - useful for
diagnostics and the ambiguity heuristic below.

### Validation happens outside the manager

The Scripture Context Manager itself never touches a `BibleProvider` - it
tracks *what was said*, not *whether it's real*. The orchestrator validates
a book+chapter *before* ever calling `resolve()` with it (so an invalid
chapter like `"Romans 999"` never becomes the active context at all), and
validates a bare-verse candidate before treating it as final (so an invalid
verse like `"Romans 8:999"` is reported `Unresolved` without ever updating
`last_verse`). This is why `ScriptureContext::valid` is always `true` in
practice for Phase 1.1 - every context that exists has already passed
validation by construction.

### Context replacement

A new explicit book+chapter reference always replaces the active context
- `"Now let's go to John chapter 3"` after Romans 8 was active makes John 3
the active context; a subsequent bare `"verse 16"` resolves against John 3,
not Romans 8.

### Partial reference resolution & sequential references

A bare `"verse N"` fragment resolves against the active context. The
*first* verse pulled from a context is classified `Verse`; any further bare
verse resolved while that same context is still active - regardless of
direction, "verse 31" and "go back to verse 18" are both continuations -
is classified `Sequential`. Both require Bible validation before they're
final; neither is produced without an active context (`"verse 28"` with no
context ever established is `Unresolved`, never a guess).

### Ambiguity

Immediately after a context replacement, the manager keeps the
just-replaced context as a one-shot "shadow." If the very next bare verse
fragment is plausible against *both* the new active context and the shadow
(both validate against the `BibleProvider`), that's reported as
`ReferenceKind::Ambiguous` with both candidates and their confidence - the
current context scores higher (it's what's actively being taught from) than
the shadow (recency only). CIP never silently picks one:

```
"John 3" -> "Romans 8" -> "verse 16"
  with a translation where both John 3:16 and Romans 8:16 are real verses
  -> AMBIGUOUS_REFERENCE
     candidates: Romans 8:16 (0.71), John 3:16 (0.64)
```

The shadow is consumed (cleared) after this one check, whether or not it
turned out ambiguous, so it never lingers - the *next* bare verse after
that resolves unambiguously against whatever is still active. An operator
resolving the UI's eventual disambiguation prompt is future work; Phase 1.1
only establishes the domain result and its tests
(`core/service/src/bible_intelligence.rs`'s
`genuinely_ambiguous_bare_verse_produces_candidates_not_a_guess`).

## Reference types

`ReferenceKind` (`core/bible/src/detection.rs`) is the single enum used
throughout the pipeline: `Direct`, `Chapter`, `Verse`, `Sequential`,
`Ambiguous`, `Unresolved`. `detect_candidates` only ever produces the first
three (pure syntax); the orchestrator promotes/reclassifies as it resolves
against context and validates against the Bible - see the type's doc
comments for exactly which stage assigns which variant, and `label()` for
the `SCREAMING_SNAKE_CASE` form used in logs (matching the convention
`AppEvent::name()` already established for event names).

## Confidence

Every detection carries a `ConfidenceResult` (`cip-core-confidence`,
unchanged from Phase 1.0) reflecting the evidence behind it: an explicit
`Direct` reference (book+chapter+verse all stated, validated) scores
highest; a `Chapter`-only reference next; a bare `Verse`/`Sequential`
resolved against context scores a bit lower (it depends on context being
right, not just the words spoken); ambiguous candidates carry their own,
lower, per-candidate confidence. See
`core/service/src/bible_intelligence.rs`'s `confidence_for_kind`. **A high
confidence score is never permission to project** - see below.

## Suggestions

A `Suggestion` (`cip-core-ai`, now `service`-scoped - see below) is created
only for a detection that resolved to a concrete, *validated* verse
(`Direct`, `Verse`, `Sequential`). A bare chapter never produces one (no
verse to suggest), and neither does an ambiguous or unresolved detection
(never guess). Every suggestion starts `Pending`; nothing in the Bible
Intelligence Core - regardless of confidence - moves it to `Approved` or
creates a `PresentationItem`. That transition is a human action elsewhere
in the system, by design.

### `Suggestion` gained a `service_id`

Phase 1.0's `Suggestion` had no service linkage, even though
`PresentationItem` did and the `ai_suggestions.service_id` database column
already existed. Phase 1.1 is the first code that actually constructs a
`Suggestion` from a live transcript segment (`processTranscriptSegment`
takes a `serviceId`, per the required API), so it adds the field the schema
and `PresentationItem`'s precedent already implied - not a redesign, a gap
the first real caller exposed.

### `ContextResolution::Ambiguous` gained per-candidate confidence

Phase 1.0's `ContextResolution::Ambiguous(Vec<ScriptureReference>)` had no
way to express "how confident in each candidate," even though presenting
ambiguity with confidence is an explicit Phase 1.1 requirement. It now
carries `Vec<AmbiguousCandidate>` (`{ reference, confidence }`). The only
prior consumer was a `#[cfg(test)]` null stub, so nothing else needed to
change.

## Provider architecture (unchanged)

The Bible Intelligence Core depends only on the `BibleProvider` trait from
Phase 1.0 - never on SQLite directly. `integrations/bible`'s
`SqliteBibleProvider` is the one real implementation; every pipeline
function takes `&dyn BibleProvider`, so tests use an in-memory
`FakeBibleProvider` (`core/service/src/bible_intelligence.rs`'s test
module) instead of touching a database at all. See
[`docs/database.md`](database.md) for the schema this validates against.

## The transcript test harness

`process_transcript_segment(service_id, text, translation_id, provider,
context)` *is* the harness: deterministic, synchronous, no audio. Call it
once per segment, in order, feeding the same `context` across calls (that's
what lets context survive intervening segments). It has no idea whether
`text` came from a human typing a test fixture, a fixed transcript file, or
eventually a real `SpeechEngine` - and that's the point.

### How Phase 1.2's `SpeechEngine` plugs in

Implemented in Phase 1.2: a real `SpeechEngine` (`ai/speech`, now also
`ScriptedSpeechEngine` and `WhisperSpeechEngine` alongside
`NullSpeechEngine`), `AudioEngine` capture wired to it
(`integrations/audio::CpalAudioEngine`), and a call to
`process_transcript_segment` once per emitted final `TranscriptSegment.text`
- the same call this document's examples already make by hand. As
predicted, no change to the Bible Intelligence Core itself was required;
`process_transcript_segment` and everything above it in this document are
unchanged from Phase 1.1.

Persisting `ScriptureDetection`s/`Suggestion`s into `scripture_detections`
/`ai_suggestions` and emitting `SCRIPTURE_DETECTED` / `SCRIPTURE_UPDATED` /
`SUGGESTION_CREATED` now happens in
`apps/desktop/src-tauri/src/pipeline.rs` and `commands.rs` - thin, Tauri/
SQLite-aware wiring around a `ProcessedSegment`, kept out of `core/service`
as planned. See [`docs/live-speech.md`](live-speech.md) for the full
pipeline, the real `AudioEngine`/`SpeechEngine` implementations, and the
Live Church Brain UI that reviews what this produces.

## Testing

- `core/bible`'s own unit tests cover normalization, book aliasing, and
  detection in isolation (`core/bible/src/{normalize,book_alias,detection}.rs`).
- `core/service/src/bible_intelligence.rs`'s tests cover the orchestrator
  end to end (all 20 required Phase 1.1 scenarios) against a fast in-memory
  `FakeBibleProvider`.
- `tests/tests/bible_intelligence_acceptance.rs` is the realistic,
  multi-segment acceptance test against the real `SqliteBibleProvider` and
  a real migrated+seeded database - the "if this sequence doesn't pass
  deterministically, Phase 1.1 isn't complete" test.

```sh
cargo test -p cip-core-bible
cargo test -p cip-core-service
cargo test -p cip-integration-tests --test bible_intelligence_acceptance
```
