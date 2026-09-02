# Phase 5.4 — Wrong-Verse Feedback Loop

## Baseline

Phase 5.1 (Post-Service Observability Report), Phase 5.2 (Temporal
Confirmation), and Phase 5.3 (Audio Pipeline Hardening / VAD) closed the
first three slices of the operator's "Reliability & Trust" Phase 5 theme.
This phase closes the fourth, named directly from the operator's own
roadmap item: *"Wrong-verse feedback loop."*

Auditing `reject_suggestion` (`commands.rs`), `update_suggestion_status`
(`persistence.rs`), and the dedup/confirmation branch in
`pipeline.rs::handle_final_transcript_inner` found that an operator's
Reject decision has **zero downstream effect today** - and worse, a real
detection can be silently dropped afterward with no trace at all:

- `reject_suggestion` only flips `SuggestionStatus` to `Rejected`. Nothing
  else in the codebase ever reads that status.
- The suggestion-dedup window (`has_recent_detection_for_reference`)
  checks the `scripture_detections` table, not suggestion status - so if
  the *same* reference/category is redetected within 60s of a reject
  (e.g. the pastor's wording triggers the same false-positive paraphrase
  again), `is_duplicate` is `true` and the code hits the existing
  `continue` branch.
- Inside that branch, Phase 5.2's confirmation logic calls
  `find_pending_suggestion_for_reference`, which filters to `Pending`
  only. Since the suggestion is now `Rejected`, it finds nothing - so the
  repeat is dropped with **zero trace**: no new suggestion, no
  confirmation, no timeline event, nothing beyond a `debug!` log line.

This gap is not unique to `Rejected` - the identical silent-drop happens
after `Approved`/`Edited` too - but for `Rejected` it is the one case
that actually matters: the operator explicitly said "this is wrong," and
the system had no way to remember that decision mattered, or to show it
recurred.

## Why this phase exists

"Wrong-verse feedback loop" names exactly this gap: make an operator's
Reject decision genuinely observable when it recurs, without ever
undermining the decision itself.

## Architecture decision

Two designs were considered and put to the operator directly, since the
choice has real live-service behavioral implications:

1. **Silent echo counter (chosen)**: keep suppressing the repeat exactly
   as today (the operator already said no; don't re-annoy them), but
   increment a new `rejection_echo_count` on the rejected suggestion's own
   row each time this happens - purely observational, visible in
   History/Service Report, never resurrecting a decided suggestion.
2. **Re-surfaced, dampened suggestion**: stop silently suppressing -
   create a new `Pending` suggestion each time, tagged as a recurrence of
   a rejected guess with reduced starting confidence, so the operator can
   reconsider.

The operator chose option 1, with an explicit flow: *detection -> exact
same reference+category -> was it rejected within the window? ->
silently suppress + increment rejection echo counter + record diagnostic
information.* This phase implements exactly that flow.

## What was built

- **`database/migrations/0015_suggestion_rejection_echo.sql`** -
  additive `ALTER TABLE ai_suggestions ADD COLUMN rejection_echo_count
  INTEGER NOT NULL DEFAULT 0`. Every pre-existing row defaults to `0`.
- **`core/ai::Suggestion`** gained `rejection_echo_count: u32`, always `0`
  at construction (`Suggestion::new`), mirroring `confirmation_count`'s
  own discipline - `core` has no notion of "redetected," which requires
  already-persisted history only the desktop layer sees.
- **`apps/desktop/src-tauri/src/persistence.rs`**:
  - `find_rejected_suggestion_for_reference(conn, service_id, reference)`
    - the most recent `Rejected` suggestion matching a reference, or
    `None`. Mirrors `find_pending_suggestion_for_reference` exactly, just
    filtered to `Rejected` instead of `Pending`.
  - `record_rejection_echo(conn, suggestion_id)` - increments
    `rejection_echo_count` unconditionally; never touches `status`,
    `confidence`, or `kind`. A rejected suggestion is a decided
    suggestion, and this must never be the mechanism that quietly
    resurrects one.
  - `persist_suggestion`/`get_suggestion`/`list_suggestions`/
    `update_suggestion_status` all extended to read/write the new column.
- **`apps/desktop/src-tauri/src/pipeline.rs`**:
  `handle_final_transcript_inner`'s dedup-suppression branch now falls
  back to a rejection-echo lookup whenever no `Pending` suggestion is
  found for a `Paraphrase`/`Semantic` repeat: if the most recent
  suggestion for that reference is `Rejected`, its echo count is
  incremented. The repeat is still `continue`d exactly as before -
  nothing about dedup's own suppression behavior changed.
- **Diagnostic information (the operator's third explicit step)**:
  `apps/desktop/src-tauri/src/service_report.rs`'s `SuggestionStats`
  (Phase 5.1's Post-Service Observability Report) gained
  `rejection_echoes: u64` - the sum of every suggestion's
  `rejection_echo_count` for the service, computed in the same read-only
  aggregation pass that already builds the rest of the report. No new
  event, no new command - the existing Service Report is where this
  project's other passive, observational counts (suggestion status
  breakdown, detection-kind counts) already live.
- **Frontend**: `domain/ai.ts`'s `Suggestion` gained `rejectionEchoCount:
  number`; `domain/service.ts`'s `SuggestionStats` gained
  `rejectionEchoes: number`. `HistoryView.tsx`'s Service Report panel
  shows a new clause ("N rejected references redetected again and kept
  suppressed") when nonzero, and its Scripture & Findings list shows
  "echoed ×N" next to a rejected suggestion's status when its own echo
  count is nonzero.

## Full regression result

`cargo fmt --check`: clean. `cargo clippy --workspace --all-targets --
-D warnings`: clean under both default features and `--features
whisper,semantic-search`. `cargo test --workspace` (single-threaded, to
route around the pre-existing, unrelated `config.rs` env-var
test-parallelism flake documented since Phase 5.1): every crate green
under both feature configurations - `cip-desktop` went from 307 to 313
passing tests (4 new in `persistence.rs`, 1 new acceptance test in
`pipeline.rs`, 1 new test in `service_report.rs`), every other crate's
count unchanged except `cip-database` (+1, a data-driven migration test
that scales with the migration count). Frontend: `npm run typecheck`/
`lint` clean (same 4 pre-existing warnings, unrelated to this phase),
`npm run test` 220/220 passing (unchanged - this phase's frontend
changes are fixture updates and two display conditionals with no
dedicated new test), `npm run build` succeeds.

## Windows rebuild

See `pilot-evidence/5.4/windows/installer-contents-verification.json`
for direct binary proof (new `find_rejected_suggestion_for_reference`/
`record_rejection_echo` symbols present, prior-phase symbols confirmed
unaffected).

## Architectural safety diff

- Zero changes to dedup's own suppression behavior - a `Paraphrase`/
  `Semantic` repeat after a rejection is still, exactly as before,
  silently absorbed with no second suggestion ever created.
- Zero changes to any already-decided suggestion's `status`, `confidence`,
  or `kind` - `record_rejection_echo` touches only the new counter
  column. Verified by a dedicated test
  (`record_rejection_echo_increments_the_count_without_touching_status_or_score`).
- Zero changes to `Approved`/`Edited` suggestions' behavior - the
  rejection-echo lookup only ever fires when no `Pending` suggestion
  exists *and* the most recent suggestion for that reference is
  specifically `Rejected`. An `Approved`/`Edited` repeat is left exactly
  as silently absorbed as it already was before this phase (the operator
  already has what they wanted on screen; no feedback signal is missing
  there).
- Zero changes to any existing command's signature.
- `SuggestionStats.rejection_echoes` is a pure sum over already-persisted
  data, computed fresh on every `get_service_report` call - no new write
  path, no cached/stale value possible.

## Environment A / B / C

- **Environment A** (this container): PASSED, fully green, as detailed
  above - including a real acceptance test
  (`a_repeated_paraphrase_after_rejection_echoes_instead_of_vanishing_silently`)
  exercising `handle_final_transcript` twice against the real dev-seed
  BSB/KJV dataset: reject a paraphrase suggestion, repeat the identical
  wording, and confirm the echo count rises while the suggestion stays
  `Rejected` and no second suggestion is ever created.
- **Environment B** (Xvfb GUI reproduction): unavailable in this
  session's container, a pre-existing, already-documented limitation
  since Phase 3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, a real rejected-then-repeated
  detection): NOT YET VERIFIED. The decisive pending gate is the
  operator's own real-hardware test: reject a Paraphrase/Semantic
  suggestion, then speak (or replay) the same wording again within about
  a minute, and confirm the Needs Attention queue does *not* show it
  again, while History's Service Report and Scripture & Findings list
  show the echo count rising.

## Known limitations

- **Scoped to `Paraphrase`/`Semantic` only, matching Phase 5.2's own
  precedent** - a rejected explicit citation's repeat is unaffected by
  this phase (dedup already suppresses it exactly as before, with no echo
  tracking added).
- **Only the most recent `Rejected` suggestion for a reference is ever
  echoed** - if an operator rejects the same reference on two separate,
  non-adjacent occasions in one service, only the most recent rejection's
  counter ever increments for a redetection.
- **The echo only ever fires within the existing 60-second dedup
  window** - a genuinely repeated false-positive minutes after a reject
  (past the window) is treated as a brand-new, unconfirmed suggestion,
  exactly as dedup already treated every other repeat before this phase.
- **No UI surface in the live Needs Attention queue** - a rejected
  suggestion is filtered out of the live queue entirely
  (`LiveChurchBrain.tsx`'s existing `onSuggestionRejected` handler), so
  the echo count is only ever visible in History's post-service view, not
  during the live service itself.
- **No disconfirmation of a `Pending` suggestion** - this phase does not
  touch Phase 5.2's own documented gap (a later segment's wording
  contradicting an earlier heuristic guess still has no mechanism to
  lower a still-`Pending` suggestion's confidence). Rejection-echo
  tracking is a distinct, narrower mechanism: it only ever applies once
  an operator has already made an explicit decision.
- **`rejection_echo_count` is an honest occurrence count, never a proxy
  for anything else** - it never feeds back into confidence scoring,
  never auto-rejects a future suggestion, and never changes detection
  behavior in any way. It is observability only.
- **Every limitation already documented for Phase 4.1's paraphrase
  detector, Phase 4.4's semantic search, Phase 5.1's process-lifetime
  diagnostics, Phase 5.2's temporal confirmation, and Phase 5.3's VAD
  still applies unchanged** - this phase adds a further observability
  refinement, it does not revisit or resolve any of them.

## Deferred work

- Surfacing rejection-echo state in the live Needs Attention queue or
  diagnostics-mode Pending Suggestions panel, not just History's
  post-service view.
- A genuine disconfirmation mechanism for still-`Pending` suggestions
  (Phase 5.2's own deferred item, not attempted here either).
- Tracking rejection echoes across *all* of an operator's rejections for
  a reference in a service, not just the most recent one.
- Real-hardware Environment C verification against an actual
  rejected-then-repeated detection in a live or replayed service.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, a genuine rejected-then-repeated detection, both outside this
container's reach). This phase is a real, verifiable, fully-tested,
purely additive reliability refinement - it never creates a new
suggestion, never mutates a decided one, and never changes dedup's own
suppression behavior; it only makes an already-existing suppression
honestly observable.
