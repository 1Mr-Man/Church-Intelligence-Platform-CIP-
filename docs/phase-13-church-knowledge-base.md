# Phase 13: Church Knowledge Base / Cross-Sermon Analytics

## Baseline

Trigger: the last remaining item from the user's own pasted advice list
("nothing aggregates it across services yet"), followed by the explicit
instruction "church knowledge base / cross-sermon analytics." Full
reasoning in `docs/phase-13-audit.md`, including the direct-evidence
verification this phase started with (confirming that `IntelligenceFinding`
- Sermon domain included - was never persisted anywhere, only held in
`AppState.intelligence_findings: Mutex<FindingQueue>`, constructed once at
startup and never cleared, which made genuine cross-restart, cross-service
analytics impossible rather than merely "not yet aggregated").

## Design choices

See `docs/phase-13-audit.md` in full. Summary: a new `saved_sermon_findings`
table, written only at the moment `accept_sermon_finding` succeeds
(mirroring Phase 2.7.1's `saved_content_candidates`/
`accept_content_candidate` exactly - an explicit, operator-initiated
"accept," never an automatic write on detection). A new read-only
aggregation module reads that table plus every persisted `sermons` row
and assembles theme frequency (most-preached themes, with the sermons
each was attached to), per-speaker sermon history, and a recent-findings
feed - all spanning every service, not just the one currently active, and
surviving a restart. `IntelligenceFinding.sermon_id` (added in Phase 2.6)
already attributes each finding to its sermon with no new correlation
logic needed. Theme grouping uses the same `summary.starts_with(...)`
text-prefix convention already established elsewhere in this codebase
(`service.rs`, `sermon_foundation.rs`, `sermon.rs`, `pipeline.rs`) -
exact-label match only, no semantic similarity.

## What was built

- **`database/migrations/0018_saved_sermon_findings.sql`**: the
  `saved_sermon_findings` table (`id`, `service_id`, `sermon_id`
  nullable, `element_label`, `summary`, `payload` JSON, `created_at`),
  registered in `database/src/migrations.rs` as version 18.
- **`apps/desktop/src-tauri/src/persistence.rs`**: `persist_saved_sermon_finding`
  (mirrors `persist_saved_content_candidate`); `list_all_saved_sermon_findings`
  and `list_sermons` - both deliberately unscoped by service, the
  cross-sermon-analytics counterparts to their per-service equivalents.
- **`apps/desktop/src-tauri/src/sermon_knowledge_base.rs`** (new module,
  mirrors `harvest.rs`'s discipline): `element_label_for_summary` (pure,
  never panics, falls back to `"Other"`); `build_knowledge_base` (pure
  aggregation over already-fetched rows - theme frequency, sermons by
  speaker, recent findings); `get_knowledge_base` (the only function that
  touches the database).
- **`apps/desktop/src-tauri/src/commands.rs`**: `accept_sermon_finding`
  now persists via `persist_saved_sermon_finding` immediately after
  accepting (the one new write path); new command
  `get_church_knowledge_base` (open to any operator, read-only).
- **Frontend**: `SermonRef`/`ThemeFrequencyEntry`/`SpeakerHistoryEntry`/
  `SermonKnowledgeBase` in `domain/sermon.ts`; `getChurchKnowledgeBase`
  wrapper in `lib/commands.ts`; a new "Church Knowledge Base" section in
  `HistoryView.tsx`, loaded once on mount (not tied to selecting an
  individual service, since this data deliberately spans all of them) -
  shows most-preached themes and sermons by speaker.

## Testing boundary

Everything in `sermon_knowledge_base.rs` is either a pure function
(`element_label_for_summary`, `build_knowledge_base`) or a thin read
composed of two already-tested `persistence.rs` functions
(`get_knowledge_base`) - all of it is genuinely testable without a model,
audio, or any external dependency, unlike Whisper-model-dependent code
elsewhere in this codebase. 9 new Rust tests cover: every known
element-label prefix, the `"Other"` fallback (never panics), theme
occurrence counting vs. sermon-count deduplication, a Theme finding with
no `sermon_id` (counts as an occurrence but attributes to zero sermons -
an honest limitation, not a crash), non-Theme findings never leaking into
`theme_frequency`, per-speaker grouping/sorting (excluding sermons with no
recorded speaker, never fabricating one), recent-findings ordering, a
real-SQLite round trip through `get_knowledge_base`, and an empty database
producing a valid, empty (not error) result. 2 new frontend tests
(`commands.test.ts`) prove `getChurchKnowledgeBase` calls the right Tauri
command with no arguments and rejects outside the Tauri runtime without
calling `invoke()`, per this project's standing Phase 1.2.1 IPC-guard
discipline.

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs (default and `--features whisper`).
- `cargo test --workspace`: all passing, 9 new (`cip-desktop` up from 341
  to 350 - see `pilot-evidence/13/build/church-knowledge-base-evidence.json`
  for exact counts).
- `cargo test --features whisper` (in `apps/desktop/src-tauri`): 350
  passed, matching the default-feature count, confirming the whisper
  build is unaffected.
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings,
  unchanged) / `npm run test -- --run` (287 passed, up from 285 - 2 new)
  / `npm run build`: all clean.

## Architectural safety

- 1 new Tauri command, zero new events, 1 new migration (justified per
  `docs/phase-13-audit.md`'s "Persistence justification" - a knowledge
  base that must survive restarts and span services cannot, by
  definition, be in-memory).
- The only new write path is inside `accept_sermon_finding`, an already-
  explicit, already-audited operator action (spec section 36/47: "no
  automatic actions," "has no way to create a `PresentationItem` or any
  other side effect") - Detected/Reviewed/Rejected findings are never
  persisted to `saved_sermon_findings`.
- `sermon_knowledge_base.rs` contains no detection logic of its own and
  invents no title/label when one is absent, mirroring `harvest.rs`'s own
  discipline exactly.
- `core/bible`, `core/service`, `core/presentation`, `core/music` (every
  other domain contract crate) are entirely untouched.

## Windows rebuild

Required: this phase changes Rust code compiled into the desktop binary
(new migration, new persistence functions, new module, one new command,
one modified command). See
`pilot-evidence/13/windows/installer-contents-verification.json` and the
updated `release/windows/release-manifest.json`.

## Known limitations (honest, not deferred silently)

- **Only operator-accepted findings appear in the knowledge base** - a
  Detected or Reviewed (but never accepted) finding, however confident,
  never enters `saved_sermon_findings`. This is deliberate (see
  `docs/phase-13-audit.md`'s persistence-justification precedent), not an
  oversight, but it does mean a church that never explicitly accepts
  Sermon Intelligence findings during a live service will see an empty
  Church Knowledge Base regardless of how much was actually said.
- **No retroactive backfill** - findings from services that happened
  before this phase shipped are gone; they were never durable in the
  first place, and this phase cannot honestly recover what was never
  saved.
- **Theme grouping is exact-label match only** - "Waiting on God" and "On
  Waiting for God" are two different entries, not one. No semantic
  similarity is attempted in this phase; see `docs/phase-13-audit.md`'s
  explicit scope boundaries for why, and what a future phase could add.
- **A finding accepted with no active sermon context** (`sermon_id: None`)
  still counts as a Theme occurrence but is attributed to zero sermons in
  `sermon_count`/`sermons` - it cannot honestly be linked to one it never
  had.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware**, and no real operator has yet accepted a real
  Sermon Intelligence finding and confirmed it later appears correctly in
  the Church Knowledge Base after a restart - see `physicalHardwareStatement`
  item 23 in the updated release manifest.

## Final gate

Environment A (build-time verification, full regression, direct binary
symbol inspection): PASS. Environment C (a real operator accepting a real
Sermon Intelligence finding during a real service, restarting the
application, and confirming the Church Knowledge Base still shows it
correctly grouped by theme and speaker): not yet performed - carried
forward into `physicalHardwareStatement` per this project's standing
discipline.
