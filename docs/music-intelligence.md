# Music Intelligence (Phase 2.1)

This document explains the Music Intelligence Foundation added in Phase
2.1: the first real *second* intelligence domain built on top of the
Phase 2.0 shared architecture ([`docs/intelligence-architecture.md`](intelligence-architecture.md)).
Read that document first - this one only explains what's specific to
Music, not the shared `IntelligenceEngine`/`IntelligenceContext`/
`FindingQueue` contracts Music reuses unchanged.

**Phase 2.2 added real acoustic (audio-fingerprint) recognition** on top
of everything below, fused with this text-matching path rather than
replacing it - see [`docs/acoustic-music.md`](acoustic-music.md). This
document still accurately describes the text-matching path unchanged.

> **Roadmap note.** This repository's authoritative Phase 2 roadmap places
> Music Intelligence at 2.3 (following the 2.2 Music Content Foundation),
> with Service Intelligence at 2.4 and Sermon Intelligence Foundation at
> 2.5 - see [`docs/service-intelligence.md`](service-intelligence.md) and
> [`docs/sermon-foundation.md`](sermon-foundation.md). This document's own
> "Phase 2.1" heading is a historical label from before that roadmap was
> adopted and is not rewritten; nothing about the Music Intelligence
> implementation described below has changed.

**Not in this phase:** real audio fingerprinting, a large copyrighted
song database, sermon intelligence, or automatic presentation of a
recognized song. This subsystem recognizes song titles, aliases, hymn/
song numbers, and lyric text from a transcript - nothing more.

## What "song recognition" means here, honestly

CIP recognizes songs by matching **text** - a spoken title, a hymn
number, or lyric words that were transcribed - against a locally
installed dataset. It does **not** listen to audio and identify a song
by its acoustic signature ("Shazam-style" fingerprinting). These are
different capabilities, and this codebase never blurs them:

- `cip_core_music::MatchType::Acoustic` existed in this type since Phase
  2.1 for exactly this reason: so a future real acoustic recognizer would
  have somewhere to put its output without a breaking change. **Phase
  2.2 is that future phase** - `Acoustic` is now genuinely constructed,
  by `core/music::fusion`, from a real `AcousticRecognitionCandidate` a
  real `AcousticMusicRecognizer` returned. See
  [`docs/acoustic-music.md`](acoustic-music.md) for the full
  architecture; this document (Phase 2.1) still describes only the
  text-matching path below.
- `cip_core_intelligence::acoustic_recognition_available()` still
  returns `false`, unchanged - but that was always a narrower claim than
  "no acoustic recognizer could ever be wired into this application":
  it means only that `MusicIntelligenceEngine::analyze()` (the
  transcript-text path) itself never performs acoustic matching. Phase
  2.2's separate `analyze_acoustic` entry point is the real acoustic
  path - see [`docs/acoustic-music.md`](acoustic-music.md).
- The architecture (`Transcript -> Music Intelligence -> Music Findings
  -> Operator -> Presentation`) was deliberately the same shape a future
  `Audio -> Acoustic Recognition Engine -> Music Intelligence -> Findings`
  path would use, so adding real acoustic recognition later was additive,
  not a redesign - and Phase 2.2 proves that: no change to this path was
  needed to add the acoustic one alongside it.

## Architecture

```
TRANSCRIPT SEGMENT
    v
MusicIntelligenceEngine::analyze()      (core/intelligence/src/music_adapter.rs)
    |  translates transcript text -> a MusicQuery
    v
cip_core_music::search_songs()          (core/music/src/matcher.rs - all real logic)
    |  dispatches by query shape (title/alias/number/lyric/lyric sequence)
    |  against a MusicProvider, scoped to enabled datasets
    v
SongRecognitionCandidate[]              (evidence-carrying, ranked, never a bare Song)
    v
IntelligenceFinding[]                   (domain: Music, kind: Music)
    v
FindingQueue                            (AppState.intelligence_findings, in-memory)
    v
OPERATOR ACCEPT / REJECT                (never automatic)
```

Like [`BibleIntelligenceEngine`](intelligence-architecture.md#24-bible-compatibility-the-one-real-engine),
`MusicIntelligenceEngine` is a thin adapter, not a second place recognition
logic lives. Every candidate comes from `cip_core_music::search_songs`;
the adapter's own job is strictly translation (transcript text -> a
`MusicQuery`, and a `SongRecognitionCandidate` -> an `IntelligenceFinding`)
plus two things that genuinely belong at the intelligence-integration
layer rather than in `core/music` itself:

- **Dataset enablement.** `MusicProvider` has no concept of "enabled" -
  that's the Phase 1.5 Content Registry's job. The engine reads which
  `Music`-typed, `Enabled` datasets exist from
  `context.content_metadata` (already part of the shared
  `IntelligenceContext`) and only ever searches those.
- **Song continuity.** Reads the single most recent `Music`-domain
  finding out of `context.recent_findings` (already bounded) and
  classifies continuity via `cip_core_music::classify_continuity`.

Non-negotiable, and proven by `core/intelligence/src/acceptance_tests.rs`'s
Phase 2.1 section: Music never calls the Bible engine directly, Bible
never calls Music directly, and Music never calls a (nonexistent) Sermon
engine directly. The only channel between engines is what the
orchestrator puts into `IntelligenceContext.recent_findings` - the same
rule Phase 2.0 established, now proven with two *real* engines
registered simultaneously, not just synthetic test engines.

## The domain model (`core/music`)

`Song` / `SongType` (hymn, worship_song, chorus, gospel_song, psalm,
anthem, spiritual_song, other) / `SongSection` / `SectionKind` (verse,
chorus, bridge, refrain, stanza, intro, outro, other) / `LyricLine` -
the music-domain counterpart to `core/bible`'s book/chapter/verse model,
with one deliberate difference: every table is scoped by `content_id`
(a Content Registry dataset id), because a song id or number is only
ever meaningful *within* its dataset. Two datasets can both have a song
`"120"` that are entirely different songs - proven directly
(`integrations/music`'s `dataset_isolation_prevents_cross_dataset_lookup`,
and the dev-seed fixture below).

`MusicProvider` (the trait `core/music` depends on, never SQLite
directly) mirrors `BibleProvider`'s shape: `search_title`,
`search_alias`, `search_number`, `search_lyrics`, `get_sections`,
`get_lyrics`, `list_datasets` - every method explicitly takes
`content_id`.

## Text normalization

`cip_core_music::normalize::normalize_for_matching` is a documented,
narrow policy: Unicode-aware lowercasing, curly-quote/apostrophe
stripping, dash-variant-to-space, all other punctuation dropped,
whitespace collapsed. It is explicitly **not** phonetic guessing, not
stemming, not English-only, not word-substituting - the same "no fuzzy
magic" discipline `core/bible::normalize` uses for spoken number words,
applied to lyric/title text instead.

## Deterministic matching (`core/music::matcher`)

No machine learning, no randomness - every confidence value is a
documented formula over the query and what `MusicProvider` returned.
Base confidence by match type, before any distinctiveness modulation:

| Match type | Base confidence |
| --- | --- |
| Explicit title | 0.97 |
| Alias | 0.93 |
| Song number | 0.90 |
| Multiple consecutive lyric lines | 0.85 |
| Exact lyric (single line) | up to 0.80, modulated by distinctiveness |
| Partial lyric | up to 0.55, modulated by distinctiveness |
| Contextual (continuity only) | 0.35 |
| Acoustic | not from this table - the recognizer's own reported score, fused via noisy-or (Phase 2.2, see [`docs/acoustic-music.md`](acoustic-music.md)) |

Title/alias/number/multi-line matches are structural (exact-equality or
exact-adjacency), so they are not modulated. A single-line lyric match
*is* modulated by **distinctiveness** - a generic phrase ("we praise
you") shared across many songs is much weaker evidence than a long
phrase unique to one song:

```
distinctiveness(word_count, songs_matched) =
    (0.4 * min(word_count / 8, 1.0) + 0.6 * (1 / max(songs_matched, 1)))
    .clamp(0.05, 1.0)
```

Candidates are always sorted `(confidence desc, song_id asc)` - ranking
never depends on `HashMap`/`HashSet` iteration order (see
`matcher_tests.rs`'s `determinism`/`ranking_order` tests).

### Multi-line lyric matching

Two (or more) consecutive transcript segments' text is checked for
strictly consecutive `sequence` adjacency within the same song (and
section, if any) - a real proof that "line 1 then line 2" is stronger
evidence than either line alone. If no song has a consecutive match,
this falls back to treating the most recent line as an ordinary
single-phrase lyric query; a partial match is still useful evidence
even without full-sequence adjacency.

### Ambiguity - first-class, never a silent pick

```rust
pub struct MatchThresholds {
    pub minimum_confidence: f32,  // 0.25 - below this, discarded entirely
    pub ambiguity_margin: f32,    // 0.08 - top-two this close is ambiguous
}
```

`is_ambiguous(candidates, thresholds)` reports `true` when the top two
ranked candidates' confidence differ by less than the margin. The engine
never auto-selects in that case - operator confirmation is required
(see "Findings and the operator workflow" below).

## Interpreting free transcript text as a music query

A live transcript segment is unstructured spoken text - unlike Bible
detection (which has its own dedicated `core/bible::detection` module),
Phase 2.1 does not build a comparable general-purpose "music utterance
parser." `MusicIntelligenceEngine` instead uses one deterministic,
documented dispatch order, honestly a heuristic rather than a claim of
natural-language understanding:

1. **Title/alias**: try the whole segment text as an exact title/alias
   query first.
2. **Song/hymn number**: look for a trigger word (`"number"`, `"hymn"`,
   `"song"`, `"take"`) immediately followed by a run of digits.
3. **Lyric, possibly multi-line**: if the immediately preceding
   transcript segment also produced no title/number match, both lines
   are tried together as a lyric sequence before falling back to the
   current line alone.

## Song continuity

`SongContinuity` (`Unknown` / `ContinuingSameSong` / `NewSong` /
`PossibleSongChange`) is classified from the single most recent
Music-domain finding in `context.recent_findings` - deliberately
reusing the shared context's existing bounded history rather than a
second history mechanism. The previous finding's song id is carried
forward inside its own `evidence` (an `EvidenceSource::Context` entry
following a documented `"song_id:<id>"` convention), not a new field on
`IntelligenceFinding` - Phase 2.1 explicitly avoids "a parallel finding
model."

## Findings and the operator workflow

A `MusicIntelligenceEngine::analyze()` call never emits more than 5
findings (`MAX_FINDINGS_PER_CALL`), and when the result is ambiguous,
only candidates within the ambiguity margin of the top score are
emitted - never padding the queue with weak also-rans just because the
top two were close. Findings are deduplicated within one call by
`(source, song_id)`.

Findings reuse the Phase 2.0 `FindingStatus` lifecycle
(`Detected -> Reviewed -> Accepted`/`Rejected`/`Expired`) unchanged -
there is no separate music-specific state machine. `FindingQueue::accept`/
`reject` change only a finding's own `status`; neither has any way to
create a `PresentationItem` (a structural, type-level fact: `music.rs`
and `FindingQueue` have no dependency on `cip_core_presentation` at
all).

**Hard requirement, proven at every layer**: music recognition never
automatically creates a presentation item. An operator who wants to
project a recognized song's lyrics still uses the existing, separate
manual/Bible presentation commands - accepting a music finding is a
review decision, nothing more.

## Tauri commands (`apps/desktop/src-tauri`)

- `search_music(query, queryType, contentIds?)` - manual title/number/
  lyric search, works with no active service (mirrors `search_bible`).
  Defaults to currently-enabled Music datasets; an explicit
  `contentIds` lets the operator search a specific (even disabled)
  dataset, same as `search_bible` accepting any `translationId`.
- `import_music_dataset(datasetJson)` - see
  [`docs/music-datasets.md`](music-datasets.md).
- `analyze_music_transcript(text)` - the deterministic manual/test
  harness, the Music counterpart to `process_test_transcript`. Persists
  `text` as an ordinary transcript segment (so a later call's multi-line
  continuity has real history), builds a real `IntelligenceContext`, and
  calls the registered Music engine directly - never routed through the
  Bible pipeline. Queues genuinely new findings; never touches
  presentation.
- `list_music_findings()` - pending findings for the active service.
- `accept_music_finding(findingId)` / `reject_music_finding(findingId)`
  - the operator decision.

There is no separate `list_music_datasets` command - Music datasets are
ordinary Content Registry entries, so `list_content_registry("music")`
already serves that purpose.

## Persistence: findings stay in-memory (a deliberate decision)

Phase 2.0 deferred persisting `IntelligenceFinding`s, preferring the
in-memory `FindingQueue` unless persistence is clearly justified.
Phase 2.1 gives that queue its first real writer
(`analyze_music_transcript`) but still does not add a
`music_findings`/`intelligence_findings` SQLite table. Nothing yet
requires a finding to survive an application restart the way a
`Suggestion` or `PresentationItem` does (both already have their own
persisted tables from earlier phases) - a live service's Music
Intelligence review happens within that service's session. If a future
phase needs findings to survive a restart, that's a deliberate,
separately-justified addition, not something this phase invents
preemptively.

## Dataset provenance reuses the Content Registry - no second system

A music dataset is registered exactly like a Bible translation
(`ContentType::Music`, the same `"<type>:<id>"` id convention, e.g.
`"music:dev-hymnbook"`), through the same `ContentRegistry` trait and
the same enabled/disabled lifecycle. There is no second
licensing/provenance system for Music - see
[`docs/content-registry.md`](content-registry.md) and
[`docs/music-datasets.md`](music-datasets.md).

## Multi-engine proof (extending Phase 2.0's acceptance architecture)

`core/intelligence/src/acceptance_tests.rs`'s Phase 2.1 section proves,
with two **real** engines (not synthetic test doubles):

- Bible and Music both analyze independently, and their findings coexist
  in one shared `IntelligenceContext.recent_findings` without either
  engine calling the other.
- A registry with only Music registered still works; a registry with
  only Bible registered still works - neither engine depends on the
  other's registration.
- `MusicIntelligenceEngine` is deterministic: identical input and
  context always produce identical findings (modulo id/timestamp).

Phase 2.4 extends this same proof one layer up: a Music finding sharing a
transcript segment with a Bible finding may produce a `ScriptureMusic`
correlation, but only at that same-segment strength - mere proximity
elsewhere in the service (e.g. a hymn's title alone matching no shared
segment with a Scripture reference) never does. `CrossDomainCorrelationEngine`
still calls neither `MusicIntelligenceEngine` nor any other engine
directly. See [`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md).

## Performance

Measured directly (`std::time::Instant`, release build, this machine,
one run - not a formal benchmark harness, using throwaway test files
deleted before commit, matching the Phase 1.5/2.0 measurement
methodology):

| Operation | Observed |
| --- | --- |
| `normalize_for_matching` | ~384ns |
| `search_songs` (exact title, 500 synthetic songs) | ~1.17µs |
| `search_songs` (lyric substring match across many of 5,000 synthetic lines) | ~1.92ms |
| `search_songs` (no match, full scan of 5,000 lines) | ~29µs |
| `MusicIntelligenceEngine::analyze` (explicit title match, in-memory fixture) | ~1.93µs |
| `MusicIntelligenceEngine::analyze` (no match, full dispatch chain) | ~1.60µs |
| `analyze_and_queue` end to end against a real `SqliteMusicProvider` (explicit title) | ~37.3µs |

Real numbers from one measurement pass, not "instant"/"real-time"
claims. Every operation here is sub-millisecond even at synthetic
scales well beyond a real hymnal/worship set, except a substring lyric
scan that matches thousands of lines at once (a synthetic worst case no
real dataset produces - the same phrase repeated across hundreds of
songs) - still under 2ms.

## Offline guarantee

`core/music` depends only on `cip-core-confidence`, `serde`, and
`thiserror` (plus their transitive machinery) - no SQLite, no Tauri, no
audio library, no network client. `integrations/music` adds `rusqlite`
(the local database driver) and nothing else. Verified structurally via
`cargo tree -p cip-core-music` and `cargo tree -p cip-integrations-music`
- neither shows `reqwest`/`hyper`/`ureq`/`tungstenite` or any other
network-capable crate, the same proof every earlier phase established
for its own domain.

## Copyright & provenance discipline

Same hard constraint [`docs/music-datasets.md`](music-datasets.md#licensing-policy---read-this-first)
states in full: CIP never scrapes or bulk-downloads song lyrics, never
ships a large copyrighted song database, and never guesses licensing
metadata - unknown stays `null`/`UNKNOWN`. The development fixture
(`database/seeds/dev_seed.sql`) uses entirely fictional, synthetic song
titles and lyrics ("Test Fixture Hymn One", "This is a test hymn about
steadfast care") - never real hymn or worship song text - specifically
so no copyright judgment call is required to ship it.

## Testing

- `core/music`'s own unit tests (34) cover the domain model,
  normalization, matcher/distinctiveness/ambiguity, and continuity in
  isolation, using a crate-local `FakeMusicProvider` fixture.
- `integrations/music`'s tests (16) cover the real `SqliteMusicProvider`
  and the dataset importer (clean import, idempotent re-import including
  alias dedup, malformed-row skipping, checksum determinism) against a
  real migrated SQLite database.
- `core/intelligence`'s tests include `music_adapter`'s own 10 tests
  (title/alias/number/lyric/multi-line/no-match/disabled-dataset/
  no-registered-content/determinism/acoustic-unavailable) plus the
  Phase 2.1 multi-engine acceptance tests described above.
- `apps/desktop/src-tauri::music`'s tests cover the real orchestration
  path end to end: dev-seed dataset registration, dataset import, and
  `analyze_and_queue` against a real `SqliteMusicProvider` and real
  `SqliteContentRegistry` - including two hard degradation proofs
  (an exact title match in a disabled dataset is never searched; no
  registered/empty dataset degrades to "no match," never an error) and
  an operator-workflow proof (accepting a queued finding changes only
  its own status).

```sh
cargo test -p cip-core-music
cargo test -p cip-integrations-music
cargo test -p cip-core-intelligence
cargo test -p cip-desktop music::
```

See [`docs/intelligence-architecture.md`](intelligence-architecture.md)
for the shared architecture this builds on, and
[`docs/music-datasets.md`](music-datasets.md) for the dataset import
format and licensing policy.
