-- Phase 2.1 music content foundation.
--
-- The music-domain counterpart to 0001's bible_translations/bible_books/
-- bible_chapters/bible_verses tables: one row per song within a dataset,
-- its structural sections (verse/chorus/bridge/...), and its lyric lines.
-- Every table is scoped by content_id (the Content Registry id, e.g.
-- "music:dev-hymnbook" - see 0005_content_registry.sql) so a song id or
-- song number is never looked up without naming which dataset it means -
-- two datasets can both have a song "120" that are entirely different
-- songs (see docs/music-datasets.md).
--
-- Aliases are a separate table (not a JSON array column) so they can be
-- indexed for lookup the same way titles are - matching this schema's
-- existing preference for real columns/tables over JSON blobs wherever
-- the shape is fixed and known (see database/schema/README.md's
-- conventions section).

CREATE TABLE music_songs (
    id               TEXT NOT NULL,
    content_id       TEXT NOT NULL,
    title            TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    song_type        TEXT NOT NULL
                         CHECK (song_type IN (
                             'hymn', 'worship_song', 'chorus', 'gospel_song',
                             'psalm', 'anthem', 'spiritual_song', 'other'
                         )),
    language         TEXT NOT NULL,
    number           TEXT,
    author           TEXT,
    composer         TEXT,
    status           TEXT NOT NULL DEFAULT 'enabled' CHECK (status IN ('enabled', 'disabled')),
    PRIMARY KEY (content_id, id)
);
CREATE INDEX idx_music_songs_normalized_title ON music_songs(content_id, normalized_title);
CREATE INDEX idx_music_songs_number ON music_songs(content_id, number);

CREATE TABLE music_aliases (
    content_id       TEXT NOT NULL,
    song_id          TEXT NOT NULL,
    alias            TEXT NOT NULL,
    normalized_alias TEXT NOT NULL,
    -- Lets the importer use INSERT OR IGNORE for idempotent re-import,
    -- the same discipline every other Phase 2.1/1.5 importer follows.
    UNIQUE (content_id, song_id, normalized_alias),
    FOREIGN KEY (content_id, song_id) REFERENCES music_songs(content_id, id) ON DELETE CASCADE
);
CREATE INDEX idx_music_aliases_normalized ON music_aliases(content_id, normalized_alias);

CREATE TABLE music_sections (
    id         TEXT NOT NULL,
    content_id TEXT NOT NULL,
    song_id    TEXT NOT NULL,
    kind       TEXT NOT NULL
                   CHECK (kind IN (
                       'verse', 'chorus', 'bridge', 'refrain',
                       'stanza', 'intro', 'outro', 'other'
                   )),
    sequence   INTEGER NOT NULL,
    PRIMARY KEY (content_id, id),
    FOREIGN KEY (content_id, song_id) REFERENCES music_songs(content_id, id) ON DELETE CASCADE
);
CREATE INDEX idx_music_sections_song ON music_sections(content_id, song_id, sequence);

-- section_id is nullable - a song this dataset didn't structure into
-- named sections still needs its lyric lines to be storable (see
-- docs/music-intelligence.md's "not every song follows the same
-- structure" note). SQLite's multi-column foreign key match is
-- automatically satisfied when either referencing column is NULL, so a
-- NULL section_id never requires a matching music_sections row.
CREATE TABLE music_lyrics (
    content_id      TEXT NOT NULL,
    song_id         TEXT NOT NULL,
    section_id      TEXT,
    sequence        INTEGER NOT NULL,
    text            TEXT NOT NULL,
    normalized_text TEXT NOT NULL,
    PRIMARY KEY (content_id, song_id, sequence),
    FOREIGN KEY (content_id, song_id) REFERENCES music_songs(content_id, id) ON DELETE CASCADE,
    FOREIGN KEY (content_id, section_id) REFERENCES music_sections(content_id, id) ON DELETE SET NULL
);
CREATE INDEX idx_music_lyrics_normalized ON music_lyrics(content_id, normalized_text);
