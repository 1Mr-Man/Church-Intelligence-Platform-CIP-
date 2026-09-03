# Phase 17: Detection Accuracy Analytics

## Trigger

No new user-reported bug this phase - proceeding on "Keep going, start Phase 17" as an
autonomous continuation. Rather than guess at a feature, this phase re-audited the master
plan's own gap list (`docs/phase-4-master-plan-gap-audit.md`) for an item still genuinely
unstarted and directly relevant to the operator's own most recent, real feedback. It names one
explicitly: *"Analytics - NOT STARTED - No usage/accuracy-metrics dashboard exists (the master
plan's own section 65 'AI Accuracy Philosophy' calls for measuring detection
accuracy/false-positive rate/correction rate empirically - none of that is instrumented)."*
That gap connects directly to the operator's own Phase 15-triggering report ("accuracy fair")
and to Phase 15's own honest limitation ("this exact artifact has not yet been re-verified
against a second real pilot session confirming the fixes actually resolve the reported
symptoms") - without any instrumentation, no one (operator or developer) can actually see
whether Phase 14/15's fixes are working, only hope so.

## What was audited before building anything

Confirmed this codebase already persists everything a real accuracy dashboard needs, and has
since Phase 1.3 - it was simply never surfaced:

- `ai_suggestions.status` (`pending`/`approved`/`edited`/`rejected`) - every operator decision
  on a Bible suggestion, ever made.
- `ai_suggestions.confidence_score`/`.confidence_level` - what CIP itself believed at the time.
- `ai_suggestions.rejection_echo_count` (Phase 5.4) - how often a rejected reference kept
  resurfacing and was suppressed rather than resurrected.
- `scripture_detections.detection_type` (`DIRECT_REFERENCE`/`PARAPHRASE_REFERENCE`/
  `SEMANTIC_REFERENCE`/etc.) - which detection method produced each detection, persisted for
  *every* detection, not just ones that became a suggestion.

Traced `pipeline.rs::persist_detections_and_suggestions` directly and confirmed a reliable join
key already exists between a suggestion and the detection that produced it: both are persisted
in the same function call, with the suggestion's `transcript_segment_id` set to the exact same
`segment_id` used to persist its originating `scripture_detections` row - true for both the
raw-window path and the Phase 15 fuller-context retry path. No new column, no schema change,
was needed to correlate "what detection method produced this suggestion" with "what happened
to it."

Also confirmed only the Bible domain has this complete accept/edit/reject history persisted -
Music and Sermon/Content route through `IntelligenceFinding`/`FindingQueue`, which (outside
Phase 13's operator-*accepted*-only `saved_sermon_findings` and Phase 2.7.1's accepted-only
`saved_content_candidates`) never durably records a *rejected* item, so no rejection-rate can
be computed for those domains today. This phase is scoped to Bible detection accuracy only,
honestly, rather than fabricating a cross-domain metric the data can't support.

## What was built

- **`core/service` / `core/ai`**: unchanged - this phase adds no new detection logic, no new
  AI, and touches no domain contract crate.
- **`apps/desktop/src-tauri/src/persistence.rs`**: two new read functions,
  `list_all_suggestions` and `list_all_scripture_detections_with_segment` - both plain,
  bounded reads across every service (mirroring `sermon_knowledge_base.rs`'s own
  cross-service precedent), never scoped to one `service_id` like their existing
  `list_suggestions`/`scripture_detection_kind_counts` counterparts.
- **`apps/desktop/src-tauri/src/bible_detection_analytics.rs`** (new): `OutcomeCounts` (total/
  pending/approved/edited/rejected, plus `approval_rate()`/`correction_rate()`), and
  `build_bible_detection_analytics` - a pure aggregation producing:
  - Overall counts, approval rate, correction rate, and total rejection echoes, across every
    service.
  - A breakdown by confidence level (low/medium/high, always all three, even at zero) - does
    higher confidence actually correlate with fewer corrections?
  - A breakdown by detection method (citation/paraphrase/semantic/etc.), correlated to outcome
    via the join key described above - directly answers whether paraphrase/semantic
    suggestions (the ones Phase 15 targeted) are approved as often as citations, or corrected
    more.
  - A per-service trend, oldest first - is accuracy improving service to service as fixes ship.
  - An honestly-reported `unmatchedDetectionKindCount` for any suggestion that can't be
    correlated (e.g. a manual context-correction suggestion with no originating transcript
    segment) - counted in `overall`, never silently dropped or guessed into the wrong bucket.
- **`apps/desktop/src-tauri/src/commands.rs`/`lib.rs`**: new `get_bible_detection_analytics`
  command, open to any operator (a read of already-decided history, no more sensitive than the
  Scripture & Findings list every service report already shows).
- **Frontend**: `domain/service.ts` gains the mirrored TS types; `lib/commands.ts` gains
  `getBibleDetectionAnalytics`; `HistoryView.tsx` gains a new "Detection Accuracy" section
  (loaded once, spanning every service, exactly like the existing Church Knowledge Base
  section right below it) showing overall/by-confidence/by-detection-method breakdowns and a
  collapsible per-service trend.

## Explicitly deferred

- No threshold was changed. `MIN_PARAPHRASE_SCORE`/`MIN_SEMANTIC_SIMILARITY` (Phase 4.1/4.4)
  remain exactly as they were - this phase makes the *evidence* for a future calibration
  decision visible; it does not make that decision itself, since there still isn't a real
  pilot session's worth of accepted/rejected data to calibrate against.
- No Music/Sermon/Content accuracy metric - see the audit section above for why the data
  doesn't support one honestly today. A future phase could add durable rejection tracking for
  those domains if this pattern proves useful for Bible detection.
- No visualization/chart library - the breakdown is plain text/lists, consistent with every
  other report this codebase has ever shipped (`ServiceReport`, `SermonKnowledgeBase`).

## Testing boundary

`OutcomeCounts`/`build_bible_detection_analytics` are pure and fully unit-tested: 9 new Rust
tests in `bible_detection_analytics.rs` covering cross-service aggregation, the `None`
approval-rate case (nothing decided / zero suggestions), the fixed three-level confidence
ordering, the detection-kind correlation (including a real FK-backed transcript segment, not a
synthetic id), the honest "unmatched" fallback for a suggestion with no originating segment,
rejection-echo summing across services, and the chronological, per-service trend ordering. The
two new `persistence.rs` functions are exercised indirectly through those same tests (no
separate unit tests, mirroring how `list_suggestions`/`scripture_detection_kind_counts`
themselves have never had dedicated tests either - they're proven through their callers). The
frontend rendering is thin display logic over an already-tested value, mirroring
`ServiceReport`'s and `SermonKnowledgeBase`'s own precedent of no dedicated frontend test for
the display itself.

## Full regression result

- Backend: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean
  (default and `--features whisper`), `cargo test --workspace` clean (cip-core-service
  unaffected; cip-desktop 359/359, up from 350 - the 9 new tests - unchanged in both feature
  configs).
- Frontend: `npm run typecheck` 0 errors, `npm run lint` same 5 pre-existing warnings
  (unchanged), `npm run test -- --run` 298/298 (unchanged - no new frontend tests, consistent
  with the testing-boundary note above), `npm run build` clean.

## Architectural safety

- Zero new Tauri commands beyond the one read-only `get_bible_detection_analytics`, zero new
  events, zero new migrations, zero schema changes.
- `build_bible_detection_analytics` only ever reads `ai_suggestions`/`scripture_detections`/
  `services` - it writes nothing, and cannot affect detection, persistence, or presentation in
  any way.
- The detection-method correlation is read-only best-effort (first match wins for a
  theoretically-possible but unobserved same-segment-same-reference collision) - it can only
  ever undercount into `unmatchedDetectionKindCount`, never misattribute a suggestion's outcome
  to the wrong detection method's bucket in a way that inflates a rate.

## Known limitations (honest, not deferred silently)

- The accuracy figures are only as informative as the operator behavior behind them - this
  container has no real pilot session's worth of decisions to show yet; the dashboard will
  read "Nothing decided yet" until a real operator approves/edits/rejects suggestions during
  real services.
- Music/Sermon/Content accuracy is not covered - see the audit section above.
- The per-service trend list has no chart, just a chronological list - a future phase could
  add a real visualization once there's enough real history to make one worth building.
- This exact rebuilt artifact has not yet been installed or launched on real Windows hardware -
  see `physicalHardwareStatement` item 26 in the updated release manifest.

## Final gate

Environment A (typecheck/lint/test/build, direct binary symbol inspection): PASS. Environment C
(a real operator reviewing this dashboard against a real service history and confirming the
breakdown matches what they actually did): not yet performed - carried forward into
`physicalHardwareStatement`.
