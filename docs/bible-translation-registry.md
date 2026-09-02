# The Bible Translation Registry

This document names, as one system, something this codebase has already
built piece by piece: **the Bible Translation Registry** - the
combination of the `bible_translations`/`content_registry` tables, the
`LicensingStatus` gate, and a per-translation evidence folder under
`docs/data/bible/<TRANSLATION_ID>/` that together decide which
translations CIP is allowed to hold and distribute, and prove why.

The operating principle, stated once and enforced in code, not just
policy: **start with legally clear datasets, and only ever add a
copyrighted translation later through a real, evidenced licensing
arrangement - never by assumption, never by omission.**

## What the registry is made of (already built)

| Piece | Where | What it does |
|---|---|---|
| `BibleTranslation` | `core/bible/src/provider.rs` | The runtime identity of a translation (id, name, abbreviation, language, `is_local`). |
| `content_registry` table | `database/migrations/0005_content_registry.sql`, extended by `0009_content_licensing_status.sql` | One row per translation, carrying `licensing_status`, `license`, `copyright`, `publisher`, `source`, `checksum`, `status` (enabled/disabled). |
| `LicensingStatus` | `core/content/src/lib.rs` | `VerifiedPublicDomain` \| `VerifiedRedistributable` \| `LicensedForCip` \| `Unknown` \| `Restricted` - see below. |
| The import gate | `cip_integrations_bible::import_bible_dataset` | Refuses to write **anything** - zero database mutation - unless `licensing_status` is one of the first three. `Unknown` and `Restricted` are hard stops, not warnings. |
| Per-translation evidence folder | `docs/data/bible/<ID>/` | A human-readable `<ID>-LICENSE.md` evidence chain (source, rights statement, what was and wasn't directly verified) plus a machine-readable `manifest.json` (source commit, checksum, licensing status, verse counts). |

This is not a proposal - it is the exact shape of what already ships.
`docs/bible-datasets.md` documents the general importer/gate;
`docs/bible-production-dataset.md` documents the first entry (BSB) in
full, including its own evidence chain and manifest.

## Current registry contents

| Translation | Status | License | Evidence |
|---|---|---|---|
| Berean Standard Bible (BSB) | `VerifiedPublicDomain` | Public Domain (CC0 1.0, per source repository) | [`docs/data/bible/BSB/BSB-LICENSE.md`](data/bible/BSB/BSB-LICENSE.md), [`manifest.json`](data/bible/BSB/manifest.json) |

Every other translation a `search_bible`/manual-search UI might name
(KJV, NIV, ESV, etc.) is **not** in this registry and cannot be, until
it clears the same gate - `core/bible`'s reference detector and Bible
Intelligence pipeline recognize translation *abbreviations* in spoken/
typed text (so a pastor saying "in the NIV..." still parses correctly),
which is a text-parsing concern, entirely separate from whether that
translation's *text* is actually installed and licensed. Parsing a name
is not permission to hold the content.

## Adding translation #2 (or #N): the playbook

Two paths, matching the two ways a translation can legitimately enter
the registry - **never skip straight to writing the importer call.**

### Path A — public domain / permissively licensed (the BSB path)

1. Identify a source and verify its license directly - read the actual
   `LICENSE` file or an explicit, dated public-domain/CC0 dedication.
   Corroborate from a second independent source when the primary
   publisher's own site isn't directly reachable (exactly as BSB's
   evidence chain does with two independent GitHub repositories).
2. Write `docs/data/bible/<ID>/<ID>-LICENSE.md` - the same shape as
   `BSB-LICENSE.md`: source, acquisition path, rights statement (quoted,
   not paraphrased), and an honest "evidence chain" table naming exactly
   what was and wasn't directly verified.
3. Write `docs/data/bible/<ID>/manifest.json` - the same fields as
   BSB's: `translation_id`, `source`, `source_reference`,
   `source_commit`/`source_commit_date`, `licensing_status` (must be
   `verified_public_domain` or `verified_redistributable`), `license`,
   `checksum`, verse/chapter/book counts.
4. Build the `BibleDatasetInput` with `translation.licensingStatus` set
   to match, and run it through `import_bible_dataset` exactly as
   documented in `docs/bible-datasets.md` - the gate will refuse the
   import outright if the evidence and the claimed status don't line up
   with what a reviewer set.
5. Update this document's "Current registry contents" table.

### Path B — a real licensing agreement (`LicensedForCip`)

`LicensingStatus::LicensedForCip` exists in the enum today specifically
for this path, and nothing in this codebase sets it automatically - by
design, this status can only be entered by a human with an actual signed
agreement in hand, never inferred from a dataset file's own claims. The
evidence folder for a `LicensedForCip` translation must additionally
contain:

- the actual licensing/permission agreement (or a redacted reference to
  where it is held, if the agreement itself cannot be committed to a
  public repository) - grantor, grantee, effective date, scope
  (offline/local redistribution, church/organizational use, etc.), and
  any field/attribution requirements the agreement imposes;
- the specific rights holder's name and the channel through which
  permission was obtained (e.g. a signed license, a written email grant
  - whichever the agreement actually is);
- an explicit statement of what CIP is and is not permitted to do under
  that agreement (e.g. "bundle in the installer" vs. "operator must
  download separately after purchasing a license elsewhere").

Until such an agreement exists and is evidenced this way, a commercial
translation's status is `Unknown` (nothing set) or, once its rights are
positively known to be restrictive with no license on file, `Restricted`
- both of which the import gate refuses unconditionally. This is not a
placeholder waiting to be relaxed; it is the permanent behavior for any
translation lacking evidenced permission.

## Why this matters beyond Bible text

The same `LicensingStatus`/`ContentMetadata`/evidence-folder pattern
already generalizes to any content type `core/content::ContentType`
names (worship songs, hymns, church-original content - see the second
uploaded advice document's own "Christian Song Intelligence Engine" and
"CCLI/SongSelect integration" sections, both future, not-yet-started
gaps). When a Song/Hymn registry is eventually built, it should reuse
this exact registry shape (gate + evidence folder), not invent a new
one - the licensing discipline is a property of `core/content`, already
shared infrastructure, not something specific to Bible text.

## Cross-references

- [`docs/bible-datasets.md`](bible-datasets.md) - the general importer/
  format/validation architecture and the licensing policy statement.
- [`docs/bible-production-dataset.md`](bible-production-dataset.md) -
  the full BSB decision record this document generalizes from.
- [`docs/content-registry.md`](content-registry.md) - the
  `content_registry` table and `ContentRegistry` trait mechanics.
- [`docs/phase-4-master-plan-gap-audit.md`](phase-4-master-plan-gap-audit.md) -
  names the master plan's own "Massive Bible-version architecture"
  section (Translation Registry, multi-language) as a gap this document
  partially closes for the licensing-discipline half of that gap; multi-
  translation *breadth* (adding KJV/NIV/ESV/etc. themselves) remains
  future work, gated by this same playbook.
