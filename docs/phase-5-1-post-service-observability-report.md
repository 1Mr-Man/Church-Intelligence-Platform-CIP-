# Phase 5.1 — Post-Service Observability Report

## Baseline

The operator's own roadmap synthesis (cross-referencing
`docs/phase-4-master-plan-gap-audit.md` against the "Advice" competitive
critique) named "Reliability & Trust" as the Phase 5 theme, with a
post-service summary as its smallest, lowest-risk, immediately-useful
slice: every fact this report shows already existed and already survived
a restart before this phase (`Suggestion`s, `scripture_detections` rows,
`audit_events`, `SpeechDiagnostics`/`EmbeddingDiagnostics`) - nothing new
is detected, scored, or persisted. This mirrors exactly how Phase 3.9
(Sermon Harvest) was scoped: "no new detection logic, no new AI, no new
persistence," a pure read-side aggregation of data the app already
captures.

## Why this phase exists

An operator finishing a live service has no single place to see how it
went: suggestion approval/rejection counts, what kinds of detections
fired, whether any errors were logged, and whether the speech/semantic
pipeline behaved as expected are each only visible by opening separate
panels (`HistoryView`'s existing suggestions/timeline/transcript
sections, or the always-running `PilotDiagnosticsPanel`). A single
report closes that gap without inventing any new signal.

## Architecture decisions

- **Pure aggregation, mirroring `harvest.rs`**: `service_report.rs`
  contains no detection logic. `build_service_report` only calls
  already-existing `persistence`/`timeline` read functions and reads
  `AppState`'s already-existing diagnostics structs.
- **Two new `GROUP BY` count queries, not row dumps**:
  `persistence::scripture_detection_kind_counts` and
  `timeline::count_events_by_category` return `(String, u64)` pairs, not
  full row lists - a long service's detections/timeline can run into the
  hundreds or thousands of rows, and the report only ever needs totals.
- **`LiveDiagnosticsSnapshot` is honestly labeled "since app launch," not
  service-scoped**: `SpeechDiagnostics`/`EmbeddingDiagnostics` are
  `AppState` fields that accumulate across every `start_listening` call
  in one run of the app, never reset per service (see `state.rs`'s own
  docs on those fields). A report claiming "this service's average
  inference duration was Xms" would misrepresent a process-lifetime
  figure as service-scoped precision this codebase does not have -
  exactly the kind of inflated software-only reading the project's
  standing Environment A/B/C discipline (`docs/phase-3-2-hardware-pilot.md`)
  guards against. Every field name and this module's own doc comment say
  "since app launch."
- **A report failure never blocks the rest of History**: `HistoryView`
  fetches the report in a separate `.then()`/`.catch()` from the existing
  `Promise.all` for timeline/transcript/suggestions/presentations/saved
  content - a report error surfaces without hiding data that already
  worked.

## What was built

- **`apps/desktop/src-tauri/src/persistence.rs`**:
  `scripture_detection_kind_counts(conn, service_id)` - one `GROUP BY
  detection_type` query over `scripture_detections`.
- **`apps/desktop/src-tauri/src/timeline.rs`**:
  `count_events_by_category(conn, service_id)` - one `GROUP BY category`
  query over `audit_events`.
- **`apps/desktop/src-tauri/src/service_report.rs`** (new module):
  - `SuggestionStats { total, pending, approved, edited, rejected }`.
  - `DetectionKindCount { kind, count }` / `TimelineCategoryCount {
    category, count }`.
  - `LiveDiagnosticsSnapshot` - 15 fields covering speech/embedding
    feature-compiled, model-loaded, chunk/inference counters, queue/
    overload counters, and `avg_inference_duration_ms` (derived
    identically to `commands::get_pilot_diagnostics`'s own existing
    derivation, never allowed to drift from it).
  - `ServiceReport { service, duration_minutes, suggestion_stats,
    detection_kind_counts, timeline_category_counts, live_diagnostics,
    generated_at }` - `duration_minutes` is `None` while the service is
    still active (no guessed duration for an in-progress service).
  - `build_service_report(conn, service_id, speech_diagnostics,
    embedding_diagnostics, embedding_ready)` - the sole entry point.
- **`apps/desktop/src-tauri/src/commands.rs`**: `get_service_report`
  Tauri command - locks `state.db`, clones `state.speech_diagnostics`/
  `state.embedding_diagnostics`, reads `state.embedding_ready`, calls
  `build_service_report`.
- **`apps/desktop/src-tauri/src/lib.rs`**: registered `mod
  service_report;` and `commands::get_service_report` in
  `tauri::generate_handler!`.
- **Frontend**: `domain/service.ts` gained `SuggestionStats`,
  `DetectionKindCount`, `TimelineCategoryCount`,
  `LiveDiagnosticsSnapshot`, `ServiceReport` (camelCase mirrors of the
  Rust structs); `lib/commands.ts` gained `getServiceReport(serviceId)`;
  `HistoryView.tsx` fetches the report when a past service is opened and
  renders a "Service Report" panel (duration, suggestion breakdown,
  detection-kind counts, timeline-category counts, and a collapsed "Live
  pipeline diagnostics (since app launch, not this service alone)"
  section carrying the same disclaimer as the backend doc comments).

## Full regression result

`cargo fmt --check`: clean. `cargo clippy --workspace --all-targets --
-D warnings`: clean under both default features and `--features
whisper,semantic-search`. `cargo test --workspace` (single-threaded, to
avoid a pre-existing, unrelated `config.rs` env-var test-parallelism
flake - see "Known limitations"): every crate green under both feature
configurations; `cip-desktop` alone gained 8 new tests, all passing (all in
`service_report.rs`'s own `#[cfg(test)] mod tests`, exercising
suggestion-status breakdown, detection-kind breakdown, active-vs-ended
duration, the `avg_inference_duration_ms` derivation matching
`get_pilot_diagnostics`'s own, timeline-category counts, and
service-scoping isolation). Frontend: `npm run typecheck` clean, `npm
run lint` clean (no new warnings), `npm run test` 220/220 passing (218
pre-existing + 2 new `commands.test.ts` cases for the Tauri-runtime
guard on `getServiceReport`), `npm run build` succeeds.

## Windows rebuild

No new native dependency was introduced - `service_report.rs` uses only
`rusqlite`/`chrono`/`serde`, all already linked. `scripts/build-windows-whisper.sh`
was re-run unchanged (`--features whisper,semantic-search`, same as
Phase 4.4) to confirm the new command compiles and links into the cross-
compiled artifact. Installer: `Church Intelligence Platform_0.1.0_x64-setup.exe`,
SHA-256 `356e69df388e80b1a3de2e86b6b8344f8ce18a68f7c5575ccfc63aca54038b8b`,
13,738,055 bytes (essentially unchanged from the Phase 4.4 baseline of
13,738,939 bytes - expected, since this phase adds no new dependency).
See `pilot-evidence/5.1/windows/installer-contents-verification.json`
for direct binary proof (new `get_service_report` symbol present, prior-
phase symbols confirmed unaffected).

## Architectural safety diff

- Zero changes to any existing command's signature or behavior - `get_service_report`
  is a wholly new, additive command.
- Zero changes to any existing database table, column, or write path -
  both new persistence functions are read-only `SELECT ... GROUP BY`
  queries against tables that already existed (`scripture_detections`,
  `audit_events`).
- Zero changes to `SpeechDiagnostics`/`EmbeddingDiagnostics`'s own
  fields or update sites - `service_report.rs` only reads a cloned
  snapshot.
- Zero changes to `HistoryView`'s existing `Promise.all` fetch or its
  existing sections - the report fetch is a wholly separate, additive
  `.then()`/`.catch()` that cannot block or fail the rest of the view.

## Environment A / B / C

- **Environment A** (this container: compile, lint, unit/integration
  tests, direct binary inspection): PASSED, fully green, as detailed
  above.
- **Environment B** (Xvfb GUI reproduction): unavailable in this
  session's container, a pre-existing, already-documented limitation
  since Phase 3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, real completed service
  data): NOT YET VERIFIED. This is read-side-only aggregation of data
  formats already exercised on real hardware in earlier phases (real
  suggestions, real detections, real timeline entries), so the residual
  risk is small, but the report has never been rendered against a real
  operator's completed service.

## Known limitations

- **Pre-existing, unrelated test flake**: `config::tests::whisper_model_path_honors_the_env_override`
  fails intermittently under the default parallel test runner because it
  mutates a process-wide environment variable that another test reads
  concurrently; it passes in isolation and under `--test-threads=1`. This
  predates Phase 5.1 (no file it touches, `config.rs`, was changed this
  phase) and is not fixed here - documented rather than silently ignored.
- **`LiveDiagnosticsSnapshot` is process-lifetime, not service-scoped**
  by construction (see "Architecture decisions" above) - an operator
  running two back-to-back services in one app session will see the same
  cumulative speech/embedding counters in both reports' diagnostics
  section. The suggestion/detection/timeline half of the report has no
  such limitation - those are always correctly scoped to the one service.
- **No report caching** - `get_service_report` re-runs its aggregation
  queries on every call; acceptable given a single service's row counts,
  but not something to call in a tight polling loop.
- **No export** - the report is view-only; printing/exporting it to a
  file is a natural, low-effort follow-up, not attempted this phase.

## Deferred work

- Report export (PDF/text) for sharing outside the app.
- A per-service reset or separate accumulator for
  `SpeechDiagnostics`/`EmbeddingDiagnostics` so the live-diagnostics half
  of the report could honestly become service-scoped (a larger change:
  `AppState`'s diagnostics fields would need to move from "since launch"
  to "since service start," touching every existing read site, not just
  this report).
- Real-hardware Environment C verification against an actual completed
  service.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase is a real,
verifiable, fully-tested, purely additive read-side feature - it
introduces no new detection, scoring, or persisted state, and changes no
existing command's behavior.
