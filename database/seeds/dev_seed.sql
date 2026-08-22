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
    ('KJV', 'ROM', 8, 18,
     'For I reckon that the sufferings of this present time are not worthy to be compared with the glory which shall be revealed in us.'),
    ('KJV', 'ROM', 8, 28,
     'And we know that all things work together for good to them that love God, to them who are the called according to his purpose.'),
    ('KJV', 'ROM', 8, 29,
     'For whom he did foreknow, he also did predestinate to be conformed to the image of his Son, that he might be the firstborn among many brethren.'),
    ('KJV', 'ROM', 8, 30,
     'Moreover whom he did predestinate, them he also called: and whom he called, them he also justified: and whom he justified, them he also glorified.'),
    ('KJV', 'ROM', 8, 31,
     'What shall we then say to these things? If God be for us, who can be against us?');

INSERT INTO services (id, title, status, started_at, ended_at)
VALUES ('00000000-0000-0000-0000-000000000001', 'Seed Sunday Service', 'ended',
        '2026-01-04T14:00:00Z', '2026-01-04T15:30:00Z');

-- Phase 2.1 music fixture. Deliberately fictional/test song titles and
-- lyrics rather than any real hymn's text - clearly marked test data, not
-- a claim about any real work's copyright status (see
-- docs/music-datasets.md's licensing policy). Three datasets, to
-- demonstrate: dataset-isolated song numbers (both "music:dev-hymnbook"
-- and "music:dev-worship-set" use song number "120" for two entirely
-- different songs), a language distinction within one dataset, and a
-- disabled dataset (registered disabled at the Content Registry level by
-- `apps/desktop/src-tauri/src/music.rs`, never used for automatic
-- recognition).

INSERT INTO music_songs (id, content_id, title, normalized_title, song_type, language, number, author, composer, status)
VALUES
    ('h1', 'music:dev-hymnbook', 'Test Fixture Hymn One', 'test fixture hymn one', 'hymn', 'en', '120', NULL, NULL, 'enabled'),
    ('h2', 'music:dev-hymnbook', 'Test Fixture Hymn Two', 'test fixture hymn two', 'hymn', 'en', '121', NULL, NULL, 'enabled'),
    ('h3', 'music:dev-hymnbook', 'Cancion De Prueba', 'cancion de prueba', 'hymn', 'es', NULL, NULL, NULL, 'enabled'),
    ('w1', 'music:dev-worship-set', 'Different Fixture Song Same Number', 'different fixture song same number', 'worship_song', 'en', '120', NULL, NULL, 'enabled'),
    ('d1', 'music:dev-disabled-set', 'Disabled Fixture Song', 'disabled fixture song', 'hymn', 'en', '1', NULL, NULL, 'enabled');

INSERT INTO music_aliases (content_id, song_id, alias, normalized_alias)
VALUES ('music:dev-hymnbook', 'h1', 'First Fixture Hymn', 'first fixture hymn');

INSERT INTO music_sections (id, content_id, song_id, kind, sequence)
VALUES ('h1-v1', 'music:dev-hymnbook', 'h1', 'verse', 0);

INSERT INTO music_lyrics (content_id, song_id, section_id, sequence, text, normalized_text)
VALUES
    ('music:dev-hymnbook', 'h1', 'h1-v1', 0, 'This is a test hymn about steadfast care', 'this is a test hymn about steadfast care'),
    ('music:dev-hymnbook', 'h1', 'h1-v1', 1, 'Every day I see new signs of faithful love', 'every day i see new signs of faithful love'),
    ('music:dev-hymnbook', 'h2', NULL, 0, 'This is an unrelated test hymn about a different theme', 'this is an unrelated test hymn about a different theme'),
    ('music:dev-hymnbook', 'h3', NULL, 0, 'Esta es una linea de prueba en espanol', 'esta es una linea de prueba en espanol'),
    ('music:dev-worship-set', 'w1', NULL, 0, 'A worship chorus used only for dataset isolation testing', 'a worship chorus used only for dataset isolation testing'),
    ('music:dev-disabled-set', 'd1', NULL, 0, 'This song belongs to a disabled dataset for testing', 'this song belongs to a disabled dataset for testing');
