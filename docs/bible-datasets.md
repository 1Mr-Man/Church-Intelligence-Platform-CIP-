# Bible Datasets (Phase 1.5)

This document explains how a local Bible translation gets onto disk: the
importer, the dataset file format, validation, idempotency, versioning,
and the integrity checker - and, first and most importantly, the
licensing policy that governs all of it.

**For CIP's first complete, production 66-book translation (Berean
Standard Bible) - the exact selection/licensing decision record, dataset
statistics, and end-to-end validation results - see
[`docs/bible-production-dataset.md`](bible-production-dataset.md).**
This document (`bible-datasets.md`) still describes the general-purpose
importer/format/validation architecture that milestone reuses unchanged.

**For the registry this policy and importer together implement - what's
currently in it, and the exact playbook for adding a translation #2
(public-domain or, later, under a real licensing agreement) - see
[`docs/bible-translation-registry.md`](bible-translation-registry.md).**

## Licensing policy - read this first

**CIP does not bulk-download or scrape Bible translations from the
Internet, does not bypass licensing restrictions, and does not add a
copyrighted translation merely because it is technically accessible.**
This is a hard constraint on the system, not just a guideline for this
phase.

A Bible dataset may only be imported when one of the following is true:

- the user provides the data themselves,
- the data is public domain,
- the license explicitly permits the intended (local, offline) use, or
- the project has explicit distribution permission from the rights
  holder.

The importer has no network access at all (see "Offline" below), so it
cannot download anything even if asked to - but the policy holds even for
data a user supplies locally: nothing here validates *license terms*
(that requires human judgment), only *data shape*. When a dataset's
licensing metadata is not known, it is recorded as `null`/`UNKNOWN` (see
[`docs/content-registry.md`](content-registry.md)) rather than guessed -
never assume permissive licensing just because a field was left blank.

**Hard production safety gate (added by the real Bible dataset
milestone):** every `BibleDatasetInput` now carries a required
`translation.licensingStatus` field
(`cip_core_content::LicensingStatus` - `VerifiedPublicDomain`/
`VerifiedRedistributable`/`LicensedForCip`/`Unknown`/`Restricted`), and
`import_bible_dataset` refuses to write anything at all - zero database
mutation - unless it is one of the first three. This applies to every
import through this path, not just the production BSB dataset: a
translation whose rights are unverified (`Unknown`) or explicitly
restricted (`Restricted`, e.g. a mainstream commercial translation with
no license on file) can never enter the database via this importer,
regardless of who invokes it. See
[`docs/bible-production-dataset.md`](bible-production-dataset.md#licensing-safety-gate)
for the full design and test coverage.

## The importer

`cip_integrations_bible::import_bible_dataset(conn, dataset)` is a
reusable, local Bible dataset importer. It is the *only* way Bible
content tables (`bible_translations`/`bible_books`/`bible_chapters`/
`bible_verses`) are populated outside the tiny development seed
(`database/seeds/dev_seed.sql`, unchanged by this phase).

It never touches the filesystem and never accepts raw SQL - see
"Security" below.

### Dataset file format

A dataset is one JSON object, `BibleDatasetInput`:

```json
{
  "translation": {
    "id": "KJV",
    "name": "King James Version",
    "abbreviation": "KJV",
    "language": "en",
    "publisher": null,
    "copyright": null,
    "license": "public domain",
    "distribution": "public domain",
    "datasetVersion": "1.0"
  },
  "verses": [
    { "book": "Romans", "chapter": 8, "verse": 28, "text": "And we know that all things work together for good..." },
    { "book": "ROM", "chapter": 8, "verse": 29, "text": "For whom he did foreknow..." },
    { "book": "John", "chapter": 3, "verse": 16, "text": "For God so loved the world..." }
  ]
}
```

`translation.publisher`/`.copyright`/`.license`/`.distribution` are all
optional (`null`/omitted means unknown - never guess). Every other
`translation` field and every `verses[]` field is required.

`book` accepts a canonical name, a known alias, or the book's own stable
code (`"Romans"`, `"Rom"`, `"Rom."`, or `"ROM"` all resolve to the same
book) - it's canonicalized through the exact same 66-book catalog
`core/bible::book_alias` uses everywhere else in the system (speech
detection, search), so the importer never introduces a second,
conflicting book list (section 12's "one authoritative canonical
catalog" requirement). See "A book-alias gap this phase fixed" below.

The frontend's import UI reads the selected file as text with
`FileReader` and sends the raw JSON string to the `import_bible_dataset`
Tauri command - the command never receives or opens a file path itself.

### Validation

Fatal (aborts the whole import, nothing is written):

- empty/missing `translation.id`, `.name`, `.abbreviation`, `.language`,
  or `.datasetVersion`.

Per-row (that row is skipped and reported; every other valid row still
imports):

- an unrecognized book (fails `canonicalize_book`),
- chapter or verse number `0` (chapters/verses are 1-indexed; malformed),
- empty/whitespace-only verse text,
- a verse that duplicates an earlier row in the *same* dataset (book,
  chapter, and verse all equal).

Nothing is silently repaired - an invalid row is reported with a specific
reason, never guessed into a "close enough" value.

### Idempotency

Every insert uses SQLite's `INSERT OR IGNORE`. Re-running the identical
import a second time inserts nothing new - every row is reported
`alreadyPresent` instead of `imported`, and the row count in
`bible_verses` is unchanged. This is proven directly:
`reimporting_the_identical_dataset_creates_no_duplicate_rows`
(`integrations/bible/src/importer.rs`).

**Never blindly overwriting existing content**: if a verse
`(translation, book, chapter, verse)` already exists, its text is left
exactly as it is, even if a newer dataset has different text for that
same reference. A changed dataset only ever *adds* what's missing; it
never silently replaces what's already there. If a translation's content
genuinely needs to be replaced, that's a deliberate human decision (e.g.
disable the old content, register a new translation id or dataset
version) - not something the importer does on your behalf.

### The import report

Every import call returns a deterministic `ImportReport` - every number
derived from the actual dataset and the database's own change-count,
never hard-coded:

```
translationId, datasetVersion, books, chapters, versesTotal,
imported, alreadyPresent, invalid, errors[], checksum
```

For the tiny development fixture (6 verses across 2 books), a clean
first import reports `imported: 6, alreadyPresent: 0, invalid: 0`. A
production-sized dataset (tens of thousands of verses) would report
proportionally - this project has never claimed, and does not claim
here, numbers for a complete 31,102-verse KJV dataset, since no such
dataset has been supplied to it in this environment (see "What's
actually installed" below).

### Checksum & dataset versioning

Each import computes a deterministic FNV-1a hash over the translation id
and every valid `(book, chapter, verse, text)` row, sorted - identical
content always produces the same checksum regardless of row order, so a
re-import of unchanged content is detectable without depending on
insertion order. No new dependency was added for this; FNV-1a is
implemented directly (a few lines) since this is a change-detection
signal, not a security primitive.

A dataset's identity is deliberately simple: `translation.id` +
`datasetVersion` (a plain string, e.g. `"1.0"`, later `"1.1"`) +
`checksum`, all recorded in the Content Registry (see
[`docs/content-registry.md`](content-registry.md)). There is no
migration/versioning *infrastructure* here - just enough identity to
answer "which version of this dataset is installed."

## The integrity checker

`cip_core_bible::check_bible_integrity(provider, translation_id)` checks
what's actually stored for one translation, entirely through the
`BibleProvider` trait - it never hard-codes canonical Bible facts (no
"Romans has 16 chapters, chapter 8 has 39 verses" table anywhere). Doing
so would mean inventing content this crate has no authoritative source
for, and would also misclassify any legitimate partial dataset as
broken.

It checks, for whatever books/chapters/verses are actually present:

- book presence against the 66-book canonical catalog,
- chapter and verse numbers are never zero or duplicated,
- verse text is never empty,
- `book_order` values are self-consistent with the canonical book
  ordering.

It deliberately does **not** require chapters or verses to start at 1 or
have no gaps - a development fixture with only Romans 8:18 and 28-31 is
exactly the "legitimate partial dataset" case, not a defect.

Three statuses:

- **Valid** - every one of the 66 canonical books is present and
  everything checked is internally consistent.
- **Incomplete** - nothing inconsistent was found, but not every
  canonical book is present (the development fixture is always this).
- **Invalid** - a structural defect was found (duplicate, empty text,
  malformed/zero numbering, or a book-ordering inconsistency).

This directly distinguishes a development fixture from a complete
canonical dataset, as required: the seeded KJV fixture (2 books) reports
`Incomplete`, `booksPresent: 2`, `booksExpected: 66`, zero issues -
never `Invalid`, and never claimed to be a complete Bible.

## A book-alias gap this phase fixed

While building the importer and search dispatcher, both of which resolve
a book by calling `canonicalize_book`, testing surfaced a real gap:
`canonicalize_book` only matched a book's canonical *name* or its curated
*aliases* - never its own stable *code*, unless that code happened to
also be listed as an alias. Most codes are (`"ROM"` -> alias `"rom"`),
but ~15 aren't: `"1SA"`'s aliases are `"1 sam"`/`"1 sa"` (with a space),
never the bare `"1sa"`; `"SNG"` has no `"sng"` alias at all. A dataset
that identified books by their stable code - exactly what this importer
documents as accepting - would silently fail to import ~15 of 66 books.

Fixed in `core/bible::book_alias::canonicalize_book` by also matching a
book's `code` directly (case-insensitively), independent of its alias
list. This only *adds* a new way to match; it changes nothing about
spoken-text detection (`detect_candidates`'s book-name regex is built
from `name`/`aliases` only, never `code`, so this has zero effect on the
Phase 1.1 speech-detection behavior). See
`book_alias::tests::a_code_not_listed_as_its_own_alias_still_resolves`.

## Translation-aware `BibleProvider`

Every lookup takes an explicit `translation_id` and is scoped to exactly
that translation - requesting an installed translation succeeds;
requesting one that isn't installed (e.g. `NIV` when only `KJV` exists)
returns "not found," never a silent fallback to whatever *is* installed.
Proven directly:
`presentation::tests::rejects_an_unavailable_translation_rather_than_substituting_one`
(Phase 1.4) and
`search::tests::requesting_an_unavailable_translation_finds_nothing_rather_than_falling_back`
(this phase).

### Chapter and verse-range retrieval

`BibleProvider::get_chapter` (Phase 1.0) already retrieves a complete
chapter in canonical verse order - re-verified this phase with a
dedicated fixture-based test. `core/bible::get_verse_range` is new: a
free function (not a new trait method - it composes `get_chapter` and
filters, so no existing `BibleProvider` implementation needed to change)
retrieving an inclusive verse range in canonical order, rejecting an
inverted range (e.g. `"Romans 8:31-28"`) with an explicit
`InvalidRange` error rather than an empty or silently-reordered result.

## Local Bible search

`core/bible::search_bible(provider, translation_id, query)` is the
minimum local Bible search `core/search::SearchEngine`'s contract leaves
for each domain to fill in, built entirely on `BibleProvider` and this
crate's own reference detection - no network, no new search
infrastructure (SQLite's `LIKE` is still what backs free-text matches).
It dispatches:

- `"Romans 8:28"` -> a single verse,
- `"Romans 8:28-31"` -> a verse range,
- `"Romans 8"` -> a whole chapter,
- anything else -> free-text search.

A query is only ever treated as a reference if it parses as *one*,
covering the whole (normalized) input - `"tell me about Romans 8:28"`
falls through to free text rather than guessing which part of a longer
sentence was "the reference." Every result identifies its
`translationId` explicitly (`BibleSearchResult`) - a search never mixes
text from two translations without saying which is which, and an exact
reference/chapter/range match reports `relevance: 1.0` while a free-text
match reports `relevance: null` (`BibleProvider::search`'s `LIKE` lookup
has no real ranking signal - this is left honestly absent, never
fabricated).

## Offline

The importer, integrity checker, and search dispatcher add no new
dependency with any network capability - verified structurally via
`cargo tree` (no `reqwest`/`hyper`/`ureq`/`curl` anywhere in
`cip-integrations-bible`, `cip-core-bible`, `cip-core-content`, or
`cip-integrations-content`'s dependency graphs), the same proof Phase
1.2 established for the detection pipeline.

## Security

- Never executes an imported file.
- Never trusts an arbitrary filesystem path - the backend command
  (`import_bible_dataset`) takes JSON *text*, not a path; the frontend
  reads the file client-side.
- Never accepts SQL from a dataset file - every value is bound through
  `rusqlite` parameters, never string-interpolated into a query.
- Never silently overwrites an existing dataset (see "Idempotency"
  above).
- Never downloads unverified Bible content - there is no code path that
  fetches a dataset from the network at all.

## What's actually installed

As of the real Bible dataset production import milestone, two
translations install into every non-Production launch's database (and
one - the production dataset - into every launch including Production):

- `database/seeds/dev_seed.sql`'s tiny KJV fixture (2 books, 6 verses),
  auto-registered with every licensing field `UNKNOWN` (never applied in
  Production; the dev seed never recorded real provenance) - unchanged
  by this milestone, see [`docs/content-registry.md`](content-registry.md).
- The complete, 66-book Berean Standard Bible (`bible:BSB`, `licensingStatus`
  `verified_public_domain`), installed idempotently at every launch in
  every environment via `content::import_and_register` - see
  [`docs/bible-production-dataset.md`](bible-production-dataset.md) for
  the full selection/licensing/validation record.

The importer, integrity checker, and search dispatcher are validated
against the real dev fixture, the real complete BSB dataset, and a
larger synthetic dataset built purely for performance measurement (see
[`docs/full-service-validation.md`](full-service-validation.md)'s
performance section).
