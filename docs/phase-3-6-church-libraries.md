# Phase 3.6 — Church Knowledge Libraries & Service History

## 1. Baseline

Started at commit `60f3994` (Phase 3.5.1, professional operator UX color
correction), tree clean — confirmed via `git branch --show-current`,
`git rev-parse HEAD`, and `git status --porcelain` before any file was
touched.

## 2. Audit

A forensic, read-only audit was performed across five areas in parallel
before any implementation began: the Bible dataset/provider architecture,
the Music system, Service/Presentation/Content History persistence,
Cross-Domain correlation persistence and the full SQLite schema, and the
frontend navigation/command surface. Every claim below is backed by a
file/line citation gathered during that audit — none is inferred from
prior phase reports, per this phase's own baseline rule ("the repository
is authoritative, not prior documentation").

## 3. Existing capabilities (PROVEN, reused unmodified)

- **BSB dataset**: the complete, real production dataset —
  `database/datasets/bsb/bsb.json`, embedded via `include_str!`
  (`bible_production_dataset.rs`) — 66 books, 1,189 chapters, 31,086
  verses, checksum `d4335582ff26a3ac`, `verified_public_domain`. Imported
  idempotently on every launch through `content::import_and_register` →
  `cip_integrations_bible::import_bible_dataset`.
- **`core/bible`'s provider API**: `get_book`, `get_chapter`, `get_verse`,
  `search`, `list_chapters`, plus the free functions `get_verse_range` and
  `search_bible` (which already dispatches "Book C:V" / "Book C:V-V" /
  "Book C" / free text to the right provider call).
- **Service History persistence**: `services` table; `list_service_history`,
  `get_service`, and — critically — `list_timeline`/`list_transcript`/
  `list_suggestions` already accept an optional `service_id`, so they
  already worked against a *past*, non-live service before this phase.
- **Presentation persistence**: `presentation_items` already records
  `service_id`, `status` (Prepared/Active/Stopped), `created_at`,
  `source_suggestion_id`, `template` — and already survives a restart
  (`persistence::persist_presentation_item`, a real `INSERT`).
- **Cross-domain correlations**: `IntelligenceCorrelation` already carries
  `assertion_level`, `confidence: ConfidenceResult`,
  `evidence: Vec<EvidenceSource>`, `source_finding_ids`, `rule_id`/
  `rule_version` — a complete, deterministic, non-fabricated provenance
  record, already surfaced end-to-end through the Phase 2.9 unified
  Intelligence Feed. No new UI was needed to satisfy section 12.
- **Music domain model**: `Song`/`SongSection`/`LyricLine`, a full
  `MusicProvider` trait (`get_song`/`get_lyrics`/`get_sections`/search
  methods), and a real `SqliteMusicProvider` backing four real database
  tables (`music_songs`/`music_aliases`/`music_sections`/`music_lyrics`).

## 4. Missing capabilities (the actual gap register)

| Capability | Classification | Why |
|---|---|---|
| Book/chapter browse UI (Old/New Testament → book → chapter → verses) | MISSING | No book-list command existed; `get_book`/`list_chapters` were called only internally by `check_bible_integrity`, never exposed |
| Verse-range presentation | PARTIAL → bug | `build_scripture_slide` silently kept only the first verse of a range (`parse_display_reference`'s `.split('-').next()`) — `get_verse_range` existed but was never wired into the presentation path |
| Saved/recent Scripture, reusable across services | MISSING | The only "recent" concept (`ScriptureContextManager::recent_references`) is in-memory, bounded, per-session, never persisted, never exposed by any command |
| Presentation History for a past/non-live service | PRESENT BUT NOT EXPOSED | The data (`presentation_items`) already supports it; `list_prepared_presentations` was hardcoded to the live service and `Prepared`-only status |
| A real, licensed, browsable song library | MISSING (legally) | `MusicProvider::get_song`/`get_lyrics`/`get_sections` are fully implemented and tested but **no Tauri command ever calls them** — and the only content that exists is a 5-song, explicitly fictional dev fixture, never a licensed production dataset |
| Top-level navigation (Bible / Music / History as real destinations) | MISSING | No router, no "page" concept anywhere — `LiveChurchBrain` was the entire application UI |
| Music findings/candidates/correlations surviving a restart | MISSING (by design, out of scope) | Confirmed deliberate architecture (documented `AppState` comments: "deliberately not persisted... a correlation is derived from findings that themselves already carry provenance") — not changed by this phase; see "Deferred work" |

## 5. Reuse decisions

- **Book browser**: reuses `BibleProvider::get_book` (one new Tauri
  command, `list_bible_books`, that loops the canonical 66-book list —
  no new provider method, no schema change).
- **Chapter reader**: reuses `search_bible("BOOK C")` exactly as it
  already worked (the existing "Manual Bible Search" chapter-detection
  branch) — no new command.
- **"Use in service" / "Prepare"**: reuses `createManualPresentation`
  exactly as the pre-existing Manual Bible Search already did.
- **Presentation History**: reuses the existing, already-general
  `persistence::list_presentation_items(service_id, status)` — only a
  thin new command (`list_presentation_history`) was needed to reach it
  with an operator-chosen `service_id` instead of the hardcoded live one.
- **Service History detail**: reuses `getService`/`listTimeline`/
  `listTranscript`/`listSuggestions` exactly as they already worked with
  an explicit `service_id`.
- **Cross-domain provenance**: no new work — already fully exposed via
  the Phase 2.9 Intelligence Feed/Attention Queue.
- **Music search**: reuses `searchMusic` exactly as the existing
  Diagnostics panel already called it; the new Music Library view is an
  honest status/empty-state wrapper around it, not a duplicate engine.

## 6. Database decisions

One new migration: `database/migrations/0010_saved_scriptures.sql`,
adding a single new table, `saved_scriptures`. This was judged genuinely
necessary — not a reuse-avoidable gap — because every existing candidate
table is service-scoped and one-shot (`scripture_detections`,
`ai_suggestions`, `presentation_items`), while a saved Scripture is
explicitly church-wide and meant to be found again in a *future*,
not-yet-created service. The migration is purely additive: it creates one
new table and one new index, alters nothing existing, and was verified
idempotent via the Xvfb fresh/idempotent relaunch pair (see §17).

No other new table was created. Presentation History, Service History,
and Cross-Domain provenance all reused existing tables/columns.

## 7. Bible Library architecture

`apps/desktop/src/components/library/BibleLibrary.tsx` — three tabs:

- **Browse**: Old/New Testament → book grid (`listBibleBooks`) → chapter
  grid (from the book's own `chapterCount`, no extra call) → chapter
  reader (`searchBible("BOOK C")`). A range tool lets the operator select
  a from/to verse within the open chapter.
- **Search**: the same `searchBible` free-text/reference dispatcher the
  old Manual Bible Search already used, now reachable from top-level
  navigation.
- **Saved**: `listSavedScriptures`, with Prepare/Remove actions.

Every verse/range card offers Preview (`previewScripture`), Prepare
(`createManualPresentation`), and Save (`saveScripture`) — matching
Workflow A/B in the spec exactly.

## 8. Music Library architecture

`apps/desktop/src/components/library/MusicLibrary.tsx` — honestly reports
installed music datasets via the existing `listContentRegistry("music")`
and, only when at least one is enabled, offers the existing `searchMusic`
search. When none is installed, it shows: *"No licensed song library
installed yet. Music Intelligence can still detect and analyze available
evidence during a live service... a searchable song library will appear
here once a licensed dataset is imported."* No song detail/lyrics view or
full song-browse UI was built — see §24 for why.

## 9. Service History architecture

`apps/desktop/src/components/library/HistoryView.tsx` — a service list
(`listServiceHistory`) → detail view combining `listTimeline`,
`listTranscript`, `listSuggestions`, and the new
`listPresentationHistory`, all scoped to the selected past service.
"Reuse in current service" on a historical Scripture presentation item
calls `createManualPresentation` against whichever service is *currently*
live — it never mutates the historical record it read from.

## 10. Presentation History

Exposed via one new command, `list_presentation_history(serviceId)`
(`commands.rs`), wrapping the existing `persistence::list_presentation_items`
with no status filter — returns every item (Prepared/Active/Stopped) ever
created for that service. No presentation *behavior* changed: nothing is
ever displayed automatically, and `prepare_to_activate`/
`commit_activation`/`stop_active_item` are untouched.

## 11. Content history

Audited and found already reused correctly: "Saved Content"
(`listAcceptedContentCandidates`) already exists in Diagnostics.
`ContentCandidate` itself remains in-memory only (a deliberate,
documented Phase 2.7 decision, unrelated to this phase's scope) — not
changed here; see "Deferred work."

## 12. Cross-domain relationships

Already fully, honestly represented — `CorrelationKind` variants
(`ScriptureSermon`, `ScriptureMusic`, `SermonMusic`, `ThemeScripture`,
`ThemeMusic`, `ServiceTransition`, `SermonContent`,
`MultiDomainConvergence`, `TemporalProximity`) are each produced by a
plain, deterministic rule function in `cross_domain.rs` — zero ML/LLM.
`IntelligenceCorrelation` carries `assertion_level`/`confidence`/
`evidence`/`source_finding_ids` end-to-end into the frontend
(`domain/intelligence.ts`'s `IntelligenceCorrelation`, field-for-field
matching). No fabricated relationship exists anywhere in this engine.
`CorrelationKind::SharedContext` is a reserved, currently-unused variant —
noted, not implemented (out of this phase's scope; not fabricated).

## 13. Navigation

`App.tsx` gained a small `<nav className="app-nav">` tab strip (Live
Service / Bible / Music / History), local `useState`, no router
dependency. `LiveChurchBrain` renders exactly as it always did when "Live
Service" is selected — nothing inside it was restructured.

## 14. Security

Re-audited against the full diff: no new Tauri capability, no CSP change,
no new network dependency (`cargo tree` unchanged), no secret/credential
committed, no dangerous shell command, no new filesystem access outside
the existing SQLite database path. See
`pilot-evidence/3.6/software/automated-regression.json`.

## 15. Licensing

No copyrighted Bible translation or song library was imported. The one
Music Library empty-state explicitly refuses to present the 5-song
fictional dev fixture as a real church library — the exact "test fixture
≠ production data" discipline this phase's hard rules require.

## 16. Offline operation

Unchanged — every new command is a local SQLite read/write; no network
call was added anywhere.

## 17. Performance

`list_bible_books` performs 66 in-process `get_book` calls inside one
Tauri command (not 66 separate IPC round-trips) — the same pattern
`check_bible_integrity` already used internally. Chapter browsing reuses
`search_bible`'s existing single-query chapter path (never loads all
31,086 verses into the frontend). Presentation/Service History list calls
are bounded (`listTimeline`/`listTranscript` already take a `limit`).

## 18. Failure recovery

`delete_saved_scripture` is idempotent by design (returns whether a row
existed, never errors on a double-delete — verified by test). An empty
Bible search, an invalid reference, and a missing/never-imported Music
dataset all degrade to an honest empty state, never a crash — verified by
the Music Library's explicit empty-state path and the existing
`search_bible`/`build_scripture_slide` error handling (unchanged,
already tested for malformed/unavailable references).

## 19. Test matrix

**Rust (`apps/desktop/src-tauri`)**: 4 `saved_scriptures` persistence
tests (create/retrieve, verse range, ordering, idempotent delete), 2
`build_scripture_slide` verse-range tests (dev-fixture data — a real
range and a rejected inverted range), 1 real-BSB-dataset Phase 3.6
acceptance test (`content::tests::phase_3_6_bible_library_acceptance_against_the_real_bsb_dataset`
— 66 books/39 OT/27 NT/1,189 total chapters, plus range presentation
verified across John, Psalms, and 1 Corinthians using real imported
text).

**Frontend**: 12 new tests in `lib/libraryHelpers.test.ts`
(`referenceFor`, `parseVerseRange`, `presentationHeading`) — this
project's established pure-function testing convention (no
`@testing-library/react` dependency exists or was added; see the
`testingApproach` note in the evidence JSON for why a DOM-rendering
library was judged disproportionate for this phase).

**Regression**: every pre-existing Rust and frontend test still passes
unmodified (776 Rust / 191 frontend total, up from 769 / 179 — the
difference is entirely new tests, zero removed or weakened).

## 20. PROVEN

- Full BSB dataset genuinely queryable end-to-end: React → Tauri command
  → `SqliteBibleProvider` → SQLite → back to the frontend, for search,
  book browse, chapter browse, and (now) verse ranges — traced and tested
  against the real dataset, not the dev fixture.
- Saved Scripture create/retrieve/delete durability (SQLite commit
  durability, the same proof pattern every other persistence test in this
  codebase uses).
- Presentation/Service History retrieval for a past, non-live service.
- Verse-range presentation bug fixed and verified against real BSB text
  in three different books.
- Full automated regression green (Rust + frontend), zero test weakened.

## 21. PARTIAL

- Music Library: search and honest status reporting are real and
  functional; browsing/opening an individual song's lyrics was not built
  (see §24).

## 22. NOT AVAILABLE

- A real, licensed, production song dataset — none exists in this
  repository or was fabricated for this phase.
- Music findings/content candidates/cross-domain correlations surviving
  an app restart — a pre-existing, deliberate architecture decision from
  Phase 2.7/2.9, unrelated to and unchanged by this phase.
- Sermon History scoped to an arbitrary past `service_id` — audited and
  found to share the same "hardcoded to the live service" gap as
  Presentation History did, but not fixed this phase (see §24).

## 23. NOT VERIFIED

- Real Windows / physical hardware / human-operator usability of the new
  Bible/Music/History screens — no physical Windows machine, screenshot
  tool, or human operator was available in this container, for the same
  reason recorded in every prior hardware-pilot phase (3.1–3.5.1). Xvfb
  proves startup/rendering correctness only, never physical usability.

## 24. Deferred work (explicit, justified, not started)

- **Full song browse/detail UI** (`get_song`/`get_lyrics`/`get_sections`
  wired to new commands): the backend primitives already exist and are
  already tested; wiring them was deferred because the only data to
  browse today is a 5-song fictional fixture — building a polished
  browse UI against fabricated content was judged higher-risk (looks like
  a real library, isn't) than valuable, per this phase's explicit "never
  hide an unavailable capability behind fake data" rule. Revisit once a
  real licensed dataset exists.
- **Song save/reuse persistence**: no "songs this church has sung"
  history table was created, for the same reason — no real data exists
  to make it meaningful yet, and audited to require a genuinely new table
  (unlike Presentation History, nothing existing could be reused for
  this).
- **Sermon History for a past `service_id`**: `list_sermon_history` is
  still hardcoded to the live service, mirroring the exact gap
  Presentation History had before this phase. Left for Phase 3.7 — the
  fix pattern (add an optional `service_id` parameter to an existing,
  already-`service_id`-aware persistence function) is now well
  established by this phase's Presentation History work.

## 25. Phase 3.7 handoff

What's ready: three real library/history screens reachable from
top-level navigation, all built on existing commands plus three narrowly
justified additions, zero backend contract broken, full regression green.
What's explicitly left for a future phase: sermon-history-by-service_id,
a real licensed song dataset (whenever one becomes legally available) and
the song browse/detail/save UI that would sit on top of it, and any
further History polish (e.g. surfacing music/content-candidate/
correlation history, which remains architecturally blocked on the
pre-existing in-memory-only decision documented in §22 until a future
phase deliberately revisits that decision).

---

## Release Gate

```
SOFTWARE:
    PASS

BIBLE LIBRARY:
    PASS

MUSIC LIBRARY:
    LEGALLY BLOCKED (no licensed production dataset exists; empty-state
    and search-reuse architecture is real and functional)

SERVICE HISTORY:
    PASS

PRESENTATION HISTORY:
    PASS

OPERATOR WORKFLOW:
    PASS (Environment A/B - automated + Xvfb smoke test)

LICENSING:
    PASS

OFFLINE:
    PASS

SECURITY:
    PASS

PHASE 3.6:
    GO WITH CONDITIONS
```

**Conditions:** real Windows/physical-hardware/human-operator validation
of the new Bible/Music/History screens remains NOT VERIFIED (see §23) —
carried forward as an open condition, consistent with every prior
hardware-pilot phase's honest reporting, not a blocker to this phase's
own software-level completeness.
