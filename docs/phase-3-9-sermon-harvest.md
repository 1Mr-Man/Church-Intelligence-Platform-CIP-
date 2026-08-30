# Phase 3.9 — Sermon Harvest

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `af7223a` (Phase 3.8.7.8)

## Why this phase exists

The user shared the project's original master vision document (the same
specification this entire codebase has been built against, phase by
phase, since Phase 0) and asked to continue building out what remained.
That document is large - it spans dozens of major feature areas, many
already substantially implemented after 300+ prior phases (Bible
Intelligence, Sermon Intelligence's 19-kind structural taxonomy, Service
Phase detection, offline-first SQLite architecture, the Live Intelligence
Router). A gap analysis against the real, current codebase (not the
document's own aspirational framing) identified five genuinely unbuilt
pillars: multi-screen presentation output, semantic/paraphrase Bible
detection, real audio fingerprinting, multi-language support, and a
"Sermon Harvest" one-click export. The user asked for all five; this
phase delivers the first, in keeping with this project's own established
discipline of one real, fully-verified phase at a time rather than
unscoped, unreviewable sprawl across many subsystems at once.

Sermon Harvest was sequenced first because it required zero new
dependencies, zero new detection logic, and zero new AI - it only
assembles data this application already captures through its existing,
separately-tested pipelines into one bundle.

## Design

`crate::harvest`'s own module docs state the constraint plainly: this
module contains no detection logic of its own. It does not classify
text, does not invent a title/summary when one is absent, and runs no new
pass over the transcript. `harvest_sermon(conn, findings, sermon)` reads
back:

- **Sections** - `persistence::list_sermon_sections`, unchanged.
- **Elements** - every `IntelligenceFinding` with `domain: Sermon` and
  `sermon_id == Some(sermon.id)` from the in-memory `FindingQueue`,
  excluding `Rejected` (an operator explicitly said it was wrong) but
  keeping `Detected`/`Reviewed`/`Accepted` - harvesting only accepted
  findings would silently drop everything the operator hadn't gotten
  around to reviewing yet.
- **Scripture** - every `Suggestion` for the sermon's service, any status.
- **Transcript** - the full transcript for the sermon's service, oldest
  first, bounded at 5,000 segments (generous - a 3-hour service at
  ~15-18s logical segments is well under 1,000).
- **Timeline** - the service's audit timeline, oldest first, same bound.

### The one real scope boundary, stated honestly

`IntelligenceFinding::sermon_id` linkage lives only in the in-memory
`FindingQueue` - never persisted to a database, by the same deliberate
Phase 2.0 design `docs/phase-3-8-7-6-live-intelligence-integration-audit.md`
already documented for every domain's findings. This means Harvest only
ever works for the **currently active** sermon, in the **same app
session** it was delivered in. A restart between the sermon ending and
running Harvest would silently produce an empty `elements` list if this
weren't handled - so `harvest_sermon` (the Tauri command) calls
`active_sermon_or_error`, the same guard every other active-sermon-scoped
command already uses, and refuses cleanly instead of returning a
misleadingly incomplete bundle. Harvesting a sermon from a past session
remains a real future need (the underlying `sections`/`scripture`/
`transcript`/`timeline` data all HAVE been persisted and are already
retrievable via `get_sermon` + the corresponding list commands) - not
attempted this phase, since it would require either persisting
`IntelligenceFinding` (a real, separate architectural decision this
phase does not make unilaterally) or accepting a `elements: []` gap that
must be surfaced honestly rather than hidden.

## Fix applied

- **New module** `apps/desktop/src-tauri/src/harvest.rs` - the pure
  aggregation function and the `SermonHarvest` struct, plus three unit
  tests (assembly correctness, no-title-fabrication, keeps-not-only-
  accepted-findings).
- **New Tauri command** `harvest_sermon` (`commands.rs`), registered in
  `lib.rs`. Records a `SermonHarvested` timeline entry and emits a
  `SermonHarvested` event (new `AppEvent` variant) alongside returning
  the bundle - mirrors every other explicit operator action in this
  codebase.
- **Frontend**: `SermonHarvest` type (`domain/sermon.ts`), `harvestSermon()`
  wrapper (`lib/commands.ts`), and a new "Sermon Harvest" panel in
  `LiveChurchBrain.tsx` - a single button (disabled when no sermon is
  active) that fetches and displays the bundle in five collapsible
  sections (Sections/Elements/Scripture/Transcript/Timeline).

No new database migration, no new persistence, no new detection engine,
no new dependency.

## Full regression result

Backend: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
(both default and `--features whisper`): clean. `cargo test -p cip-desktop`
(default feature config): 251/251 passed (up from 248 - 3 new harvest
tests). Frontend: `tsc -b` (0 errors), `oxlint` (0 errors, same 4
pre-existing warnings), `vitest` 210/210 passed (one existing assertion
in `eventNames.test.ts` updated for the new 54th event), `vite build`
clean.

## Windows artifact

Rebuilt with `scripts/build-windows-whisper.sh`; DLL bundling, whisper
feature, and every prior phase's fix re-verified present via direct
`x86_64-w64-mingw32-strings` inspection of the extracted binary - see
`pilot-evidence/3.9/`.

## Known limitations

- Harvest only ever covers the currently active sermon, in the same app
  session - see the scope-boundary section above. A past-session harvest
  is a real, separate future phase (either persist `IntelligenceFinding`,
  or ship a version that honestly reports `elements: []` for anything
  before the current session).
- The UI presents the bundle inline; it does not yet export to a file
  (PDF/text/markdown) - the vision document's "one click produces
  [...]" framing implies an export artifact, which this phase does not
  attempt. The data is fully present in the response; only the
  save-to-file step remains.
- No new grouping/labeling of `elements` by `SermonElementKind` (Points/
  Quotes/Illustrations/Prayer Points/etc.) - that taxonomy is currently
  only encoded as a text prefix in each finding's own `summary` field
  (e.g. "Prayer Point: ..."), not a structured field on
  `IntelligenceFinding` itself. Re-deriving it via string-prefix parsing
  would be fragile and was deliberately not attempted; promoting element
  kind to a first-class field is a separate, real decision for a future
  phase.
- Multi-screen presentation output, semantic/paraphrase Bible detection,
  real audio fingerprinting, and multi-language support - the other four
  pillars the user asked for - remain **not started**. This phase
  delivers one of five, honestly, rather than claiming completion of a
  scope this size in a single pass.

## Final gate

| Item | Status |
|---|---|
| Real existing data sources traced before designing the bundle | DONE |
| Zero new detection/generation logic - pure aggregation | DONE |
| Scope boundary (active-sermon-only) identified and honestly enforced, not hidden | DONE |
| Full regression green (backend + frontend) | DONE |
| Windows artifact rebuilt + verified | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.9: Environment A verification PASS.** Real Windows re-test
(Environment C) is the pending, decisive gate, per this project's
standing discipline.
