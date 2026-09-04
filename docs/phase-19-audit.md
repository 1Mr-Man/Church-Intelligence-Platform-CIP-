# Phase 19: Detection Accuracy Audit Response

## Trigger

The operator ran their own independent audit of the repository (not asked for by this session)
identifying why live Bible detection misses most references, and asked whether to start
implementing any of the findings. This phase first verified every factual claim in that audit
against the actual current code (rather than taking it on faith), then implemented the
safest, most bounded, highest-confidence fix from it.

## Verification of the operator's audit

Read every file/line the audit cited directly. Confirmed accurate:

- `CHUNK_SAMPLES = SAMPLE_RATE_HZ * 3` (`ai/speech/src/whisper.rs:51`) - a fixed 3s hard cut,
  no overlap, no VAD-aligned boundary.
- Phase 3.8.7.3's own status is still genuinely `HOLD` (`docs/phase-3-8-7-3-live-speech-stability.md:272`)
  - never confirmed under a real 60-minute load, even though the worker-thread fix itself
  shipped.
- `core/bible/src/detection.rs` is pure regex (6 shapes + a book-name alternation) with zero
  fuzzy tolerance - confirmed no `levenshtein`/`fuzzy`/`soundex` anywhere in `core/bible`.
- `core/bible/src/normalize.rs` converted cardinals only, no ordinals - "First Corinthians"
  worked *only* because `book_alias.rs` hardcoded it as a literal per-book alias string, not
  because of any general rule.
- `MIN_PARAPHRASE_SCORE = 0.75`, `MIN_SEMANTIC_SIMILARITY = 0.55`,
  `MAX_PARAPHRASE_CANDIDATES = 25` (`core/service/src/bible_intelligence.rs:66-89`) - the
  code's own comments say these are documented, not empirically calibrated.
- Whisper never emits interim segments - `is_final: true` is hardcoded
  (`ai/speech/src/whisper.rs:379`), confirmed by the module's own doc comment.
- 18 migrations exist, matching the audit's count.

One claim needed correction: the audit stated *"Only Bible detection runs live...
sermon/content intelligence... effectively dead in a service."* Direct trace of
`commands.rs::finalize_and_route_segment` → `route_segment_to_live_intelligence_engines`
(established by Phase 3.8.7.5, "Live Intelligence Router") shows Sermon, Service, and
Music (text-based) intelligence **do** run live, automatically, on every accumulated
12-20s segment - this is not a gap. Content Intelligence and Cross-Domain Correlation are
correctly described as never automatic, but that is a documented Phase 2.7/2.4 design
decision (`analyze_content_intelligence`'s own doc comment: "an explicit operator/diagnostic
action, never triggered automatically by a transcript segment arriving"), not an oversight.

One claim needed a footnote: bare-verse fragments ("verse 4") are matched by an existing
`BARE_VERSE_PATTERN` (already implemented, contrary to a literal reading of "silently
dropped" as never detected) - but `resolve_bare_verse` returns `Unresolved` when there is no
active scripture context, and `Unresolved` detections are never persisted, so the practical
end result the audit described (nothing reaches the operator) is accurate.

## What was implemented this phase

The audit's own prioritized list ranks "overlapping windows + VAD-triggered flush" and
"emit interim transcripts" as the biggest wins, but both require real-time audio on real
hardware to validate safely - this container has none, and a wrong change to the live
capture/segmentation path risks a regression nobody could catch before it reached the
operator. The audit's own "confirm Whisper model size" and "verify Phase 3.8.7.3 under load"
items are real-hardware verification tasks, not code changes.

This phase implemented the one item that is genuinely safe, bounded, and fully verifiable
without real hardware: **general ordinal normalization** (`core/bible/src/normalize.rs`).

- New `ORDINALS` table + `ordinal_value()`, deliberately scoped to `first`/`second`/`third`
  and their digit-suffix forms `1st`/`2nd`/`3rd` - covers every real case, since no canonical
  Bible book is ever numbered past three (1/2/3 Samuel, Kings, Chronicles, Corinthians,
  Thessalonians, Timothy, Peter, John - see `book_alias.rs`). A general 1st-99th ordinal
  parser was deliberately not built: chapter/verse numbers are always spoken as cardinals,
  never ordinals, so nothing else would ever consume it.
- Wired into `normalize_word_token` alongside the existing cardinal-word conversion.
- Confirmed via `normalize_text` is called only on a local copy for detection
  (`core/service/src/bible_intelligence.rs:249,295`) - it never touches the transcript text
  actually persisted or shown to the operator, so this is a detection-only, zero-risk change.
- Closes a real, previously-uncovered gap: digit-suffix forms ("1st Corinthians", "2nd
  Timothy") were never in `book_alias.rs`'s literal alias table and were invisible to the
  detector before this phase - only the word forms ("First Corinthians") worked, and only
  because someone had hand-written that exact alias.

## Explicitly deferred (not this phase)

- **Fuzzy/near-miss book-name matching** - the audit's own "biggest single win" that's safe
  to build without real hardware. Not attempted this phase: it is genuinely a Phase-4.1/4.4-
  sized feature (a new detection tier, a new `ReferenceKind`, wiring through every exhaustive
  match site `bible_intelligence.rs` already guards with its own regression tests, a
  persistence column, frontend mirroring, and its own confidence calibration) - not a
  same-turn addition to rush alongside a verification pass. Real next-phase candidate.
- **Overlapping windows + VAD-triggered flush** - the audit's own top pick, but touches the
  live capture/segmentation path this container cannot exercise against real timing-sensitive
  audio. Any change here needs a real-hardware verification loop this session doesn't have.
- **Interim transcript detection** - same real-hardware risk, plus a genuine semantic change
  to `is_final`'s meaning that several other subsystems (persistence, dedup windows) currently
  assume is always true for anything they see.
- **Operator-adjustable thresholds** - a real, valid idea, but needs its own settings-UI design
  pass, not a drive-by change to hardcoded constants.
- **Bare-verse speculative low-confidence suggestions** - genuinely underspecified: what would
  an operator do with "verse 4, book/chapter unknown"? Needs actual design thought, not a
  rushed implementation.
- **End-to-end audio→suggestion fixture test** - a real, valuable idea (the audit is right that
  359/359 unit tests prove components work in isolation, not the funnel end to end), but no
  labelled real service-audio fixture exists in this repository to build it against yet.

## Testing boundary

`ordinal_value`/`normalize_word_token`'s ordinal branch are pure and fully unit-tested (3 new
tests: word-form ordinals, digit-suffix ordinals, and a negative test confirming "fourth"/"4th"
are deliberately left unconverted).

## Full regression result

`cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean (default and
`--features whisper`), `cargo test --workspace` clean (cip-core-bible 85/85, up from 82 - the
3 new tests; cip-desktop unaffected at 359/359 in both feature configs; every other crate
unchanged). No frontend files touched this phase - `npm run typecheck`/`lint`/`test`/`build`
not re-run beyond confirming `git status` shows no frontend files changed.

## Architectural safety

- Zero new Tauri commands, zero new events, zero new migrations, zero schema changes.
- `normalize_text` is called only on a local copy for detection - this change cannot affect
  the transcript text persisted or shown to the operator.
- Every other domain contract crate (core/sermon, core/music, core/service's non-normalize
  logic, core/presentation) is entirely untouched.

## Known limitations (honest, not deferred silently)

- This closes one narrow, real gap (digit-suffix ordinals) - it does not move the needle on
  the audit's own identified biggest contributors (the 3s window cut, zero fuzzy tolerance,
  uncalibrated thresholds). Those remain open, explicitly deferred above with reasoning, not
  silently dropped.
- This exact change has not been verified against a real pilot session - the next real-hardware
  test would be an operator saying "1st Corinthians thirteen four" or "2nd Timothy chapter
  three" during a live service and confirming it is now detected where it previously wasn't.

## Final gate

Environment A (fmt/clippy/test, both feature configs): PASS. Environment C (a real operator
speaking a digit-suffix-ordinal book reference during a live service): not yet performed.
