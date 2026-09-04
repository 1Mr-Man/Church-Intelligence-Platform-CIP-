# Phase 25: Session Black Box (Downloadable Post-Test Report)

## Trigger

The operator's own request, verbatim: "Can it have a 'blackbox', like
after testing I should be able to download file that will report
everything that happens during the testing period for you to see what
really happened, for example I tested it a 1 hour message, I will give
you the transcript and the report from CIP for the 1 hours test so that
you will understand fully what happened and the necessary next steps. So
that you'll not be guessing when I give a half report."

## The gap

Every diagnostic surface CIP had before this phase was either **live and
ephemeral** (`get_pilot_diagnostics`, polled while the app is running -
gone the moment the operator closes CIP or asks a question afterward) or
a **summary of counts** (`get_service_report`'s `SuggestionStats`,
`DetectionKindCount`, `TimelineCategoryCount` - "12 suggestions, 3
errors" with no way to see which suggestions or what the errors actually
said). Nothing let an operator capture the *raw, complete* record of one
service - transcript text, individual timeline events, individual
suggestions, individual corrections - into a single artifact they could
hand back after the fact. A verbal "it mostly worked but there were a
few glitches" report forces a diagnosis to guess; this phase gives the
operator a file that removes the guessing.

## Design decision: extend and bundle, not reinvent

`docs/phase-5-1-*`'s own `get_service_report` (`service_report.rs`)
already aggregates almost everything relevant for one service - this
phase does **not** duplicate that logic. Instead:

1. `service_report.rs`'s `LiveDiagnosticsSnapshot` gains the fields it
   was honestly missing for a diagnosis to be complete: the quality
   tier's own counters (Phase 24.3/24.3.2 - `jobs_submitted`,
   `jobs_dropped_backlog`, `jobs_completed`,
   `consecutive_jobs_dropped`, `model_loaded`, `last_error`) and the
   real error text both `SpeechDiagnostics.last_error` and
   `AppState.audio_error` already track but `build_service_report`
   never surfaced. `build_service_report` gained two new parameters
   (`speech_quality_diagnostics`, `audio_error`) to carry them in - a
   backwards-incompatible signature change to an existing, tested
   function, deliberately: this is the one place in the codebase that
   already answers "what happened in this service," and it should not
   silently miss a whole diagnostic tier. `get_service_report` (the
   pre-existing command) picks these up automatically - it now reports
   quality-tier and audio-error state too, for free.
2. A new module, `session_report.rs`, composes `build_service_report`
   wholesale plus the **full, unsummarized** history a real diagnosis
   needs: every transcript segment (`persistence::list_transcript_segments`
   with `u32::MAX`, not the operator-facing `limit` `list_transcript`
   uses), the full timeline (`timeline::list_timeline`, same
   unbounded read - every individual event, not just per-category
   counts), every suggestion (`persistence::list_suggestions`), and
   every quality-tier correction, both texts included (a new
   `persistence::list_transcript_corrections`, joining
   `transcript_corrections` through `transcript_segments` since that
   link table itself carries no `service_id` column).
3. `session_report.rs` also composes a `human_summary`: a short,
   plain-text paragraph built entirely from the data already gathered
   (duration, segment count, suggestion outcome breakdown, detection
   kinds, timeline error count, correction count, live-diagnostics
   highlights including the last speech/quality/audio error text) - so
   the operator can paste a few sentences directly into a chat message
   alongside the full JSON, without either side needing to open the
   file to get the gist.
4. A new command, `commands::export_session_report(service_id,
   destination_dir)`, mirrors `backup_database`'s own exact pattern
   (`destination_dir` created if needed, a timestamped filename,
   `{ report_path, size_bytes }` returned) - writes the whole
   `SessionReport` as one pretty-printed JSON file,
   `cip-session-report-<short-id>-<timestamp>.json`.
5. Frontend: `HistoryView`'s existing "Service Report" section (the
   only place `getServiceReport` was already displayed) gained an
   "Export Session Report (Black Box)" button, using the same native
   folder-picker (`@tauri-apps/plugin-dialog`'s `open({directory:
   true})`) `PilotDiagnosticsPanel`'s model-file pickers already
   established.

No new migration: `transcript_corrections` (Phase 24.3, migration
0019) already has everything `list_transcript_corrections` needs.

## What changed

- `apps/desktop/src-tauri/src/service_report.rs`: `LiveDiagnosticsSnapshot`
  gained 9 new fields (`speech_last_error`, `audio_last_error`, and 7
  quality-tier fields); `build_service_report` gained
  `speech_quality_diagnostics: &SpeechQualityDiagnostics` and
  `audio_error: Option<String>` parameters. One new test
  (`live_diagnostics_carries_quality_tier_and_audio_error_fields`); all
  9 existing tests updated for the new signature.
- `apps/desktop/src-tauri/src/persistence.rs`: new `TranscriptCorrection`
  struct (id, both segment ids, both texts, `created_at`) and
  `list_transcript_corrections`, scoped to its own service via an inner
  join through `transcript_segments`. Two new tests.
- `apps/desktop/src-tauri/src/session_report.rs` (new module):
  `SessionReport` struct, `build_session_report` (pure aggregation, no
  `AppHandle`/Tauri dependency - real in-memory-database testable), and
  `build_human_summary` (pure text composition). Five new tests.
- `apps/desktop/src-tauri/src/commands.rs`: `SessionReportExport` struct
  and `export_session_report` command; `get_service_report`'s own call
  site updated for `build_service_report`'s new parameters.
- `apps/desktop/src-tauri/src/lib.rs`: registers `mod session_report;`
  and the new command.
- Frontend: `config/appConfig.ts` (`SessionReportExport`),
  `domain/service.ts` (`LiveDiagnosticsSnapshot`'s 9 new fields),
  `lib/commands.ts` (`exportSessionReport`),
  `components/library/HistoryView.tsx` (the export button + folder
  picker + notice), `lib/commands.test.ts` (2 new tests).

## Why this is safe

`export_session_report` is purely additive - a new command, reading
already-persisted rows the same way `get_service_report`/`list_transcript`/
`list_timeline`/`list_suggestions` already do, writing a new file to a
directory the operator chooses. It never mutates any table, never
changes any existing command's behavior for a caller that doesn't pass
the two new `build_service_report` parameters incorrectly (the
compiler enforces every call site updates, and there is exactly one:
`get_service_report`). The quality/audio-error fields it surfaces were
already being tracked in `AppState` before this phase - this only makes
them readable, it adds no new tracking logic of its own.

## Testing boundary

`build_session_report`/`build_human_summary` are both directly unit
tested against a real in-memory SQLite database (no `AppHandle`, no
Tauri runtime needed - see `session_report.rs`'s own module docs for
why that boundary was deliberately kept). `export_session_report`
itself (the thin `#[tauri::command]` wrapper: directory creation, JSON
serialization, `fs::write`) is not directly unit tested, matching this
project's own established precedent (`commands.rs`'s test-module docs)
for any command taking `State<'_, AppState>` - `tauri::test::mock_builder()`
would require a signature change across the whole module for a
test-only concern. The same precedent already applies to
`backup_database`, which this command's shape deliberately mirrors.

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets
-- -D warnings` clean, and again with `--features whisper` on the
desktop crate - clean. `cargo test --workspace` 381/381 `cip-desktop`
unit tests in both feature configs (373 existing + 8 new: 2
`list_transcript_corrections`, 1 `service_report.rs` quality/audio-error
test, 5 `session_report.rs`). Frontend: `npm run typecheck` 0 errors,
`npm run lint` the same 4 pre-existing warnings (unchanged), `npm run
test -- --run` 305/305 (up from 303 - the 2 new `exportSessionReport`
command tests), `npm run build` clean.

## Final gate

Environment A (fmt/clippy/test in both feature configs, frontend
typecheck/lint/test/build): PASS. Environment B (Xvfb smoke test): not
re-run this phase - no display/presentation-pipeline code touched, only
a new export command and an existing report's own diagnostic
completeness. Environment C (real Windows hardware, a real operator
downloading and opening a real exported report) has not been performed -
this is the exact gap the operator's own request names as still open
this session, and this phase's own artifact is what the operator would
use to close it: run a real test, download the black box, hand it back.

## Known limitations (honest, not deferred silently)

- **`SessionReport.suggestions` includes every suggestion ever created
  for the service, at whatever status it currently holds** - a
  suggestion approved, then later independently re-evaluated by nothing
  in this codebase (suggestions are never re-scored after their initial
  creation), so this is simply the final, settled state of each one.
  Not a limitation of correctness, just worth naming: the export is a
  snapshot at export time, not a change-log of every status transition
  a suggestion went through (that history, where it happened, is
  already in `timeline` as separate `SUGGESTION_APPROVED`/`_REJECTED`/
  `_EDITED` events with their own timestamps).
- **No periodic time-series of diagnostic counters.** The live
  diagnostics half of the report (`summary.live_diagnostics`) is one
  snapshot taken at export time, honestly labeled "since app launch, not
  this service alone" (exactly like `get_service_report`'s own existing
  disclaimer). It cannot show "the quality tier was overloaded from
  10:15 to 10:20 specifically" - only cumulative counts as of export
  time. `timeline`'s own individual events (including every
  `ERROR_OCCURRED`, each with its own timestamp) are the closest thing
  to a time-series this phase provides. A true periodic-snapshot
  recorder (a background thread persisting a diagnostics row every N
  seconds while listening is active) was considered and deliberately
  scoped out - a real, separate subsystem (new table, new thread
  lifecycle tied to `start_listening`/`stop_listening`) that would
  roughly double this phase's size for a gap the existing per-event
  timeline already substantially covers.
- **No frontend viewer for the exported JSON.** The operator opens the
  file in a text editor (or hands it directly to a diagnosis) - there is
  no in-app "browse a past export" screen. Given the file's purpose
  (leave the app, come back with it later), this was judged the right
  scope for a first version.
