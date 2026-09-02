# Phase 5.2 — Temporal Confirmation / Sliding Re-Score

## Baseline

Phase 5.1 (Post-Service Observability Report) closed the first slice of
the operator's "Reliability & Trust" Phase 5 theme with a read-only
summary. This phase closes the second slice, named directly from the
operator's own roadmap item: *"Temporal confirmation/sliding re-score."*

Auditing the live detection pipeline (`core/service::process_transcript_segment`,
`apps/desktop/src-tauri/src/pipeline.rs::handle_final_transcript`) found
every detection is scored once, from a single transcript segment, with no
mechanism that ever revisits that score. The existing suggestion-dedup
window (`SUGGESTION_DEDUP_WINDOW_SECONDS`, Phase 1.3/4.1) already detects
when the same reference recurs within 60 seconds - but treats every
repeat purely as noise to discard, never as evidence.

## Why this phase exists

`Paraphrase` and `Semantic` detections are heuristic, single-shot guesses
against real confidence ceilings well below an explicit citation's ~0.97
(`Paraphrase` starts at its raw lexical-overlap score, `MIN_PARAPHRASE_SCORE`
0.75 floor; `Semantic` starts at its raw cosine-similarity score,
`MIN_SEMANTIC_SIMILARITY` 0.55 floor - both explicitly documented as
uncalibrated, not empirically tuned). When a pastor's wording independently
re-triggers the same heuristic guess for the same verse a second time
within the dedup window, that repetition is genuine corroborating
evidence the codebase was silently discarding. This is the "sliding
re-score" the roadmap item names: an already-surfaced suggestion's
confidence rises as further evidence for it arrives, rather than staying
frozen at its first-glance score.

## Architecture decisions

- **Confirm, don't re-suggest**: dedup's existing "suppress a same-category
  repeat" behavior is completely unchanged - no second suggestion is ever
  created. A suppressed repeat now also looks up the original, still-`Pending`
  suggestion and bumps its confidence, additive on top of dedup rather than
  replacing it.
- **Scoped to `Paraphrase`/`Semantic` only**: `Direct`/`Chapter`/`Verse`/
  `Sequential` citations are already near the confidence ceiling (0.85-0.97);
  confirming a repeated citation would add no honest signal. Only the two
  heuristic fallback kinds are confirmation-eligible.
- **Only `Pending` suggestions are ever touched**: once an operator has
  approved, edited, or rejected a suggestion, its fate is decided - a later
  redetection must never mutate a human decision after the fact.
- **A hard, documented confidence ceiling below a real citation**: a
  confirmed heuristic guess can rise to at most `MAX_CONFIRMED_SCORE` (0.9),
  deliberately below the ~0.97 an explicit citation earns - no amount of
  repetition ever lets a heuristic guess outrank a real citation.
- **`confirmation_count` is an honest occurrence count, not a proxy for the
  score** - it keeps incrementing even once the score cap is reached, so the
  operator can see "this was independently redetected 3 times" distinctly
  from the (capped) confidence number.
- **A real, previously-latent dedup bug found and fixed during this audit**:
  `ReferenceKind::Semantic` had no `DetectionCategory` of its own - it fell
  into the `Explicit` bucket by omission, so a repeated `Semantic` guess was
  checked only against `DIRECT`/`VERSE`/`SEQUENTIAL_REFERENCE` detection
  rows, never against another `SEMANTIC_REFERENCE` row. A repeated semantic
  guess could never dedup against itself. `DetectionCategory::Semantic`
  (its own bucket, its own SQL branch) restores the same "repeat within the
  window is suppressed" guarantee `Paraphrase` already had - a prerequisite
  for confirmation to ever fire for `Semantic` detections at all, and a real
  correctness fix independent of this phase's main feature.

## What was built

- **`database/migrations/0014_suggestion_confirmation.sql`** - additive
  `ALTER TABLE ai_suggestions ADD COLUMN confirmation_count INTEGER NOT NULL
  DEFAULT 0`. Every pre-existing row defaults to `0` - nothing is
  retroactively reclassified as "confirmed."
- **`core/ai::Suggestion`** gained `confirmation_count: u32`, always `0` at
  construction (`Suggestion::new`) - `core` itself has no notion of
  "redetected" (that requires already-persisted history only the desktop
  layer sees).
- **`apps/desktop/src-tauri/src/persistence.rs`**:
  - `DetectionCategory::Semantic` (new variant) + its own SQL branch in
    `has_recent_detection_for_reference` (the dedup-bug fix above).
  - `find_pending_suggestion_for_reference(conn, service_id, reference)` -
    the most recent still-`Pending` suggestion matching a reference, or
    `None`.
  - `confirm_suggestion(conn, suggestion_id, score_bonus, max_score)` - the
    score/level/count update: `new_score = (current + bonus).min(max_score).max(current)`
    (never decreases, capped, and never exceeds what the caller passes),
    `confirmation_count += 1` unconditionally.
  - `persist_suggestion`/`get_suggestion`/`list_suggestions`/
    `update_suggestion_status` all extended to read/write the new column.
- **`apps/desktop/src-tauri/src/pipeline.rs`**: `handle_final_transcript`'s
  dedup loop now classifies a detection into `Explicit`/`Paraphrase`/
  `Semantic` (previously a 2-way `Paraphrase`-or-`Explicit` split); when a
  `Paraphrase`/`Semantic` repeat is suppressed, it looks up and confirms the
  original suggestion via the two new persistence functions. New policy
  constants `CONFIRMATION_SCORE_BONUS` (0.1) and `MAX_CONFIRMED_SCORE` (0.9).
- **Frontend**: `domain/ai.ts`'s `Suggestion` gained `confirmationCount:
  number`; `LiveChurchBrain.tsx`'s suggestion card renders a small "Confirmed
  ×N" badge (with an explanatory tooltip) whenever `confirmationCount > 0`.

## Full regression result

`cargo fmt --check`: clean. `cargo clippy --workspace --all-targets --
-D warnings`: clean under both default features and `--features
whisper,semantic-search`. `cargo test --workspace` (single-threaded, to
route around the pre-existing, unrelated `config.rs` env-var
test-parallelism flake documented in Phase 5.1): every crate green under
both feature configurations. New test coverage: 9 in `persistence.rs`
(the `Semantic` dedup-category fix, `find_pending_suggestion_for_reference`'s
three cases, `confirm_suggestion`'s increment/cap/never-decrease
behavior), 2 in `pipeline.rs` (a full paraphrase-repeat-confirms
acceptance test against the real dev-seed dataset, and a control test
proving an explicit citation's repeat is never confirmation-boosted), 1 in
`core/ai::suggestion` (`confirmation_count` starts at `0`). Frontend:
`npm run typecheck`/`lint` clean (no new warnings), `npm run test`
220/220 passing, `npm run build` succeeds.

## Windows rebuild

No new native dependency was introduced. Installer: `Church Intelligence
Platform_0.1.0_x64-setup.exe`, SHA-256
`b06ba51af70681e66a3e6c9aa978acaf2a0fbc290779298f1dd650c3178d0eec`,
13,742,758 bytes (+4,703 bytes over the Phase 5.1 baseline of
13,738,055 bytes - expected for the small amount of new compiled code,
no new dependency). See
`pilot-evidence/5.2/windows/installer-contents-verification.json` for
direct binary proof (new `confirm_suggestion`/
`find_pending_suggestion_for_reference` symbols present, prior-phase
symbols confirmed unaffected).

## Architectural safety diff

- Zero changes to any existing command's signature.
- Zero changes to dedup's own suppression behavior - a `Paraphrase`/
  `Semantic` repeat still never creates a second suggestion; confirmation
  is strictly additive on top of that existing path.
- Zero changes to `Explicit`-category detections' scoring or dedup
  behavior - only their existing suppression is retained, with no
  confirmation logic ever touching them.
- Zero changes to any already-decided (`Approved`/`Edited`/`Rejected`)
  suggestion - `find_pending_suggestion_for_reference` filters to `Pending`
  only.
- The one behavior change to existing dedup semantics is the
  `DetectionCategory::Semantic` fix: a repeated `Semantic` guess is now
  correctly suppressed against a prior `Semantic` guess (previously it was
  not, per the bug described above) - a correctness fix, not a new
  category of behavior.

## Environment A / B / C

- **Environment A** (this container): PASSED, fully green, as detailed
  above - including a real acceptance test exercising `handle_final_transcript`
  twice against the real dev-seed BSB/KJV dataset, proving the confidence
  rise end-to-end through persistence.
- **Environment B** (Xvfb GUI reproduction): unavailable in this session's
  container, a pre-existing, already-documented limitation since Phase
  3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, a real live/replayed service
  with a genuine repeated paraphrase): NOT YET VERIFIED. The decisive
  pending gate is the operator's own real-hardware test: speak (or replay)
  a paraphrase of a verse twice within about a minute and confirm the
  Needs Attention queue shows a single suggestion with a "Confirmed ×1"
  badge and a visibly higher confidence than the first mention alone would
  have produced.

## Known limitations

- **`CONFIRMATION_SCORE_BONUS` (0.1) and `MAX_CONFIRMED_SCORE` (0.9) are
  documented policy choices, not empirically calibrated** - consistent
  with every other confidence threshold in this codebase
  (`MIN_PARAPHRASE_SCORE`, `MIN_SEMANTIC_SIMILARITY`), no real operator
  feedback on live services has informed these numbers yet.
- **Confirmation only ever fires within the existing 60-second dedup
  window** - a genuinely repeated paraphrase minutes apart (past the
  window) is treated as a brand-new, unconfirmed suggestion, exactly as
  dedup already treated it before this phase.
- **No UI surface for confirmation on the diagnostics-mode Pending
  Suggestions panel** - only the Needs Attention queue
  (`LiveChurchBrain.tsx`'s suggestion card) shows the "Confirmed ×N" badge
  this phase adds.
- **No negative confirmation / disconfirmation** - if a later segment's
  wording clearly contradicts an earlier heuristic guess, this phase has
  no mechanism to lower confidence; only repetition of the *same* reference
  raises it.
- **Every limitation already documented for Phase 4.1's paraphrase
  detector, Phase 4.4's semantic search, and Phase 5.1's process-lifetime
  diagnostics still applies unchanged** - this phase adds a further
  scoring refinement, it does not revisit or resolve any of them.

## Deferred work

- Empirical calibration of `CONFIRMATION_SCORE_BONUS`/`MAX_CONFIRMED_SCORE`
  once real operator feedback exists.
- Surfacing confirmation state in the diagnostics-mode Pending Suggestions
  panel, not just the Needs Attention queue.
- A genuine disconfirmation mechanism (a later segment's evidence lowering
  an earlier guess's confidence), a substantially different design not
  attempted this phase.
- Real-hardware Environment C verification against an actual repeated
  paraphrase in a live or replayed service.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, a genuine repeated paraphrase, both outside this container's
reach). This phase is a real, verifiable, fully-tested, purely additive
reliability refinement - it never creates a new suggestion, never mutates
a decided one, and never lets a heuristic guess outrank a real citation.
