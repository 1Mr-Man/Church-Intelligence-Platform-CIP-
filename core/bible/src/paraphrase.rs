//! Lexical/keyword-overlap paraphrase detection - a deliberately narrow,
//! honest fallback for when a transcript segment cites no book/chapter/verse
//! at all (`crate::detection` found no candidate shape) but its wording
//! closely echoes a specific verse's text, e.g. "all things work together
//! for good for those who love God" for Romans 8:28 - the exact example the
//! CIP Master Architecture document's Paraphrase Detection section uses.
//!
//! This is **not** semantic/neural matching. There is no embedding model,
//! no vector index, and no understanding of meaning anywhere in this
//! module - only a bounded ratio of shared distinctive words, after a very
//! light heuristic stemmer that bridges simple conjugation/pluralization
//! differences (e.g. "work"/"works", "call"/"called"). A "conceptual"
//! paraphrase that shares little or no vocabulary with the verse it's based
//! on (e.g. "Jesus said we should love our enemies" for Matthew 5:44) will
//! **not** be found by this module - that remains a documented, not-yet-
//! started gap (see `docs/phase-4-master-plan-gap-audit.md`).
//!
//! [`score_overlap`] is the pure, deterministic scoring function;
//! [`significant_words`] is the shared tokenizer both this module and
//! `BibleProvider::find_similar_verses`'s default implementation use to
//! build a candidate word list for a query. Neither function is specific to
//! any Bible translation or dataset - both operate on plain text.

use std::collections::HashSet;

/// Common words with no distinctive matching value - excluded from both
/// the candidate-retrieval query terms and the overlap score itself.
/// Deliberately small and specific to spoken/written English scripture
/// paraphrase, not a general-purpose stopword list.
const STOPWORDS: &[&str] = &[
    "a", "all", "an", "and", "are", "as", "at", "be", "but", "by", "did", "do", "does", "for",
    "from", "had", "has", "have", "he", "her", "him", "his", "how", "i", "if", "in", "is", "it",
    "its", "may", "me", "might", "must", "my", "no", "not", "of", "on", "or", "our", "shall",
    "she", "should", "so", "than", "that", "the", "their", "them", "then", "there", "these",
    "they", "this", "those", "to", "us", "was", "we", "were", "what", "when", "where", "which",
    "who", "whom", "will", "with", "would", "you", "your",
];

/// Strip a handful of common English suffixes so simple conjugation/
/// pluralization differences ("work"/"works", "call"/"called") don't block
/// an otherwise-matching word. Deliberately not a real stemmer (no Porter/
/// Snowball algorithm) - just enough to serve this module's narrow purpose,
/// kept simple so it stays fully deterministic and easy to reason about.
fn stem(word: &str) -> String {
    if word.len() > 5 && word.ends_with("ing") {
        return word[..word.len() - 3].to_string();
    }
    if word.len() > 4 && word.ends_with("ed") {
        return word[..word.len() - 2].to_string();
    }
    if word.len() > 4 && word.ends_with("es") {
        return word[..word.len() - 2].to_string();
    }
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

/// Lowercase, split on non-alphanumeric characters, drop stopwords and
/// short tokens (`len < 3`), then lightly stem what's left - the shared
/// vocabulary both [`score_overlap`] and `BibleProvider::find_similar_verses`'s
/// default candidate retrieval build from. Order-preserving (first
/// occurrence order), may contain duplicates - callers that need a set
/// should collect into one themselves.
pub fn significant_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .map(|w| stem(&w))
        .collect()
}

/// How many *distinct* significant words `text` has. Callers pair this with
/// [`score_overlap`] to require a minimum amount of genuine vocabulary
/// before trusting a high ratio, so a two- or three-word utterance can't
/// reach a perfect score by accident.
pub fn significant_word_count(text: &str) -> usize {
    significant_words(text)
        .into_iter()
        .collect::<HashSet<_>>()
        .len()
}

/// What fraction of `query_text`'s distinct significant words also appear
/// (post-stemming) in `verse_text` - `0.0..=1.0`. Deliberately asymmetric:
/// this is *recall of the query's vocabulary in the verse*, not similarity
/// in general, because a paraphrase is judged by how much of what the
/// operator said came from the verse, not how much of the (usually longer)
/// verse the operator happened to say.
pub fn score_overlap(query_text: &str, verse_text: &str) -> f32 {
    let query_words: HashSet<String> = significant_words(query_text).into_iter().collect();
    if query_words.is_empty() {
        return 0.0;
    }
    let verse_words: HashSet<String> = significant_words(verse_text).into_iter().collect();
    let matched = query_words.intersection(&verse_words).count();
    matched as f32 / query_words.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact Berean Standard Bible text for Romans 8:28, the CIP Master
    // Architecture document's own worked example for paraphrase detection.
    const ROM_8_28_BSB: &str = "And we know that God works all things together for the good of those who love Him, who are called according to His purpose.";

    #[test]
    fn stopwords_and_short_words_are_filtered() {
        let words = significant_words("And we know that all things work together for good.");
        assert_eq!(
            words,
            vec!["know", "thing", "work", "together", "good"],
            "stopwords (and/we/that/all/for) and the trailing period must not appear"
        );
    }

    #[test]
    fn stemming_bridges_simple_conjugation_and_pluralization() {
        assert_eq!(stem("works"), "work");
        assert_eq!(stem("called"), "call");
        assert_eq!(stem("according"), "accord");
        assert_eq!(stem("things"), "thing");
        // Must not over-strip short/irregular words.
        assert_eq!(stem("good"), "good");
        assert_eq!(stem("us"), "us");
        assert_eq!(stem("purpose"), "purpose");
    }

    #[test]
    fn the_master_plans_own_paraphrase_example_scores_a_near_perfect_match() {
        let query = "All things work together for good for those who love God";
        let score = score_overlap(query, ROM_8_28_BSB);
        assert!(
            score >= 0.95,
            "expected the master plan's own worked example to score near 1.0, got {score}"
        );
    }

    #[test]
    fn the_shorter_paraphrase_without_the_full_second_clause_still_matches_well() {
        // The exact sentence this project's own existing regression test
        // uses as a false-positive check - it must score high, since that
        // test is being deliberately updated to expect a Paraphrase
        // detection for it.
        let query = "And we know that all things work together for good.";
        let score = score_overlap(query, ROM_8_28_BSB);
        assert!(
            score >= 0.9,
            "expected a strong match for the shorter paraphrase, got {score}"
        );
    }

    #[test]
    fn unrelated_sentences_mentioning_one_shared_word_score_low() {
        let cases = [
            "Paul is showing us the work of the Spirit.",
            "Chapter eight of our study is important.",
            "Romans is an important book.",
            "John was one of the disciples.",
        ];
        for text in cases {
            let score = score_overlap(text, ROM_8_28_BSB);
            assert!(
                score < 0.5,
                "expected {text:?} to score well below the paraphrase threshold, got {score}"
            );
        }
    }

    #[test]
    fn empty_query_scores_zero_rather_than_dividing_by_zero() {
        assert_eq!(score_overlap("", ROM_8_28_BSB), 0.0);
        assert_eq!(score_overlap("and the of to", ROM_8_28_BSB), 0.0);
    }

    #[test]
    fn significant_word_count_matches_the_deduplicated_tokenizer_output() {
        assert_eq!(significant_word_count("Romans 8"), 1);
        assert_eq!(
            significant_word_count("And we know that all things work together for good."),
            5
        );
        // Repetition of the same significant word must not inflate the
        // count used to gate against short, trivially-matching utterances.
        assert_eq!(significant_word_count("good good good"), 1);
    }
}
