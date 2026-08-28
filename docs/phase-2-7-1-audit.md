# Phase 2.7.1 — Content Intelligence Operationalization & Church Resource
# Library UX: Audit

## A. Git baseline

- Branch: `claude/cip-foundation-init-i85g87`
- HEAD at audit start: `4c6978f` (Phase 3.7, "Full Offline Operator Test
  Mode & System Verification")
- Working tree: clean

This phase's numbering ("2.7.1") is historical (it names the original
Content Intelligence milestone this phase completes the operator-facing
half of) and does **not** roll the repository back — all Phase 1–3.7 work
stays fully in place and is reused, not rebuilt.

## B. Repository architecture (confirmed unchanged from Phase 3.7)

One `IntelligenceContext`, one intelligence-engine registry
(`core/intelligence`), one presentation architecture (`presentation.rs` +
`core/presentation`), one persistence layer (`persistence.rs` over a
single SQLite database), one Content Registry (`core/content` +
`integrations/content`). Nothing in this audit found a second copy of any
of these anywhere in the tree.

## C. Bible audit

- **Dataset**: `database/datasets/bsb/bsb.json` (5.2 MB, compiled into the
  binary via `include_str!` in `bible_production_dataset.rs`). Verified
  directly (not merely from a prior report): **66 distinct books, 1,189
  distinct chapters, 31,086 verses**, `translation.licensingStatus =
  "verified_public_domain"`. This matches every prior phase's stated
  figures exactly.
- **Checksum**: deterministic FNV1a-based (`importer.rs::compute_checksum`,
  sorted rows, 16 hex chars), proven stable/reimport-idempotent by
  `checksum_is_deterministic_and_changes_with_content` and
  `checksum_is_stable_across_repeated_imports_of_the_same_dataset`
  (pre-existing, re-verified this audit via `cargo test`). This audit did
  not need to hand-verify one specific literal checksum value — the tests
  already prove it deterministic and stable, which is the property that
  matters.
- **Commands** (all registered in `lib.rs`, all reused unchanged this
  phase): `search_bible`, `list_bible_books`, `save_scripture`,
  `list_saved_scriptures`, `delete_saved_scripture`,
  `list_presentation_history`, `preview_scripture`/`preview_presentation`,
  `prepare_presentation`/`create_manual_presentation`,
  `import_bible_dataset`, `check_bible_dataset_integrity`.
- **Frontend**: `apps/desktop/src/components/library/BibleLibrary.tsx`
  (Phase 3.6) already implements Browse (Old/New Testament → book →
  chapter → verses), Search (reference or text), Saved (list/prepare/
  remove), verse-range save/prepare, and inline Preview — against the
  real BSB dataset, with no audio dependency anywhere in the file.
- **Verse/range retrieval**: `get_verse`/`get_verse_range` (pre-existing,
  `core/bible`) — used by both search and presentation.
- **Presentation**: `build_scripture_slide` → `persist_prepared_item` →
  `prepare_to_activate`/`commit_activation` → `stop_active_item` (all
  pre-existing, `presentation.rs`) — the one presentation state machine,
  reused, not duplicated.
- **Cross-references**: `grep -rniE "cross.?reference|related.?verse|xref"`
  across `core/bible/`, `database/`, `integrations/bible/` returns **zero
  matches**. No cross-reference data structure exists anywhere in this
  codebase — not partially built, not stubbed, genuinely absent. Per spec
  section 8, this must be stated honestly rather than fabricated.
- **What prevents a "complete" Bible Library today**: nothing
  functionally — browsing, search, save, reuse, and presentation are all
  already real and working against the real dataset. The two honest gaps
  are (1) no cross-reference data (never claimed, see above) and (2) no
  "used in service/presentation" usage-reference display (see section H
  below — a legitimate, non-biblical provenance view this audit found
  reusable data for).

## D. Music audit

- **Song struct**: `core/music/src/song.rs::Song` (id, title, sections,
  lyric lines) — unchanged since Phase 2.1/2.2.
- **`MusicProvider` trait** (`core/music/src/provider.rs`):
  `list_datasets`, `get_song`, `search_title`, `search_alias`,
  `search_number`, `search_lyrics`, `get_sections`, `get_lyrics`. **No
  `list_songs(content_id)` enumeration method exists** — there is no way,
  anywhere in the current architecture, to ask "what songs does this
  dataset contain," only to search for one by title/alias/number/lyric
  substring or fetch one by known id. This is a real, structural gap, not
  a UI omission.
- **Persistence**: song data lives in `bible_verses`-sibling tables from
  migration `0006_music_content.sql`, read-only from the application's
  perspective — there is no `persist_song`/save mechanism anywhere (`git
  grep` for `persist_song`/`save_song` returns nothing). Re-confirms Phase
  3.6's own finding (`pilot-evidence/3.6/validation-matrix.json`'s "Song
  Save" row: `code: false`).
- **Dataset reality**: `database/seeds/dev_seed.sql` / the
  `integrations/music` dev fixture is a **5-song, explicitly fictional**
  set (`docs/music-datasets.md`: "no real hymnal or worship set has been
  imported or is claimed to be installed"), registered in the Content
  Registry only in non-`Production` builds (`apply_dev_seed`). **No
  licensed production song dataset exists anywhere in this repository.**
- **Frontend**: `MusicLibrary.tsx` (Phase 3.6) already shows an honest
  empty state in a production build ("No licensed song library installed
  yet...") and reuses `searchMusic` for whatever *is* installed (dev/test
  only). No song browse or detail view exists — Phase 3.6 explicitly
  deferred building one "against fictional data."
- **Conclusion**: MUSIC LIBRARY gate = **LEGALLY BLOCKED** (no licensed
  dataset exists to browse), not PASS or HOLD — building song
  browse/detail/save against the fictional fixture would not move CIP
  toward a real church resource library and was again judged not worth
  doing this phase, consistent with the Phase 3.6 precedent. See section N.

## E. Content Intelligence audit

- **`ContentCandidate`** (`core/intelligence/src/content_candidate.rs`):
  `id`, `service_id`, `sermon_id`, `source_finding_ids`, `candidate_type`,
  `title_or_label`, `working_concept`, `assertion_level`, `status`
  (reused `FindingStatus`), `confidence` (reused `ConfidenceResult`),
  `content_potential`, `evidence` (`Vec<EvidenceSource>`), `provenance`
  (`IntelligenceProvenance`), `engine_id`/`engine_version`, `created_at`.
  Already `#[derive(Serialize, Deserialize)]` with `#[serde(rename_all =
  "camelCase")]` — the same shape already crosses the Tauri IPC boundary
  today, so it round-trips through JSON safely.
- **Storage**: `AppState::content_candidate_queue: Mutex<ContentCandidateQueue>`
  (`state.rs`) — **in-memory only**. No `content_candidates` (or similar)
  table exists in any migration. Confirmed by `git grep -n
  "content_candidate" database/migrations` (no matches) and by reading
  `state.rs` directly.
- **The broken link** (spec section E's exact chain,
  `Bible/Sermon/Music finding → correlation → ContentCandidate → review →
  accept → save → reopen → reuse`): every step up to and including
  `accept_content_candidate` (`commands.rs`) works and is real — but
  `accept()` (`ContentCandidateQueue::accept`, `core/intelligence`) only
  flips the in-memory candidate's status to `Accepted`. Nothing writes it
  to SQLite. `list_accepted_content_candidates` additionally requires
  `current_service_id(&state)` to succeed — i.e. it errors once no
  service is active. **The chain breaks at "save"**: an accepted content
  candidate does not survive the service ending, and does not survive an
  application restart. This directly conflicts with spec section 29's
  acceptance scenario ("Restart. Open Content. EXPECT: accepted content
  accessible") and section 35's explicit "SAVED CONTENT: PASS/HOLD" gate
  item.
- **Frontend**: `LiveChurchBrain.tsx` already has a complete, real,
  provenance-carrying review UI — candidates appear in the merged
  Attention Queue/Intelligence Feed with `titleOrLabel`, `workingConcept`,
  `candidateType`, `confidence.score`, `evidence.length`, and `status`
  shown per card, with working Accept/Reject buttons wired to the real
  commands, plus a "Saved Content" `<details>` panel listing accepted
  candidates for the *current* live session. This is good, reusable UI —
  it only needs a durable data source to survive past the current
  session.
- **Events**: `ContentCandidateDetected`/`Accepted`/`Rejected` already
  exist and already fire correctly (`events.rs`) — reused unchanged.

**This is the one clearly-provable, spec-justified gap this audit found**:
Saved Content has no persistence. Section H proposes the minimal additive
fix.

## F. History audit

`apps/desktop/src/components/library/HistoryView.tsx` (Phase 3.6) already
provides, per service, real (not fabricated) history: Presentation History
(with a "Reuse in current service" action), Scripture & Findings
(suggestions), Transcript, and Timeline — all read through
`listServiceHistory`/`listTimeline`/`listTranscript`/`listSuggestions`/
`listPresentationHistory`, all `service_id`-scoped SQLite reads, all
pre-existing. Saved Scripture is reachable globally from Bible Library's
own Saved tab (deliberately not service-scoped — a Scripture bookmark is
meant to be found again in a *future*, not-yet-existing service, exactly
as `0010_saved_scriptures.sql`'s own migration comment documents).

**Missing from History**: a "Saved Content" section — because, per
section E, there is currently nothing durable to show. Once section H's
persistence lands, History is the natural place to expose it (mirroring
exactly how Presentation History already works: a `service_id`-scoped
list command plus a new panel in `HistoryView.tsx`), with no new
persistence architecture — the audit confirms the existing service/
timeline/persistence infrastructure is sufficient.

## G. Presentation audit

Unchanged from the Phase 3.7 audit (re-confirmed, not re-litigated this
phase): `PresentationItem` states `Prepared → Active → Stopped`
(`core/presentation`), `build_scripture_slide`/`persist_prepared_item`/
`prepare_to_activate`/`commit_activation`/`stop_active_item`
(`presentation.rs`), `reconcile_stale_active_presentation_items` at
startup for crash recovery, and `phase_3_7_full_offline_operator_chain_acceptance`
(`pipeline.rs`) already prove display/stop/restart survival end to end
against the real BSB dataset. Nothing here needs to change for this
phase — Content Candidates and Saved Scripture already connect to this
exact presentation path (`createManualPresentation`) rather than a
duplicate.

## H. Existing commands (Bible/Content/History-relevant, non-exhaustive)

`search_bible`, `list_bible_books`, `save_scripture`,
`list_saved_scriptures`, `delete_saved_scripture`, `preview_scripture`,
`create_manual_presentation`, `prepare_presentation`,
`display_presentation`, `stop`-family presentation commands,
`list_service_history`, `get_service`, `list_timeline`, `list_transcript`,
`list_suggestions`, `list_presentation_history`,
`analyze_content_intelligence`, `list_content_candidates`,
`list_accepted_content_candidates`, `accept_content_candidate`,
`reject_content_candidate`.

## I. Existing events (Content-relevant)

`ContentCandidateDetected`, `ContentCandidateAccepted`,
`ContentCandidateRejected` — all pre-existing, all reused unchanged.

## J. Existing database tables (relevant subset)

`services`, `transcript_segments`, `scripture_detections`,
`ai_suggestions`, `presentation_items`, `audit_events`, `content_registry`,
`bible_translations`/`bible_books`/`bible_chapters`/`bible_verses`,
`saved_scriptures` (Phase 3.6), music content tables (migration 0006/0007),
sermon foundation tables (migration 0008). **No table for Content
Candidates exists.**

## K. Existing migrations

`0001`–`0010` (see `database/migrations/`), most recently
`0010_saved_scriptures.sql` (Phase 3.6). This phase's proposed
`0011_saved_content_candidates.sql` (section N) would be the eleventh.

## L. Existing frontend screens

`LiveChurchBrain.tsx` (live workspace), `BibleLibrary.tsx`,
`MusicLibrary.tsx`, `HistoryView.tsx`, `TestCenter.tsx` (Phase 3.7),
`PilotDiagnosticsPanel.tsx`. Top-level nav in `App.tsx`:
Live Service / Bible / Music / History / Offline Test Center.

## M. Missing capabilities (confirmed, not assumed)

1. **Saved Content Candidate persistence** — see section E. Confirmed
   missing by direct inspection of `state.rs`, `commands.rs`, and every
   migration file.
2. **Bible cross-reference data** — confirmed absent (section C). Not a
   gap to fill; a fact to state honestly in the UI.
3. **Music song enumeration/browsing/saving** — confirmed missing at the
   `MusicProvider` trait level (no `list_songs`), and confirmed no
   licensed dataset exists to make building it worthwhile this phase
   (section D/N).
4. **Scripture "used in service/presentation" usage references** — not
   missing at the data level (every fact needed already exists in
   `presentation_items`/`ai_suggestions`), but no UI currently surfaces it
   per-verse. See section N for the minimal, honest scope chosen.

## N. Reusable capabilities / proposed minimal changes

| Capability | Existing | Partially Existing | Missing | Reusable Path | New Code Needed |
|---|---|---|---|---|---|
| Bible browse/search/save/reuse/present | ✅ | | | `BibleLibrary.tsx` + existing commands | None |
| Bible cross-references | | | ✅ | — | None (honest label only, see below) |
| Music search (dev fixture) | ✅ | | | `searchMusic` | None |
| Music browse/save (production) | | | ✅ | `MusicProvider` trait exists but has no enumeration method, and no licensed dataset exists | Deferred (section 33 hard-stop-adjacent: would require copyrighted content this repo does not have rights to) |
| ContentCandidate detection/review/accept/reject (in-session) | ✅ | | | `content_intelligence.rs`, `LiveChurchBrain.tsx` | None |
| ContentCandidate persistence across restart | | ✅ (type already `Serialize`) | ✅ | New table storing the existing `ContentCandidate` JSON verbatim (same pattern as `ai_suggestions.payload`/`presentation_items.content`) | One additive migration + `persistence.rs` functions + 1 new command + a small `HistoryView.tsx` addition |
| Service History | ✅ | | | `HistoryView.tsx` + existing commands | None |
| Presentation History | ✅ | | | `HistoryView.tsx` + `list_presentation_history` | None |
| Scripture usage references (non-biblical) | | ✅ (data exists, unsurfaced) | | Client-side derivation is out of scope for this phase's minimal-change rule (would need a new cross-service query); deferred, see section P | Deferred |
| Presentation (prepare/preview/display/stop/restart) | ✅ | | | `presentation.rs` | None |

## O. Proposed minimal changes for this phase

1. **`database/migrations/0011_saved_content_candidates.sql`** (additive
   only): a new `saved_content_candidates` table — `id`, `service_id`,
   `candidate_type`, `payload` (the full `ContentCandidate`, JSON, same
   convention as `ai_suggestions.payload`/`presentation_items.content`),
   `created_at`. No existing table is altered.
2. **`persistence.rs`**: `persist_saved_content_candidate`,
   `list_saved_content_candidates_for_service`, mirroring
   `persist_saved_scripture`/`list_saved_scriptures`'s exact existing
   pattern.
3. **`commands.rs`**: `accept_content_candidate` additionally persists a
   durable copy the moment a candidate is accepted (never on detection,
   never on mere review — matching "save" being an explicit, terminal
   operator action, same as Bible Library's "Save"). One new command,
   `list_saved_content(serviceId)`, mirroring `list_presentation_history`'s
   exact signature/shape, for reopening after a restart regardless of
   whether that service is still active.
4. **`HistoryView.tsx`**: a new "Saved Content" section per opened
   service, reusing the exact same list-card pattern already used for
   Presentation History.
5. **`BibleLibrary.tsx`**: a one-line, honest disclaimer under verse text
   — "Cross-references are not available in this installed Bible
   dataset." — satisfying spec section 8 without fabricating anything.
6. **Tests**: persistence round-trip tests (mirroring
   `saved_scripture_create_retrieve_matches_the_committed_row`), and an
   acceptance test extending the Phase 3.7 pattern (real file-backed
   restart) to also prove an accepted Content Candidate survives service
   end and application restart.

No second `ContentCandidate` type, no second intelligence engine, no
second presentation path, no second persistence architecture. The
`ContentCandidate` Rust type is persisted **verbatim** (via its own
existing `Serialize`/`Deserialize`), so provenance, evidence, confidence,
and assertion level are preserved byte-for-byte, not re-derived.

## P. Explicitly deferred work

- Music song browse/detail/save — no licensed dataset exists; building
  this against the fictional fixture would not produce anything useful
  toward a real church resource library (Phase 3.6's own conclusion,
  re-confirmed).
- A generalized Collections/Favorites/"My Collections" framework — the
  existing flat "Saved" lists (Scripture; now Content) already serve the
  reuse need the screenshot's Collections concept exists for; spec
  section 10 explicitly requires this be proven necessary before being
  built, and this audit did not find a workflow the flat lists cannot
  already serve.
- Per-verse "used in service/presentation" usage-reference UI — the
  underlying data exists, but surfacing it well (a cross-service query,
  not currently exposed by any command) is a genuinely separate, larger
  addition than this phase's proven-necessary scope; deferred rather than
  rushed.
- Any visual redesign inspired by the reference screenshot beyond what
  Phase 3.5.1's existing semantic color system already provides — the
  screenshot is UX inspiration only, not a redesign mandate (spec
  sections 3/15/39).

## Q. Licensing constraints

BSB: `VerifiedPublicDomain`, unchanged. Music: no licensed dataset exists;
this phase introduces no new song, lyric, or media asset of any kind — the
persisted `ContentCandidate` payload contains only content this
application's own intelligence engines already derived from a real
transcript the operator entered, never anything copied from an external
source.

## R. Offline constraints

Every proposed change (migration, persistence functions, one new command,
two frontend additions) is local-SQLite-only. `cargo tree --workspace
--all-features` (re-run this phase) still contains no HTTP client crate.
Nothing proposed here requires, or could accidentally introduce, network
access.

---

**Hard-stop check (spec section 33)**: none of the ten listed conditions
apply. The BSB dataset is genuinely available and verified. Licensing
metadata is consistent. No feature proposed requires copyrighted content.
The proposed migration is purely additive (a new table; nothing existing
is altered or destroyed). No second intelligence/presentation architecture
is required. Nothing proposed requires Internet connectivity. Existing
backend contracts (the `ContentCandidate` type, existing commands) fully
support the proposed workflow without modification. The reference
screenshot is used only as UX inspiration, not copied. Every proposed
feature can be implemented honestly with data that genuinely exists.

**Proceeding to implementation as scoped in section O.**
