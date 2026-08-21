# Schema reference

This directory documents the schema; it is not the source of truth for it.
The source of truth is the ordered SQL files in `../migrations/`. Nothing
here is executed automatically.

## Phase 1 tables

| Table                 | Purpose                                                              |
| ---------------------- | --------------------------------------------------------------------- |
| `services`             | One row per live-service session (`ServiceSession`).                 |
| `transcript_segments`  | Speech-to-text output, linked to a service.                          |
| `bible_translations`   | Metadata for an installed Bible translation.                         |
| `bible_books`          | Book metadata per translation.                                       |
| `bible_chapters`       | Chapter metadata per book.                                           |
| `bible_verses`         | Verse text per chapter.                                              |
| `scripture_detections` | Scripture references detected in a transcript, pending confirmation. |
| `ai_suggestions`       | AI-produced suggestions of any kind, pending human review.           |
| `presentation_items`   | Queued/active/stopped items in the presentation queue.                |
| `audit_events`         | Append-only log of domain events, categorized (see logging docs).    |
| `content_registry`     | One row per locally-installed content item (Bible today), with provenance/licensing metadata and enabled/disabled status (Phase 1.5, see `docs/content-registry.md`). |

## Conventions

- **IDs**: domain-event tables (`services`, `transcript_segments`,
  `scripture_detections`, `ai_suggestions`, `presentation_items`,
  `audit_events`) use TEXT UUID primary keys generated in application code.
  Bible content tables use surrogate `INTEGER` ids since they are
  provider-populated, not user-created.
- **Timestamps**: ISO-8601 TEXT in UTC (`chrono`'s default
  `DateTime<Utc>` serialization).
- **Confidence**: any row produced by inference stores both
  `confidence_score` (raw `0.0..=1.0`) and `confidence_level`
  (`low`/`medium`/`high`), mirroring `cip-core-confidence::ConfidenceResult`
  so a caller never has to recompute the bucket after loading a row.
- **JSON payload columns** (`ai_suggestions.payload`,
  `presentation_items.content`, `audit_events.payload`): used where the
  shape legitimately varies by a `kind`/`content_type`/`event_name`
  discriminator column, matching the corresponding Rust
  `#[non_exhaustive]` enum in `core/*`. Everything with a fixed, known
  shape gets real columns instead.

See `../migrations/0001_initial_schema.sql` for the authoritative DDL,
including foreign keys, `CHECK` constraints, and indexes.
