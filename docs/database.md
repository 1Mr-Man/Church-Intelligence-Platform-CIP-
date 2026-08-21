# Local database

CIP is local-first: every install owns a single SQLite file on local disk.
There is **no required cloud database, no Supabase dependency, and no
external database server requirement** - the app must fully function
offline.

## Where the database lives

`apps/desktop/src-tauri/src/config.rs`'s `AppConfig::resolve` puts it at
`<Tauri app data dir>/cip.sqlite3` (e.g.
`~/.local/share/org.churchintelligence.cip/cip.sqlite3` on Linux). Nothing
in `core` or `integrations/*` hard-codes this path - it's resolved once,
in the app shell, and passed down.

## Migrations

`database/migrations/*.sql` are ordered, numbered SQL files, embedded into
the binary at compile time (`include_str!` in
`database/src/migrations.rs`) so the app never depends on migration files
being present on disk at runtime. `cip_database::run_migrations` applies
whichever migrations a `schema_migrations` tracking table says haven't run
yet, each inside its own transaction - safe to call on every app startup,
including against an already-current database (a no-op).

Adding a migration: add a new `NNNN_description.sql` file, add a matching
entry to the `MIGRATIONS` array in `database/src/migrations.rs`. Never edit
an already-applied migration file - add a new one.

`0002_live_speech_detail.sql` (Phase 1.2) added the columns the live
pipeline needed that Phase 1.0's schema didn't yet have:
`transcript_segments.sequence_number`/`.language`/`.speaker_id` and
`scripture_detections.detection_type`/`.source_text` - see
[`docs/live-speech.md`](live-speech.md).

## Schema

See [`database/schema/README.md`](../database/schema/README.md) for the
full table reference and conventions (ID strategy, timestamp format,
confidence-score columns, JSON payload columns). The Phase 1 tables:

`services`, `transcript_segments`, `bible_translations`, `bible_books`,
`bible_chapters`, `bible_verses`, `scripture_detections`, `ai_suggestions`,
`presentation_items`, `audit_events`.

## Seed data

`database/seeds/dev_seed.sql` inserts one Bible translation (KJV), six
verses (John 3:16 and Romans 8:18, 28, 29, 30, 31 - enough to exercise the
Bible Intelligence Core's context/sequential-verse behavior, see
[`docs/bible-intelligence.md`](bible-intelligence.md)), and one sample
service. It is **not** a full Bible dataset, and it is never applied
automatically in `production`;
in `development`/`test` the app shell applies it once, on first launch,
guarded by a check that `bible_translations` is empty (see `lib.rs`'s
`setup` hook) so re-launching the app doesn't try to re-insert it.

Apply it manually against any connection with
`cip_database::seed::apply_dev_seed(&conn)`.

## Validating the database layer

```sh
cargo test -p cip-database          # migration idempotency + all 10 tables exist
cargo test -p cip-integrations-bible # SqliteBibleProvider against the real schema
cargo test -p cip-integration-tests  # full cross-domain flow through the DB
```
