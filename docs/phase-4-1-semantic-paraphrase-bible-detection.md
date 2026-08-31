# Phase 4.1 — Paraphrase Bible Detection

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `5330ea9` (Phase 4 master-plan gap audit)

## Why this phase exists

The user's own Phase 4 request was to cross-reference the codebase against
the full CIP Master Architecture v1.0 document, close any remaining gaps,
and move on to a new major arc. `docs/phase-4-master-plan-gap-audit.md`
graded every module against that document; when asked which gap to tackle
first, the user chose **"Semantic/paraphrase Bible detection"** - the
master plan's own worked example (Section 8.4): a pastor says "all things
work together for good for those who love God" without ever citing
"Romans 8:28," and the system should still suggest that verse, with a
confidence score, for the operator to approve.

## An honest scope correction from the gap audit

The gap audit itself framed this gap as requiring "a local embedding model
+ vector search," grouped with semantic Bible search and cross-reference
intelligence as one bundle depending on shared infrastructure. That
framing does not survive contact with this container's actual
constraints: every prior phase in this project established that
huggingface.co-class model hosts are network-blocked here, and no
embedding/vector library (`candle`, `onnxruntime`, `hnsw`, `faiss`,
`usearch`, `tantivy`, etc.) exists anywhere in this workspace's
`Cargo.lock`. Attempting to bolt one on inside this session would mean
either vendoring a model file this container cannot download, or
fabricating a "semantic" system that's secretly still just string
matching under a misleading name - both violate this project's evidence-
and-honesty discipline more than simply not building the embedding tier
at all.

What this phase delivers instead is the **lexical/keyword-overlap** slice
of that gap: real, working, honestly-labeled detection of a paraphrase
that shares most of its distinctive vocabulary with a specific verse -
exactly the master plan's own Section 8.4 example, which itself is a
near-quotation, not a conceptual rewording. What it explicitly does
**not** deliver is the harder "conceptual references" tier the master
plan also describes (e.g. "Jesus said we should love our enemies" for
Matthew 5:44, sharing almost no vocabulary with the verse) - that remains
a documented, not-yet-started gap requiring real embeddings, unchanged
from the prior audit. See "Known limitations" below.

## Architecture audit (before writing any code)

A research pass over `core/bible`, `core/service`, `core/confidence`, and
`integrations/bible` established:

- `core/bible::detection::detect_candidates` is pure syntax matching
  (`Book chapter:verse` shapes) - it has no concept of a citation-free
  paraphrase and was never going to be extended to have one; paraphrase
  detection has to live at a later pipeline stage, same as
  `Sequential`/`Ambiguous`/`Unresolved`.
- `ReferenceKind` already documents those three variants as "pipeline-
  level outcomes assigned by `core/service`'s orchestrator" rather than
  syntactic detections - `Paraphrase` fits that exact category, just
  decided even later (after every syntactic candidate in a segment has
  already failed to produce a suggestion).
- `BibleProvider::search` (LIKE-based substring search) already exists on
  every implementation, real (`SqliteBibleProvider`) and fake alike -
  reusing it as the retrieval primitive avoids inventing new indexing
  infrastructure or a schema migration for a feature whose honest scope
  is "keyword overlap," not "ranked full-text search."
- `ConfidenceSource::Heuristic` vs `::Model` already exists as a
  documented distinction - `Model` is reserved for "a machine learning
  model (speech, classifier, embeddings)." Paraphrase detection here uses
  `Heuristic`, matching what it actually is.
- `ai/embeddings` (`cip-ai-embeddings`) is a pre-existing, deliberate empty
  placeholder crate reserved for a *future* real embedding model. This
  phase does not touch it or fill it in - using it for a lexical-overlap
  feature would misrepresent what the crate is for.

## What was built

### `core/bible/src/paraphrase.rs` (new)

Pure, deterministic, dependency-free scoring:

- `significant_words(text)` - lowercases, splits on non-alphanumerics,
  drops a small scripture-specific stopword list and short tokens, then
  lightly stems what's left (a handful of suffix rules for
  `-ing`/`-ed`/`-es`/`-s`, not a real Porter/Snowball stemmer) so
  "work"/"works" and "call"/"called" match without over-stripping short
  words.
- `significant_word_count(text)` - the deduplicated count callers use to
  gate against short utterances scoring perfectly by accident.
- `score_overlap(query_text, verse_text)` - the fraction of the query's
  *distinct* significant words also present in the verse text, `0.0..=1.0`.
  Deliberately asymmetric (recall of the query's vocabulary in the verse,
  not general similarity), because a paraphrase is judged by how much of
  what the operator said came from the verse.

### `core/bible::ReferenceKind::Paraphrase` (new variant)

Added alongside `Sequential`/`Ambiguous`/`Unresolved` with the same
"pipeline-level outcome, not a syntactic detection" framing in the type's
own doc comment. Label `"PARAPHRASE_REFERENCE"`.

### `BibleProvider::find_similar_verses` (new default trait method)

```rust
fn find_similar_verses(&self, translation_id: &str, query_text: &str, limit: usize)
    -> Result<Vec<BibleVerse>, BibleProviderError>
```

Default implementation unions `search()` results for each of the query's
distinct significant words - every existing `BibleProvider` implementation
(`SqliteBibleProvider`, both `FakeBibleProvider`s, `EmptyProvider`) gets
this for free with zero changes, since it's built entirely from a method
the trait already required. No migration, no new index, no schema change.

### `core/service::bible_intelligence::try_paraphrase` (new fallback)

Runs in `process_transcript_segment` only when the segment produced **no
suggestion at all** through the normal citation-based path (so an explicit
"Romans 8:28" is never second-guessed by a lexical heuristic). Gated by
two thresholds, both chosen against the master plan's own example and this
project's existing false-positive test corpus:

- `MIN_PARAPHRASE_SIGNIFICANT_WORDS = 4` - a segment needs at least 4
  distinct significant words before scoring is even attempted, so short
  utterances ("Praise God", "Let's pray") can't reach a perfect ratio by
  sharing one or two words.
- `MIN_PARAPHRASE_SCORE = 0.75` - at least 75% of the segment's
  significant vocabulary must appear in the candidate verse.

Never mutates the active Scripture context (`ScriptureContextManager`) -
a paraphrase is not a citation, so it must never establish or replace
context the way a real `Chapter`/`Direct` reference does. Produces, at
most, a `Pending` `Suggestion` - identical guarantees to every other path
in this module (never auto-approved, never auto-projected).

### Dedup fix: category-aware deduplication (`persistence.rs`, `pipeline.rs`)

Wiring paraphrase detection into the live pipeline surfaced a real
interaction the existing regression suite caught immediately: the
existing suggestion-dedup window (originally "don't re-suggest the same
reference within 60 seconds") would silently suppress an explicit
citation if a `Paraphrase` guess for the same verse had already fired
moments earlier in the same service - exactly the realistic case of a
pastor paraphrasing a verse and then reading it directly. Fixed by adding
`DetectionCategory` (`Explicit` vs `Paraphrase`) and querying
`scripture_detections` (which already records every detection's
`detection_type`) instead of `ai_suggestions`, so dedup only suppresses a
repeat *within the same category* - a `Paraphrase` guess is still deduped
against a recent `Paraphrase` guess, and an explicit citation is still
deduped against a recent explicit citation (preserving the original
behavior the existing test asserted), but the two categories never
suppress each other in either direction. The now-superseded
`has_recent_suggestion_for_reference` function and its test were removed
rather than left as dead code.

### Exhaustive-match updates

`core/intelligence::bible_adapter::finding_for_detection` gained a
`Paraphrase` arm (maps to `AssertionLevel::Suggested`, labeled "(paraphrase,
not cited)" in the finding summary). `core/service::confidence_for_kind`
gained an unreachable-but-exhaustive arm (paraphrase confidence is built
directly from the real overlap score in `try_paraphrase`, never through
this function). `commands.rs::emit_processed_segment_events`'s existing
wildcard arm already covers `Paraphrase` correctly with no change needed.

### Frontend

`domain/ai.ts`'s `ReferenceKind` union gained `"paraphrase"`, documented
with the same lexical-overlap honesty framing. `LiveChurchBrain.tsx`'s
`SuggestionCard` now renders `suggestion.confidence.reason` when present -
this already existed as backend data (every `ConfidenceResult` has always
carried an optional `reason`) but was never surfaced in the UI; it's the
natural, minimal way to show the operator *why* a paraphrase suggestion
appeared without a citation ("lexical overlap with ROM 8:28 (100% of
significant words matched, not a citation)"), and is harmless/useful for
every other suggestion kind too.

## Deliberate regression-test update

`apps/desktop/src-tauri/src/pipeline.rs::phase_1_5_full_service_validation`
previously asserted that "And we know that all things work together for
good." (with an active Romans 8 context already established, but no
citation) must **never** produce a suggestion - the exact scenario this
phase is built to change. Updated deliberately, with an expanded comment
explaining the narrowed "resemblance is never enough" stance and why: the
segment now produces exactly one `Paraphrase` suggestion for ROM 8:28,
`Pending`, `Heuristic`-sourced, and the active context (chapter 8) remains
provably unchanged by it. Every other false-positive assertion in that
test (the three unrelated-prose segments, the later explicit "Look at
verse twenty-eight" flow) is unchanged and still passes.

## New tests

- `core/bible/src/paraphrase.rs`: 7 unit tests - stopword/stemming
  correctness, the master plan's own worked example scoring ≥0.95, the
  shorter existing-test phrasing scoring ≥0.9, four unrelated sentences
  scoring <0.5, and the empty-query zero-not-panic case.
- `core/bible/src/provider.rs`: 2 tests for the new
  `find_similar_verses` default implementation (retrieves the right verse;
  finds nothing for vocabulary absent from the dataset).
- `core/service/src/bible_intelligence.rs`: 4 new tests (numbered 21-24
  in that file's existing sequence) - a close paraphrase produces a
  `Paraphrase` suggestion; an explicit citation is never second-guessed by
  the fallback; paraphrase detection never mutates context; short/unrelated
  segments never trigger it.
- `apps/desktop/src-tauri/src/persistence.rs`: 4 new tests for the
  category-aware dedup fix - same-category repeat is a duplicate within
  the window and not after; an explicit citation is not suppressed by a
  recent paraphrase; the reverse (a paraphrase is not suppressed by a
  recent explicit citation); the current segment's own just-persisted row
  is correctly excluded from its own dedup check.
- `apps/desktop/src-tauri/src/pipeline.rs::phase_1_5_full_service_validation`:
  updated in place (see above), not a new test.

## Performance (measured, not estimated)

Timed `process_transcript_segment` end to end against a real
`SqliteBibleProvider` loaded with the full production BSB dataset (31,086
verses, all 66 books) in an unoptimized debug build - the worst case this
project's test suite ever exercises:

| Segment | Result | Elapsed |
|---|---|---|
| "And we know that all things work together for good." | 1 suggestion (paraphrase fires) | 92ms |
| "Paul is showing us the work of the Spirit." | 0 suggestions (below threshold) | 28ms |
| "For God so loved the world that he gave his only son." | 1 suggestion (paraphrase fires) | 329ms |
| "Let us pray together this morning as we gather." | 0 suggestions (below threshold) | 101ms |

The paraphrase fallback only ever runs when nothing else in the segment
already produced a suggestion - a segment with an explicit citation never
pays this cost. Every measured case is well under the multi-second cadence
final transcript segments actually arrive at in live speech (see
`docs/phase-3-8-7-3-audit.md`'s pipeline timing analysis), so this is not
a live-pipeline bottleneck, but a release build would be measurably faster
than these debug-build numbers, and a future phase could still swap the
default `search()`-based retrieval for an indexed one (FTS5, confirmed
available in this project's `rusqlite` configuration with zero Cargo.toml
changes) if a much larger dataset or tighter latency budget ever demanded
it.

## Offline dependency proof

No network call, no model download, no new crate dependency anywhere in
this phase's diff - `cargo tree` for the touched crates is unchanged
except for the new `core/bible::paraphrase` module, which imports only
`std::collections::HashSet`. Confirmed via direct code reading (every
function `search()`s the local `BibleProvider`, nothing else) rather than
a network-block test, since no code path introduced here could reach the
network in the first place.

## Full regression result

Backend: `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean (both default and `--features whisper`); `cargo test --workspace` and `cargo test --workspace --features whisper` both fully green (every crate's `test result: ok`, zero failures). Frontend: `tsc -b` (0 errors), `oxlint` (0 errors, same pre-existing `set-state-in-effect`/`only-export-components` warning pattern as every prior phase), `vitest` 212/212 passed (211 prior + 1 new domain-contract test for the `paraphrase` kind), `vite build` clean.

## Design decisions

- **Lexical overlap, not embeddings, and said so everywhere.** Every doc
  comment, UI string, and this document itself calls this "lexical/
  keyword-overlap matching," never "semantic," "neural," or "AI
  understanding" - the master plan's own language for the harder tier
  this phase does not deliver.
- **Reused `search()` instead of a new FTS5 migration.** An FTS5 virtual
  table was evaluated and confirmed to work in this project's exact
  `rusqlite` configuration (via a standalone scratch probe, not kept in
  the repo), but building and maintaining a synced index for a feature
  whose honest scope is "keyword overlap over a few dozen candidate
  words" was more infrastructure than the problem justified - the
  smallest-justified-fix discipline this project has followed throughout.
- **Category-aware dedup, not a broader rewrite.** The existing
  reference-based dedup window was kept exactly as it was for same-
  category repeats (an explicit citation repeated twice is still
  suppressed, matching the pre-existing test's expectation); only the
  cross-category interaction that paraphrase detection newly introduced
  was changed, via the smallest mechanism available (the `detection_type`
  column already on `scripture_detections`) rather than a schema
  migration.
- **`ai/embeddings` left untouched.** It remains a reserved-but-empty
  placeholder for whatever phase eventually gets real embedding-model
  access in this environment; wiring lexical overlap through it would
  misrepresent both what was built here and what the crate is for.

## Known limitations

- **"Conceptual references" remain unsupported.** A paraphrase that
  shares little or no vocabulary with its source verse (the master plan's
  own harder example, "Jesus said we should love our enemies" for Matthew
  5:44) will score near 0 and never surface - this requires real semantic
  embeddings, which remain unavailable in this network-restricted
  container. Still an open, explicitly-tracked gap.
- **No cross-translation matching.** `find_similar_verses` only searches
  within the segment's own `translation_id` - a paraphrase of a KJV-style
  wording will not be matched against a BSB verse's differently-phrased
  text, or vice versa, even though they're the same underlying verse.
- **English-only stopword/stemming lists.** `core/bible::paraphrase`'s
  tokenizer is tuned for English; a future multi-language phase (already
  tracked in the Phase 4 gap audit) would need equivalent lists per
  supported language.
- **Retrieval, not ranking.** `find_similar_verses`'s default
  implementation gathers candidates without any relevance ordering (LIKE
  has none) - `try_paraphrase` scores and picks the best of whatever comes
  back, so correctness doesn't depend on retrieval order, but a verse
  whose distinctive words happen not to be searched first among a very
  large candidate set could in principle be missed if `MAX_PARAPHRASE_CANDIDATES`
  (25) is reached before it's found. Not observed against the real 31k-verse
  dataset in this phase's own testing, but worth noting as a scale limit.

## Final gate

| Item | Status |
|---|---|
| Paraphrase of the master plan's own worked example detected end to end | DONE |
| Never auto-projected - always a `Pending` suggestion requiring operator approval | DONE |
| Explicit citations never second-guessed or suppressed by the heuristic | DONE |
| Existing false-positive corpus (unrelated prose) still produces zero suggestions | DONE |
| Deliberate, documented update to the one existing test whose behavior this phase intentionally changes | DONE |
| Full regression green (backend both configs + frontend) | DONE |
| Real embedding/semantic matching ("conceptual references") | **NOT DELIVERED** - honestly scoped out; no embedding model reachable in this container |

**Phase 4.1: PASS** for its honestly-scoped goal (lexical/keyword-overlap
paraphrase detection). The harder semantic/conceptual tier the master plan
also describes remains a tracked, not-yet-started gap - see
`docs/phase-4-master-plan-gap-audit.md`, updated alongside this phase.
