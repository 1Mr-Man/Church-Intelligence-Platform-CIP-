//! Comprehensive matcher-level tests (Phase 2.1 spec section 44, "CORE
//! MUSIC") - exercised against [`crate::fixtures::FakeMusicProvider`],
//! proving `search_songs`'s dispatch/ranking/ambiguity behavior end to
//! end rather than unit-by-unit.

#![cfg(test)]

use crate::candidate::MatchType;
use crate::fixtures::FakeMusicProvider;
use crate::matcher::{is_ambiguous, search_songs, MatchThresholds, MusicQuery};
use crate::song::{LyricLine, SectionKind, Song, SongSection, SongType};

const DATASET: &str = "music:dev-hymnbook";
const DATASET_TWO: &str = "music:dev-worship-set";

fn hymn(id: &str, title: &str, number: Option<&str>, aliases: Vec<&str>) -> Song {
    Song::new(
        id,
        DATASET,
        title,
        aliases.into_iter().map(String::from).collect(),
        SongType::Hymn,
        "en",
        number.map(String::from),
        None,
        None,
    )
}

fn line(song_id: &str, section_id: Option<&str>, sequence: u32, text: &str) -> LyricLine {
    LyricLine::new(song_id, section_id.map(String::from), sequence, text)
}

/// Populates a fixture with:
/// - "Test Hymn One" (#120 in DATASET), alias "First Test Hymn", with a
///   distinctive two-line lyric pair.
/// - "Test Hymn Two" (#121 in DATASET), unrelated lyrics.
/// - A song in DATASET_TWO that also uses number "120" - a *different*
///   song, proving dataset isolation (spec section 57).
/// - Two songs sharing a short, generic phrase ("we praise you lord"),
///   to exercise ambiguity and distinctiveness weakness.
fn seeded_provider() -> FakeMusicProvider {
    let mut provider = FakeMusicProvider::new();

    let hymn_one = hymn("h1", "Test Hymn One", Some("120"), vec!["First Test Hymn"]);
    provider.add_song(hymn_one);
    provider.add_sections(
        DATASET,
        "h1",
        vec![SongSection {
            id: "h1-v1".to_string(),
            song_id: "h1".to_string(),
            kind: SectionKind::Verse,
            sequence: 0,
        }],
    );
    provider.add_lyrics(
        DATASET,
        "h1",
        vec![
            line(
                "h1",
                Some("h1-v1"),
                0,
                "Great is thy faithfulness my Father",
            ),
            line(
                "h1",
                Some("h1-v1"),
                1,
                "Morning by morning new mercies I see",
            ),
        ],
    );

    let hymn_two = hymn("h2", "Test Hymn Two", Some("121"), vec![]);
    provider.add_song(hymn_two);
    provider.add_lyrics(
        DATASET,
        "h2",
        vec![line("h2", None, 0, "A completely unrelated lyric line")],
    );

    let cross_dataset_song = Song::new(
        "w1",
        DATASET_TWO,
        "Different Song Same Number",
        vec![],
        SongType::WorshipSong,
        "en",
        Some("120".to_string()),
        None,
        None,
    );
    provider.add_song(cross_dataset_song);

    let generic_a = Song::new(
        "g1",
        DATASET,
        "Generic Praise Song A",
        vec![],
        SongType::Chorus,
        "en",
        None,
        None,
        None,
    );
    provider.add_song(generic_a);
    provider.add_lyrics(
        DATASET,
        "g1",
        vec![line("g1", None, 0, "We praise you Lord")],
    );

    let generic_b = Song::new(
        "g2",
        DATASET,
        "Generic Praise Song B",
        vec![],
        SongType::Chorus,
        "en",
        None,
        None,
        None,
    );
    provider.add_song(generic_b);
    // Deliberately the exact same generic phrase as g1's lyric - two
    // songs tied on both match type and distinctiveness, constructing
    // genuine ambiguity rather than one candidate simply outranking the
    // other.
    provider.add_lyrics(
        DATASET,
        "g2",
        vec![line("g2", None, 0, "We praise you Lord")],
    );

    provider
}

fn datasets() -> Vec<String> {
    vec![DATASET.to_string(), DATASET_TWO.to_string()]
}

#[test]
fn exact_title_match_is_the_strongest_candidate() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Title("Test Hymn One".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].song_id, "h1");
    assert_eq!(candidates[0].match_type, MatchType::ExplicitTitle);
    assert!(candidates[0].confidence.score > 0.9);
}

#[test]
fn alias_match_resolves_to_the_song() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Title("First Test Hymn".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(candidates[0].song_id, "h1");
    assert_eq!(candidates[0].match_type, MatchType::Alias);
}

#[test]
fn song_number_is_scoped_to_its_dataset() {
    let provider = seeded_provider();
    let in_first_dataset = search_songs(
        &provider,
        &[DATASET.to_string()],
        &MusicQuery::Number("120".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(in_first_dataset[0].song_id, "h1");

    let in_second_dataset = search_songs(
        &provider,
        &[DATASET_TWO.to_string()],
        &MusicQuery::Number("120".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(
        in_second_dataset[0].song_id, "w1",
        "the same number in a different dataset must resolve to a different song"
    );
}

#[test]
fn searching_both_datasets_at_once_never_conflates_the_shared_number() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Number("120".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    let song_ids: Vec<&str> = candidates.iter().map(|c| c.song_id.as_str()).collect();
    assert!(song_ids.contains(&"h1"));
    assert!(song_ids.contains(&"w1"));
    assert_eq!(
        candidates.len(),
        2,
        "two distinct songs, not merged into one"
    );
}

#[test]
fn exact_lyric_phrase_matches() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("Great is thy faithfulness my Father".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(candidates[0].song_id, "h1");
    assert_eq!(candidates[0].match_type, MatchType::ExactLyric);
}

#[test]
fn multi_line_consecutive_lyrics_outrank_a_single_line_match() {
    let provider = seeded_provider();
    let single = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("Great is thy faithfulness my Father".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    let multi = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::LyricSequence(vec![
            "Great is thy faithfulness my Father".to_string(),
            "Morning by morning new mercies I see".to_string(),
        ]),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert_eq!(multi[0].match_type, MatchType::MultipleLyricLines);
    assert!(multi[0].confidence.score > single[0].confidence.score);
}

#[test]
fn partial_lyric_returns_candidates_rather_than_a_forced_single_answer() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("morning by morning".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].song_id, "h1");
}

#[test]
fn a_generic_phrase_shared_by_two_songs_scores_lower_than_a_distinctive_one() {
    let provider = seeded_provider();
    let generic = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("We praise you Lord".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    let distinctive = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("Great is thy faithfulness my Father".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert!(
        generic[0].confidence.score < distinctive[0].confidence.score,
        "a phrase shared across multiple songs must not score as strongly as a distinctive one"
    );
}

#[test]
fn a_generic_phrase_producing_close_scores_is_reported_ambiguous() {
    let provider = seeded_provider();
    let thresholds = MatchThresholds::default();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("We praise you Lord".to_string()),
        &thresholds,
    )
    .unwrap();
    assert!(
        candidates.len() >= 2,
        "both generic-phrase songs must be returned, not one forced pick"
    );
    assert!(is_ambiguous(&candidates, &thresholds));
}

#[test]
fn no_match_returns_an_empty_result_not_an_error() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Title("Nonexistent Song Title".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    assert!(candidates.is_empty());
}

#[test]
fn ranking_is_deterministic_across_repeated_calls() {
    let provider = seeded_provider();
    let run = || {
        search_songs(
            &provider,
            &datasets(),
            &MusicQuery::Lyric("We praise you Lord".to_string()),
            &MatchThresholds::default(),
        )
        .unwrap()
        .into_iter()
        .map(|c| (c.song_id, c.ranking, c.confidence.score))
        .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn candidates_are_ranked_by_confidence_descending() {
    let provider = seeded_provider();
    let candidates = search_songs(
        &provider,
        &datasets(),
        &MusicQuery::Lyric("We praise you Lord".to_string()),
        &MatchThresholds::default(),
    )
    .unwrap();
    for window in candidates.windows(2) {
        assert!(window[0].confidence.score >= window[1].confidence.score);
        assert_eq!(window[0].ranking + 1, window[1].ranking);
    }
}
