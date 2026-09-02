# Phase 13 Audit: Church Knowledge Base / Cross-Sermon Analytics

## Trigger

The user's own item from a pasted advice list ("nothing aggregates
[sermon intelligence] across services yet"), followed by the explicit
instruction "church knowledge base / cross-sermon analytics" - the last
remaining item from that list. This phase follows this project's standing
discipline: verify the real architecture before designing anything, then
implement, test, rebuild, and ship exactly like Phases 10-12.

## Verifying the premise before building anything

The user's framing ("nothing aggregates it across services yet") is close
but not quite precise. Direct source inspection found the sharper, real
gap:

1. **`IntelligenceFinding` (all domains, Sermon included) is never
   persisted to any database table.** `apps/desktop/src-tauri/src/state.rs`
   holds it only in `AppState.intelligence_findings: Mutex<FindingQueue>`,
   and `apps/desktop/src-tauri/src/persistence.rs` contains a literal
   comment confirming this is deliberate for now: `"(findings stay in
   AppState.intelligence_findings, unchanged)"`. `FindingQueue` is
   constructed exactly once, at `AppState` startup
   (`state.rs:433`), and nothing anywhere in this codebase ever clears or
   replaces it mid-process - so within a single continuous run of the
   application, findings from every sermon given during that run *do*
   stay addressable in memory. But that memory does not survive an
   application restart, which is the normal weekly cadence a church
   actually operates on. **Genuine cross-sermon analytics - across
   restarts, across weeks, across months of services - is not "not yet
   aggregated," it is currently impossible**, because the source data
   itself does not durably exist past the current process lifetime. This
   is the real, corrected premise this phase must fix first.

2. **Sermon Harvest (Phase 3.9, `apps/desktop/src-tauri/src/harvest.rs`)
   already proves the read-only-aggregation pattern this phase extends -
   and its own module docs already say why it cannot do cross-sermon
   work today**: "why this only ever harvests the *currently active*
   sermon, never a past one, in this phase." It reads
   `AppState.intelligence_findings` directly (in-memory), which is exactly
   why it is scoped to whatever is still resident in that Mutex.

3. **The `sermons` table (`database/migrations/0008_sermon_foundation.sql`)
   is durable** - id, service_id, title, speaker_id/name/role, status,
   started_at, ended_at, created_at - and already survives a restart. It
   carries no theme/topic field, and no function today lists *every*
   sermon across every service (only `list_sermons_for_service`, scoped
   to one service, exists in `persistence.rs`).

4. **`IntelligenceFinding` already carries a `sermon_id: Option<Uuid>`**
   (added in Phase 2.6, `core/intelligence/src/finding.rs:51`), set only
   from `IntelligenceContext::active_sermon` by
   `sermon_adapter.rs:243-244` - never guessed, `None` both when no
   sermon was active and for every non-Sermon-domain finding. This means
   the moment findings are made durable, they are *already* individually
   attributable to the sermon that produced them - no new correlation
   logic is needed for that part.

5. **`IntelligenceFinding` has no structured per-taxonomy-element field.**
   `FindingKind` is coarse (`Scripture`/`Music`/`ServiceState`/`Sermon`/
   `Content`/`Correlation`) - it does not distinguish a Theme from a
   Takeaway from a Key Statement. The only place that distinction exists
   at all is a human-readable text-prefix convention inside
   `summary`, established by `core/intelligence/src/sermon_adapter.rs`'s
   `finding_for_detection`/`finding_for_theme`/`finding_for_transition`/
   `finding_for_scripture_cross_link` functions - e.g. `"Theme: {label}"`,
   `format!("Takeaway: {raw}")`, `format!("Main Point: {raw}")`,
   `format!("Key Statement: {raw}")`, `format!("Food for Thought: {raw}")`,
   `format!("Transition: {from} -> {to}")`,
   `format!("Supporting Scripture: {reference}")`. This is not a
   one-off hack invented for this phase to lean on: `service.rs:177/185`,
   `sermon_foundation.rs:159`, `sermon.rs:126/260`, and
   `pipeline.rs:3431` already branch on `summary.starts_with(...)`
   elsewhere in this exact codebase, and `harvest.rs`'s own docs
   explicitly note a finding's `summary` "is already presentation-ready
   text... produced by the Sermon engine" that downstream code is
   expected to treat as meaningful, not opaque. Using this same,
   already-precedented convention for this phase's theme-grouping logic
   is consistent with the codebase, not a new brittle shortcut - and it
   is pinned by a regression test (see "What was built" below) so a
   future change to `sermon_adapter.rs`'s summary formats cannot silently
   break it.

6. **Precedent for "persist only on explicit operator acceptance"
   already exists and is the right shape to reuse**: Phase 2.7.1's
   `saved_content_candidates` table + `persist_saved_content_candidate`
   (called only from `accept_content_candidate`, never from detection)
   is the exact model for how a normally-in-memory, normally-ephemeral
   intelligence artifact becomes durable history in this codebase - "an
   explicit, operator-initiated 'save,' never an automatic write on
   detection" (its own migration's docstring). `Sermon Intelligence`'s own
   spec section 36/47 discipline ("no automatic actions", accepting a
   finding "has no way to create a `PresentationItem` or any other side
   effect") is exactly the same "explicit human action, no silent
   automation" posture this phase must not violate when adding a new
   write path for findings.

## Design decisions

- **New migration `0018_saved_sermon_findings.sql`**: a `saved_sermon_findings`
  table (`id`, `service_id`, `sermon_id` NULLABLE, `element_label`,
  `summary`, `payload` JSON, `created_at`), mirroring
  `saved_content_candidates`'s exact shape and its "only written on
  explicit accept" discipline. `sermon_id` is nullable because
  `IntelligenceFinding.sermon_id` itself is `Option<Uuid>` - a finding
  detected with no active sermon context is still a real, operator-
  accepted piece of intelligence worth keeping, it just cannot be
  attributed to a specific sermon for cross-sermon grouping (an honest,
  documented limitation, not silently dropped).
- **Hook point: `accept_sermon_finding`** (the only place a Sermon-domain
  finding transitions from ephemeral-and-heuristic to
  operator-confirmed-fact) persists to the new table, exactly mirroring
  `accept_content_candidate` -> `persist_saved_content_candidate`.
  Detected/Reviewed/Rejected findings are never persisted here - this
  keeps the knowledge base built only from what an operator explicitly
  confirmed, not every heuristic guess the Sermon engine ever produced.
- **`element_label`** is derived once, in Rust, by a small pure function
  that maps `summary`'s known prefix to a stable label (`"Theme"`,
  `"Takeaway"`, `"Main Point"`, ... falling back to `"Other"` for anything
  unrecognized rather than panicking) - stored as its own column purely so
  SQL-adjacent grouping doesn't need to re-parse JSON, matching
  `saved_content_candidates.candidate_type`'s own precedent of duplicating
  a derived value out of the JSON payload.
- **New orchestration module `apps/desktop/src-tauri/src/sermon_knowledge_base.rs`**
  (mirrors `harvest.rs`'s shape: pure aggregation, no detection logic of
  its own, no SQL joins - reads already-persisted rows with two new
  `persistence.rs` functions (`list_sermons` - every sermon, most recent
  first, unscoped by service; `list_all_saved_sermon_findings` - every
  saved sermon finding, most recent first) and assembles them in Rust,
  the same style `harvest_sermon` already uses).
  - `theme_frequency`: every distinct Theme label, most-mentioned first,
    with the sermons it was attached to (title/speaker/date) - the
    "what have we preached about, and how often" view.
  - `sermons_by_speaker`: every speaker (from the durable `sermons` table
    itself, not from findings) with their sermon count and sermon list -
    the "who has preached, and on what" view.
  - `recent_findings`: the most recently accepted sermon findings of any
    kind, newest first - a simple browsable feed.
- **New command `get_church_knowledge_base`** - open to any operator
  (read-only, retrospective; no reason to Admin-gate a read of already-
  accepted history, consistent with `get_speech_language_capabilities`/
  `list_saved_content`'s own precedent of leaving read-only, non-
  destructive commands open).
- **Frontend**: a new "Church Knowledge Base" section in `HistoryView`
  (retrospective, cross-service data belongs with History, not the live
  `LiveChurchBrain` workspace - the same reasoning that already placed
  Saved Content there in Phase 2.7.1).

## Explicit scope boundaries (what this phase does NOT do)

- **No new AI/detection logic.** This phase persists and aggregates data
  the Sermon engine already produces; it adds no new theme/element
  detection.
- **No semantic similarity.** Theme grouping is exact-label match only
  (case-sensitive, as `ThemeTracker` already produces its labels
  consistently upstream) - two differently-worded mentions of the same
  underlying idea are not merged. A future phase could layer
  `core/bible::semantic`-style embedding similarity on top of this if a
  real need for it is demonstrated; deliberately out of scope here.
- **No retroactive backfill.** Only findings accepted *after* this
  phase ships enter `saved_sermon_findings` - findings from services that
  already happened are not (and cannot honestly be) recovered, since they
  were never durable in the first place.
- **No cross-domain correlation beyond what already exists.** This is a
  Sermon-domain-only knowledge base; Scripture/Music/Content history
  already have their own separate "Saved" sections in `HistoryView`.

## Persistence justification

Phase 2.0's own default (spec section 31, restated in `queue.rs`'s module
docs) is "in-memory unless persistence is clearly justified." This phase
is exactly that justification, made explicit: a knowledge base that must
survive restarts and span services by definition cannot be in-memory.
This mirrors `sermons` itself (Phase 2.5) and `saved_content_candidates`
(Phase 2.7.1), both of which made the identical argument for their own
tables.
