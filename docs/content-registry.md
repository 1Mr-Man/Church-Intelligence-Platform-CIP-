# Content Registry (Phase 1.5)

This document explains the Content Registry: a general, source-agnostic
answer to "what local content exists?" - Phase 1.5's content/dataset
foundation underneath the existing Bible Intelligence/presentation
pipeline.

## Why this exists

Before this phase, "what Bible translations are installed" meant
querying `bible_translations` directly - fine for Bible content alone,
but not a pattern a future engine (music, sermon media, reference
material) could reuse without coupling to Bible-specific tables. The
Content Registry is the general version of that question: one metadata
record per installed content item, regardless of which domain-specific
tables actually hold the content itself.

Phase 1.5 only *populates* the Bible category. Music/Service/Media/
Reference exist as the closed set of categories a future phase would
populate - explicitly not implemented as engines here (see
[`README.md`](../README.md)'s phase boundary).

## Domain model

`core/content` (a new domain crate, mirroring `core/bible`'s
provider/adaptor split):

```rust
pub enum ContentType { Bible, Music, Service, Media, Reference }
pub enum ContentStatus { Enabled, Disabled }

pub struct ContentMetadata {
    pub id: String,              // "<type>:<domain-id>", e.g. "bible:KJV"
    pub content_type: ContentType,
    pub name: String,
    pub version: String,         // a simple dataset identity, e.g. "1.0"
    pub language: String,
    pub source: String,          // e.g. "user-provided import", "development fixture"
    pub publisher: Option<String>,   // None = UNKNOWN, never guessed
    pub copyright: Option<String>,
    pub license: Option<String>,
    pub distribution: Option<String>,
    pub imported_at: DateTime<Utc>,
    pub checksum: Option<String>,
    pub status: ContentStatus,
}

pub trait ContentRegistry: Send + Sync {
    fn list(&self, content_type: Option<ContentType>) -> Result<Vec<ContentMetadata>, ContentRegistryError>;
    fn get(&self, content_id: &str) -> Result<Option<ContentMetadata>, ContentRegistryError>;
    fn register(&self, metadata: &ContentMetadata) -> Result<(), ContentRegistryError>;
    fn set_enabled(&self, content_id: &str, enabled: bool) -> Result<(), ContentRegistryError>;
}
```

Every field describing a real-world fact CIP cannot independently verify
- `publisher`/`copyright`/`license`/`distribution` - is `Option<String>`.
`None` means *unknown*, recorded honestly. Nothing anywhere in this
system invents a value for one of these fields; see
[`docs/bible-datasets.md`](bible-datasets.md)'s licensing policy.

`integrations/content::SqliteContentRegistry` is the one Phase 1.5
implementation, backed by the new `content_registry` table
(`database/migrations/0005_content_registry.sql`, additive-only, see
[`docs/database.md`](database.md)). `register()` is an upsert on the
*metadata* row only - it never touches the actual content a registration
describes (e.g. Bible verse text), which has its own, separate,
never-silently-overwritten discipline.

## The `id` convention

`"<type>:<domain-id>"` - e.g. `"bible:KJV"`. This is an application-level
convention (`apps/desktop/src-tauri/src/content.rs::bible_content_id`),
not a database constraint, so a future content type is free to choose
its own scheme without a schema change.

## Enable / disable, never delete

Disabling content (`set_content_enabled`) never deletes it - only its
`status` changes. Disabled content stops appearing in normal
selection/search (`list_bible_translations` filters it out - see below)
but a service that already used it while it was enabled remains fully
understandable in history: nothing about a past `presentation_items` row
or suggestion changes when its source content is later disabled.

## Fail-open filtering

`list_bible_translations` filters out a translation only when its
Content Registry entry explicitly says `Disabled`. A translation with
**no** registry entry at all, or a registry read error, is never hidden
- Phase 1.5's bookkeeping catching up late must never silently make an
already-installed, already-working translation disappear. This is a
directly unit-tested guard function
(`commands::is_translation_selectable`), not just inline logic: see
`commands::tests::is_translation_selectable_fails_open_on_missing_or_errored_registry_lookups`.

## The dev-seed fixture's own registration

On startup (development/test environments only, mirroring the existing
dev-seed guard in `lib.rs`), the dev-seeded KJV translation is registered
in the Content Registry if it isn't already:
`source: "development fixture"`, `version: "dev-fixture"`, every
licensing field `None`/`UNKNOWN` - the dev seed never recorded real
provenance, so nothing here invents any. Registering it is a no-op if an
entry already exists (e.g. a real dataset was later imported over it) -
it never overwrites a more complete registration
(`content::register_dev_seed_content_if_missing`).

## Provenance and traceability

Every Bible verse used in a suggestion or presentation item already
carries its `translation_id` (Phase 1.0/1.4). Given a `translation_id`,
its full provenance is one Content Registry lookup away:
`bible:<translation_id>` -> publisher/copyright/license/distribution/
dataset version/checksum. Phase 1.4's `PresentationItem` already records
`source_suggestion_id` and `template`; combined with the verse's
`translation_id` and this registry lookup, "where did this Scripture
text come from" is always answerable:

```
Romans 8:28 -> translation_id "KJV" -> content_registry "bible:KJV"
  -> dataset version, checksum, license (or UNKNOWN)
  -> source_suggestion_id (if automatic) -> the original transcript segment
```

No verse text is duplicated into the registry itself to make this work -
the registry holds metadata only; the text stays exactly where it
already lived (`bible_verses`), one join away.

## Tauri commands

| Command | Purpose |
| --- | --- |
| `list_content_registry(contentType?)` | What's installed, optionally filtered to one category |
| `get_content_metadata(contentId)` | One item's full metadata |
| `set_content_enabled(contentId, enabled)` | Enable/disable without deleting |
| `import_bible_dataset(datasetJson)` | Import + register in one call (see `docs/bible-datasets.md`) |
| `check_bible_dataset_integrity(translationId)` | Structural integrity check (see `docs/bible-datasets.md`) |

Every command validates its own input and returns `AppError` on failure,
matching every earlier command in this codebase.

## Frontend

`LiveChurchBrain.tsx` gained a **Content Registry** diagnostics
panel: installed datasets (name, language, version, enabled/disabled,
license/publisher/distribution or `UNKNOWN`), an Enable/Disable toggle,
a "Check Integrity" action per Bible item, and a local-file dataset
import workflow (`<input type="file">` + `FileReader`, reading the file
in the browser - the backend command never touches the filesystem
itself). This is diagnostics and local content management, not an
administrator CMS - no bulk operations, no remote content browsing.

## Extensibility

Nothing about `ContentRegistry`/`ContentMetadata` assumes Bible content
specifically - a future Music or Sermon engine registers its own
`ContentMetadata` rows (`content_type: Music`/`Sermon`, its own `id`
prefix) against the same table and trait, without a schema change and
without `core/content` needing to know anything about music or sermon
domain internals. This phase deliberately implements only the Bible
category; the rest is the reserved shape, not implemented engines (see
`README.md`'s explicit "not implemented" list).
