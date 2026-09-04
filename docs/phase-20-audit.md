# Phase 20: Fuzzy Book-Name Matching + Manual Context-Correction Aliases

## Trigger

Two direct operator requests in one message: "start Phase 20 with fuzzy book-name
matching" (the leading item explicitly deferred out of Phase 19's response to the
operator's own detection-accuracy audit), plus a real UX complaint with a screenshot
of the Live Service UI's "Correct Active Context" panel: "Bible edit here requires
pressing the full name of the book and chapter and verse can you change it along
side."

## Part 1: Fuzzy book-name matching (detection)

### Design

`core/bible/src/book_alias.rs` gains `fuzzy_match_book(&str) -> Option<(&CanonicalBook, f32)>`,
a Levenshtein-distance near-miss matcher scoped deliberately narrowly:

- Only matches against the **single-word** canonical book names (48 of the 66
  books - Genesis, Romans, Revelation, etc.). Multi-word names ("1 Corinthians",
  "Song of Solomon") are excluded entirely: fuzzy-matching a single mis-heard word
  against a multi-word name, or worse, guessing which of two numbered variants
  ("1 John" vs "2 John") a bare near-miss word meant, is a fundamentally
  higher-risk problem this function doesn't attempt - those books remain reachable
  through `canonicalize_book`'s existing exact alias table ("1 cor", "2 tim", ...).
- The edit-distance budget scales with the canonical name's length (1 char for
  names up to 5 letters, 2 for 6-9, 3 for longer) - a fixed threshold would either
  reject obvious short-name typos or accept wild long-name guesses.
- Refuses to guess on input under 4 characters, non-alphabetic input, or a tie
  (two books equidistant from the input) - "never guess" applies here exactly as
  it does everywhere else in this codebase's detection code.

`core/bible/src/detection.rs`'s `detect_candidates` gets a second pass: for every
word not already claimed by an exact `BOOK_PATTERN` match, try `fuzzy_match_book`,
and only trust it enough to emit a candidate when the word is *also* immediately
followed by a real two-number (chapter:verse) shape - the same precision guard the
exact pass gets from `BOOK_PATTERN` itself. A fuzzy-matched word with no citation
shape after it (e.g. "Roman was a great empire") is far too weak a signal alone
and produces nothing. Chapter-only fuzzy shapes are deliberately never attempted -
see below.

A new `ReferenceKind::FuzzyBook` variant carries this result. Unlike
`Paraphrase`/`Semantic` (pipeline-level fallbacks only attempted when nothing else
in a segment produced a suggestion), `FuzzyBook` is produced directly by
`detect_candidates` because it's still fundamentally the same syntactic citation
shape, just with a tolerant book-name match.

`core/service/src/bible_intelligence.rs`'s new `resolve_fuzzy_book`:

- Re-validates the guessed book/chapter/verse against the real `BibleProvider`
  before ever producing a suggestion - "do not trust the parser alone" applies at
  least as strongly to a fuzzy guess as to an exact citation.
- **Never mutates the active Scripture context**, even after validation succeeds -
  exactly like `try_paraphrase`/`try_semantic`. A near-miss book name is not an
  explicit citation, so it must never become the trusted context a later bare
  `"verse N"` would silently inherit. This is also why the detection layer never
  attempts a chapter-only fuzzy shape: a fuzzy chapter-only "match" would have no
  verse to suggest and would do nothing except risk a false positive for zero
  benefit.
- Derives confidence directly from the real fuzzy-match similarity score
  (dampened by a further 0.85 multiplier), never `confidence_for_kind`'s fixed
  per-exact-kind scores - a near-miss can never out-rank or be mistaken for a
  genuine `Direct` citation.

`core/intelligence/src/bible_adapter.rs` gets a new match arm (mirroring
`Paraphrase`) so cross-domain correlation renders `FuzzyBook` findings honestly
labeled, not silently dropped by an exhaustive-match compile error.

### What this closes

Real Whisper mishearings that previously produced zero candidates at all: "Roman
8:28" (dropped trailing "s"), "Revelations 21:4" (added trailing "s"), "Galatins
5:22" (dropped internal letter) now resolve to a real, validated, honestly
lower-confidence suggestion instead of silently vanishing.

### Explicitly deferred (not this phase)

- **Multi-word/numbered books** (1/2/3 Samuel, Kings, Chronicles, Corinthians,
  Thessalonians, Timothy, Peter, John; Song of Solomon) - see the design section
  above for why guessing between numbered variants is a different, higher-risk
  problem. These remain covered only by exact aliases.
- **Fuzzy matching against short aliases** ("Rom", "Jn") rather than full
  canonical names - a 2-3 character near-miss budget is too noisy to trust; this
  phase only fuzzy-matches full canonical names.
- **Interim transcripts, overlapping Whisper windows + VAD** - still deferred from
  Phase 19, unrelated to this phase's scope, still require real-hardware
  validation this container cannot perform.

### Testing boundary

`fuzzy_match_book` is pure and fully unit-tested (8 new tests in
`book_alias.rs`, including a systematic sweep - single-character-dropped typos of
every single-word canonical book name - that never resolves to the *wrong* book).
`detect_candidates`'s fuzzy pass has 5 new tests. `resolve_fuzzy_book`/the
orchestrator has 4 new tests confirming validation, the "never mutates context"
invariant, and that an exact reference is never reclassified as fuzzy.

## Part 2: Manual "Correct Active Context" now accepts any book form

### The bug

`apps/desktop/src-tauri/src/commands.rs::correct_scripture_context` passed the
operator's typed book text straight to `BibleProvider::get_chapter`, which does an
exact SQL match against `bible_books.code` (a 3-4 letter internal code like
`"JHN"`). The frontend then additionally `.toUpperCase()`'d whatever the operator
typed - so typing the book's actual full name ("John") produced `"JOHN"`, which
never matches the stored code `"JHN"` (no letter "O"). The only input that ever
worked was the exact internal code, which the operator has no way to know and the
UI never told them (the placeholder said "e.g. ROM", the input's own label says
nothing about codes vs. names).

### The fix

`correct_scripture_context` now resolves the operator's input through
`cip_core_bible::canonicalize_book` - the same alias table
(`core/bible/src/book_alias.rs`) the live detector itself already uses - before
validating the chapter. Typing the full name ("John"), a common abbreviation ("Jn",
"1 Cor"), or the raw code ("JHN") all now work identically, case-insensitively. An
unrecognized book text now returns a clear `"not a recognized Bible book: <text>"`
error instead of a confusing false-negative from an internal code mismatch. The
frontend drops its own `.toUpperCase()` (no longer meaningful - resolution is
case-insensitive) and updates its hint/placeholder text to say plainly that any
form works.

This is a pure reuse of an already-exhaustively-tested function
(`canonicalize_book`, unchanged this phase) - no new backend logic needed its own
test; the existing `book_alias.rs` test suite already covers every alias/name/code
form this command now accepts.

## Testing boundary (both parts)

Everything above is pure/deterministic and fully covered by unit and orchestrator
tests without any audio or Tauri runtime dependency - consistent with this
project's established "no `tauri::test` harness" boundary (see prior phase docs).

## Full regression result

`cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean
(default and `--features whisper`), `cargo test --workspace` clean in both
feature configs (cip-core-bible 98/98, up from 85 - the 13 new fuzzy-matching
tests; cip-core-service/cip-core-intelligence/cip-desktop all pass with their new
FuzzyBook coverage; every other crate unchanged). Frontend: `npm run typecheck` 0
errors, `npm run lint` the same 5 pre-existing warnings (unchanged), `npm run test
-- --run` 303/303 (unchanged - no new frontend test needed since the manual-entry
fix reuses an already-tested backend function and only widens a TS union type),
`npm run build` clean.

## Architectural safety

- Zero new Tauri commands, zero new events, zero new migrations, zero schema
  changes.
- `ReferenceKind::FuzzyBook` never mutates `ScriptureContext` - confirmed by a
  dedicated regression test.
- Every other domain contract crate (core/sermon, core/music, core/presentation)
  is entirely untouched.
- `correct_scripture_context`'s new behavior is strictly more permissive (accepts
  everything it did before, plus more forms) - no existing valid input becomes
  invalid.

## Known limitations (honest, not deferred silently)

- Fuzzy matching only covers single-word book names - the numbered-book gap
  (1/2/3 Samuel/Kings/Chronicles/Corinthians/Thessalonians/Timothy/Peter/John) is
  real and explicitly deferred above, not silently dropped.
- The similarity threshold/distance budget is documented reasoning, not
  empirically calibrated against real mishearing data - no labeled real-service
  audio-to-transcript corpus exists in this repository to calibrate against yet
  (the same honest limitation Phase 19's ordinal work and the paraphrase/semantic
  thresholds already carry).
- This exact change has not been verified against a real pilot session - the next
  real-hardware test is an operator speaking a plausibly mis-transcribable book
  name ("Romans" coming through as "Roman") during a live service and confirming
  it now surfaces as a suggestion where it previously wouldn't have, and
  separately using the Correct Active Context field with a full book name/
  abbreviation instead of the internal code.

## Final gate

Environment A (fmt/clippy/test, both feature configs, plus full frontend
typecheck/lint/test/build): PASS. Environment C (a real operator triggering a
fuzzy-book detection, and a real operator using the corrected manual-entry field,
during a live service): not yet performed.
