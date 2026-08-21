-- Development/test seed data.
--
-- Deliberately NOT a full Bible dataset - Phase 1 only needs enough rows to
-- prove the schema and query layer work end to end (foreign keys, unique
-- constraints, join paths). Populating a complete translation is a later,
-- explicitly separate task (Bible Intelligence Engine).
--
-- Not applied automatically by the migration runner; run it deliberately
-- via `cip_database::seed::apply_dev_seed`.

INSERT INTO bible_translations (id, name, abbreviation, language, is_local)
VALUES ('KJV', 'King James Version', 'KJV', 'en', 1);

INSERT INTO bible_books (translation_id, code, name, testament, chapter_count, book_order)
VALUES
    ('KJV', 'JHN', 'John', 'new', 21, 43),
    ('KJV', 'ROM', 'Romans', 'new', 16, 45);

INSERT INTO bible_chapters (translation_id, book_code, chapter_number, verse_count)
VALUES
    ('KJV', 'JHN', 3, 36),
    ('KJV', 'ROM', 8, 39);

INSERT INTO bible_verses (translation_id, book_code, chapter_number, verse_number, text)
VALUES
    ('KJV', 'JHN', 3, 16,
     'For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.'),
    ('KJV', 'ROM', 8, 28,
     'And we know that all things work together for good to them that love God, to them who are the called according to his purpose.'),
    ('KJV', 'ROM', 8, 31,
     'What shall we then say to these things? If God be for us, who can be against us?');

INSERT INTO services (id, title, status, started_at, ended_at)
VALUES ('00000000-0000-0000-0000-000000000001', 'Seed Sunday Service', 'ended',
        '2026-01-04T14:00:00Z', '2026-01-04T15:30:00Z');
