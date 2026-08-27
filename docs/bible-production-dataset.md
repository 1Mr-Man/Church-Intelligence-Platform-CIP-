# Real Bible Dataset — Production Import Milestone

This document describes CIP's first complete, legally-documented,
offline, production Bible dataset — the release-readiness milestone that
replaces the tiny six-verse development fixture with a real, complete,
66-book translation the intelligence and presentation pipeline can
actually operate on end to end.

**This is not a new intelligence phase.** It does not touch
`core/intelligence`, Music/Service/Sermon/Cross-Domain Intelligence, or
the presentation display architecture. It answers one question: *can CIP
take a real, complete, legally-installed Bible translation all the way
from dataset → search → detection → context → presentation →
display?*

## Selected translation: Berean Standard Bible (BSB)

| | |
|---|---|
| **Selected translation** | Berean Standard Bible (BSB) |
| **Exact edition** | Standard text (no interlinear/Strong's), 66 books, as transcribed in `github.com/lyteword/bsb` commit `808caa6` |
| **Source** | `github.com/lyteword/bsb` (git-clonable, CC0 1.0 Universal), text originating from the Berean Bible project (bereanbible.com / berean.bible) |
| **License** | Public Domain |
| **Redistribution status** | `VerifiedPublicDomain` |
| **Why selected** | A directly reachable, git-clonable, formally-licensed (CC0), structurally complete (66/66 books, 1189 chapters) source existed for BSB within this environment's network constraints; no equivalently strong, directly reachable source was found for WEB in the time available (see below) |
| **Bible Hub verification role** | None in the acquisition path — biblehub.com was unreachable in this environment and was never scraped; it is referenced only as background (BSB's historical association with the Bible Hub team) |
| **Why WEB was not selected** | `ebible.org` (the World English Bible's own primary distribution site) is blocked by this environment's network egress policy, as are `biblehub.com`, `bereanbible.com`/`berean.bible`, and `en.wikipedia.org`. Only `raw.githubusercontent.com` and the `git` protocol to `github.com` were reachable. No equivalently well-documented, directly git-clonable, CC0/public-domain-licensed WEB transcription repository was located and verified in the time available. WEB remains a strong future candidate if a comparable acquisition path is found. |

Full evidence chain: [`docs/data/bible/BSB/BSB-LICENSE.md`](data/bible/BSB/BSB-LICENSE.md).
Machine-readable manifest: [`docs/data/bible/BSB/manifest.json`](data/bible/BSB/manifest.json).

## Bible Hub usage policy (repository-wide)

**Bible Hub is a reference/verification/comparison source. It is never
CIP's runtime Bible provider, and it was never scraped to build this
dataset.** Display rights (Bible Hub showing a translation to a human
reader) are not redistribution rights, and nothing in this codebase
treats them as equivalent. The only things Bible Hub may legitimately be
used for: confirming a translation exists, checking book/chapter/verse
references, comparing a small sample of verses for accuracy, and general
translation-metadata research — never as the acquisition path for bulk
text, and never as authorization to redistribute a copyrighted
translation. See `docs/data/bible/BSB/BSB-LICENSE.md`'s evidence chain
for the honest accounting of what was and wasn't directly verified.

## The complete flow

```
AUTHORITATIVE SOURCE (lyteword/bsb, CC0, git-clonable)
        |
   LICENSE CHECK (docs/data/bible/BSB/BSB-LICENSE.md; LicensingStatus::VerifiedPublicDomain)
        |
   SOURCE VALIDATION (66/66 book dirs present, 1189 chapter files, uniform per-book naming)
        |
   NORMALIZATION (Markdown -> BibleDatasetInput JSON; one-time, offline, in this session)
        |
   66-BOOK VALIDATION (cip_core_bible::check_bible_integrity)
        |
   IMPORTER (cip_integrations_bible::import_bible_dataset - transactional, licensing-gated)
        |
   CONTENT REGISTRY (cip_core_content::ContentRegistry - bible:BSB)
        |
   CHECKSUM + VERSION (FNV-1a over sorted content; dataset_version "bsb-1.0")
        |
   SQLITE (bible_translations/bible_books/bible_chapters/bible_verses - unchanged schema)
        |
   INTEGRITY CHECK (same check_bible_integrity, against the real SqliteBibleProvider)
        |
   BIBLE SEARCH (cip_core_bible::search_bible - exact/chapter/range/free-text)
        |
   REFERENCE DETECTION (cip_core_bible::detection - unchanged)
        |
   CONTEXT / CHAPTER / RANGE (cip_core_bible::context_manager / range - unchanged)
        |
   PRESENTATION RENDERING (presentation::build_scripture_slide - unchanged)
        |
   LOCAL DISPLAY (existing display-window foundation - unchanged)
        |
   END-TO-END ACCEPTANCE (content::tests::phase_real_bible_dataset_full_validation)
```

Every stage after "NORMALIZATION" reuses existing, already-tested CIP
architecture unchanged in shape — only the licensing gate and the
dataset asset itself are new.

## Licensing safety gate

`cip_core_content::LicensingStatus` is a five-variant enum:
`VerifiedPublicDomain`, `VerifiedRedistributable`, `LicensedForCip`,
`Unknown`, `Restricted`. Only the first three permit bulk import
(`LicensingStatus::permits_bulk_import()`); `Unknown` and `Restricted`
never do, regardless of any other field. This is enforced inside
`cip_integrations_bible::import_bible_dataset` itself — the lowest,
hardest-to-bypass layer — **before any row is validated, let alone
written**: an unverified or restricted translation is refused with zero
database mutation (`ImportError::LicensingNotVerified`), proven by
`refuses_import_when_licensing_status_is_unknown_and_writes_nothing` and
`refuses_import_when_licensing_status_is_restricted_and_writes_nothing`.
`ContentMetadata` (the existing Content Registry, extended, never
duplicated) carries `licensing_status` alongside the pre-existing
free-text `license`/`distribution` fields: those record what a source
*said*; `licensing_status` records what CIP has *concluded*, and never
silently upgrades from `Unknown`.

`TranslationInput.licensingStatus` (JSON: `"licensingStatus"`) is a
**required** field on every Bible dataset import — including the
existing, general-purpose "Import a Bible dataset" flow the Content
Registry panel already exposed before this milestone, so the hard gate
applies uniformly, not just to this one production import.

## Development fixture vs. production dataset

| | Development fixture (unchanged) | Production dataset (this milestone) |
|---|---|---|
| Content | 6 hand-picked verses (Romans 8:18,28-31; John 3:16) | Complete 66-book, 1189-chapter, 31,086-verse BSB |
| Location | `database/seeds/dev_seed.sql` (raw SQL) + `core/bible::fixtures::FakeBibleProvider` (Rust-test-only) | `database/datasets/bsb/bsb.json` (real dataset asset) |
| Translation id | `KJV` | `BSB` |
| Applied | Deliberately, only outside Production (`apply_dev_seed`) | Every launch, every environment, idempotently (`content::import_and_register`) |
| Licensing | `Unknown` (dev seed never recorded real provenance) | `VerifiedPublicDomain` |
| Purpose | Deterministic, offline unit tests | Real church use |

`DEFAULT_TRANSLATION_ID` (`state.rs`) remains `"KJV"` — **unchanged** by
this milestone, to keep every existing test's assumptions intact (see
"Regression protection" below). Every command that resolves a
translation (`search_bible`, `preview_scripture`, `preview_presentation`,
`prepare_presentation`, `create_manual_presentation`) already accepted
(or now accepts) an optional `translationId` parameter that defaults to
`DEFAULT_TRANSLATION_ID` when omitted — passing `"BSB"` explicitly
selects the real production dataset for that operation, with **no
silent fallback**: a disabled or unknown translation id is rejected
explicitly, never silently substituted.

## Disabled-translation safety fix

Auditing this milestone found one genuine, narrow gap: `search_bible`
did not check the Content Registry's `enabled`/`disabled` status at all
(only `list_bible_translations` filtered it, for the picker list) — an
operator could still search a translation explicitly disabled from the
Content Registry panel. This has been fixed: every translation-resolving
command now calls a shared `ensure_translation_selectable` guard before
doing anything else, matching the exact fail-open discipline
`is_translation_selectable` already established (a missing/errored
registry lookup never blocks; only an explicit `Disabled` record does).

## Dataset statistics

| | |
|---|---|
| Books | 66 / 66 |
| Chapters | 1,189 |
| Verses | 31,086 |
| Checksum (FNV-1a) | `d4335582ff26a3ac` |
| Dataset version | `bsb-1.0` |
| Asset size | ~5.3 MB (JSON) |

**Performance (release build, throwaway probe, deleted before commit):**
full transactional import of all 31,086 verses: **634.8ms**; idempotent
re-import (all 31,086 rows already present): 551.6ms; 1,000x exact verse
lookup: 5.5ms total (~5.5μs each); 100x chapter lookup: 6.1ms total; 100x
verse-range lookup: 2.8ms total; one free-text search: 11.3ms - all well
within real-time operator interaction budgets.

The verse count (31,086) differs slightly from the commonly-cited KJV
figure (~31,102) — expected and investigated, not treated as a defect:
different translations legitimately differ in verse segmentation/
numbering in a handful of disputed passages. The acceptance criteria
this milestone actually checks are canonical coverage, valid references,
no duplicates, and complete chapter/verse structure — all of which the
imported dataset satisfies (see "66-book validation result" below).

## 66-book validation result

`cip_core_bible::check_bible_integrity` (already existing, unchanged)
run against the real imported dataset via the real `SqliteBibleProvider`:
**`IntegrityStatus::Valid`**, 66/66 books present, canonical ordering
correct, zero issues (no duplicate books/chapters/verses, no empty
text, no zero/negative numbering, no gaps flagged since none exist in a
complete dataset). "No unexpected books" is structurally guaranteed
upstream by the importer itself, which only ever accepts a book that
`cip_core_bible::book_alias::canonicalize_book` resolves against the
same 66-book canonical catalog used everywhere else in CIP — a row for
any other "book" is rejected at the row-validation stage, before it
could ever reach storage. "Every verse/chapter references a valid
book/chapter" is enforced by the database schema's own `FOREIGN KEY`
constraints (`bible_chapters -> bible_books`, `bible_verses ->
bible_chapters`), unconditionally, at the SQLite level.

## Content Registry result

Registered as `bible:BSB`: `name` "Berean Standard Bible", `version`
`bsb-1.0`, `language` `en`, `license` "Public Domain", `licensingStatus`
`verified_public_domain`, `checksum` `d4335582ff26a3ac`, `status`
`enabled` — set to `enabled` only because the import already passed
every validation gate (licensing, structural, transactional) by the time
the registry is written; nothing here can mark a dataset enabled before
that.

## Import / idempotency result

Proven directly (`content::tests::phase_real_bible_dataset_full_validation`):
first import inserts all 31,086 verses (`imported: 31086, alreadyPresent:
0`); a second import of the identical dataset inserts nothing
(`imported: 0, alreadyPresent: 31086`), with an identical checksum both
times. The write path (`bible_translations`/`bible_books`/
`bible_chapters`/`bible_verses` inserts) is wrapped in a single SQL
transaction (`Connection::unchecked_transaction`) for both atomicity (a
mid-import failure leaves the database exactly as it was) and
performance (one commit instead of tens of thousands of autocommits).

## Search, reference detection, context, chapter/range verification

All proven against the real imported dataset through the real
`SqliteBibleProvider` (not a fixture) in the same acceptance test:
exact verse lookup (Genesis 1:1, John 3:16, Romans 8:28/31, Psalm 23:1,
Revelation 22:21, Matthew 1:1), chapter lookup (1 Corinthians 13 returns
exactly its 13 verses), verse-range lookup (Romans 8:28-31 returns
verses 28-31 in order; the inverted range 8:31-28 is rejected, never
silently reordered), free-text search, and invalid-reference rejection
(Romans 8:999, a nonexistent book). Reference detection, context
retention, and the alias system (`core/bible::detection`/
`context_manager`/`book_alias`) are **unchanged code** already covered
by their own existing test suites; this milestone did not modify them,
only proved the production dataset flows through them correctly via the
same `BibleProvider` trait every other phase already depends on.

## Bible Intelligence, presentation, and local display

`BibleIntelligenceEngine` (`core/service`/`core/intelligence`) depends
only on the `BibleProvider` trait and `core/bible::search`/`detection` —
unchanged code, so it automatically operates on whichever translation a
call site passes it, including the real BSB dataset, with zero
modification. `presentation::build_scripture_slide` (unchanged) proven
directly against BSB: the real imported John 3:16 text ("For God so
loved the world that He gave His one and only Son...") survives
unchanged through `PresentationContent` and `RenderedSlide`. The local
presentation display foundation (a prior milestone) is untouched;
displaying a BSB-sourced item follows the exact same
`Prepared -> Active -> Stopped` path any other prepared item does — see
[`docs/presentation.md`](presentation.md).

## Offline operation

No new dependency was added anywhere in the workspace (`git diff --stat`
on every `Cargo.toml`/`Cargo.lock` is empty for this milestone), and
`cargo tree -p cip-desktop` shows no network-capable crate. The dataset
is compiled directly into the binary (`include_str!` in
`apps/desktop/src-tauri/src/bible_production_dataset.rs`) — Bible
search/detection/context/presentation/display never depend on network
access, before or after installation, matching every earlier phase's
offline discipline.

## Distribution model

**Bundled in the repository** (`database/datasets/bsb/bsb.json`,
checked into git) — chosen because the text is verified public domain
(per the evidence chain above), so nothing prevents CIP from
redistributing it as part of the application source, and bundling keeps
the release-readiness promise ("offline after installation") true
without requiring a separate user-run download step. The import itself
still runs through the exact same `import_and_register`/
`import_bible_dataset` path a user-provided import would use (see
`docs/bible-datasets.md`) — it is simply invoked automatically, once,
idempotently, at every application launch (`lib.rs::setup()`), rather
than requiring a manual Content Registry panel action.

## Update procedure (documented limitation)

If a future BSB release changes the underlying text, this milestone
does **not** yet implement automatic version-diff/update detection
beyond what the checksum already gives for free: importing an updated
dataset with a **different** `dataset_version`/content would produce a
different checksum, and because every insert is `INSERT OR IGNORE`
against the existing `(translation_id, book, chapter, verse)` key, a
changed verse's *old* text would **not** be silently overwritten — the
existing content stays exactly as it is. There is no historical
version-chain storage (old-vs-new comparison, migration between
versions) implemented yet. This is an honest, explicitly documented gap,
not a claim of update automation that doesn't exist.

## PROVEN

- Complete 66-book, 1189-chapter, 31,086-verse BSB dataset installed and
  validated (`IntegrityStatus::Valid`)
- Licensing metadata verified and recorded (`VerifiedPublicDomain`,
  full evidence chain documented)
- Source provenance recorded (exact GitHub commit, license files read
  directly, cross-corroborated by a second independent source)
- Deterministic checksum (`d4335582ff26a3ac`), reproducible from the
  checked-in asset
- Content Registry populated and enabled only after successful
  validation
- Import is transactional and idempotent (proven against real SQLite)
- Translation isolation proven (BSB never returns KJV content or vice
  versa, even with both present in the same database)
- Disabled-translation safety proven (`search_bible` now blocks a
  disabled translation explicitly; re-enabling restores it)
- Exact verse / chapter / range / free-text search proven against the
  real provider
- Invalid references rejected (never a false match)
- Bible Intelligence, presentation rendering, and the local display
  foundation all operate on the real dataset via unchanged existing code
  paths
- Offline: no new dependency, dataset compiled into the binary
- Real desktop runtime launch verified under Xvfb (see the
  implementation report's "Desktop runtime verification" section)

## NOT AVAILABLE / NOT VERIFIED

- A second complete translation (WEB or otherwise) — out of scope for
  this milestone
- Multilingual datasets
- Commercial translation redistribution (NIV/ESV/NASB/etc. remain
  structurally un-importable by the licensing gate)
- Any remote Bible API or online Bible Hub runtime dependency (never
  built; the policy above forbids it)
- Direct, first-party verification of `berean.bible/licensing.htm` (the
  network policy in this environment blocked it — see the evidence
  chain's honest accounting)
- Physical projector/display hardware verification (this environment has
  none; the local display foundation's own docs already document this
  limitation and it is unchanged by this milestone)
- Automatic future-update/version-diff handling beyond checksum-based
  change detection
- Bible Hub side-by-side comparison sample beyond the internal
  cross-corroboration described in the evidence chain (Bible Hub itself
  was unreachable in this environment)

## Validating the production dataset

```sh
cargo test -p cip-core-content                                  # LicensingStatus + gate unit tests
cargo test -p cip-integrations-bible                             # importer, incl. licensing gate + transaction
cargo test -p cip-integrations-content                           # SqliteContentRegistry licensing_status round-trip
cargo test -p cip-desktop --lib content::tests::phase_real_bible_dataset_full_validation
                                                                  # the primary milestone acceptance test
cargo test -p cip-desktop --lib bible_production_dataset         # embedded-asset parse check
```
