//! Semantic (embedding-based) Bible verse similarity - a further fallback
//! for when a transcript segment cites no book/chapter/verse at all
//! (`crate::detection` found no candidate shape) *and* shares too little
//! vocabulary with any verse for `crate::paraphrase`'s lexical/keyword-
//! overlap heuristic to find it, e.g. "Jesus said we should love our
//! enemies" for Matthew 5:44 - the exact "conceptual paraphrase" gap that
//! module's own docs name as out of its reach.
//!
//! This module holds the pure comparison primitive ([`cosine_similarity`]),
//! the small amount of vector bookkeeping ([`is_valid_embedding`]) needed to
//! compare two embeddings safely, and the [`VerseEmbeddingStore`]
//! provider/adaptor contract for retrieving precomputed verse embeddings to
//! compare against. It knows nothing about *how* an embedding is produced -
//! that is `cip_core_ai::EmbeddingEngine`'s job (implemented for a real
//! local model by `ai/embedding`, behind the `semantic-search` feature) -
//! this crate only ever consumes a `Vec<f32>` it's handed, exactly like
//! `crate::paraphrase` only ever consumes plain text.

use crate::reference::ScriptureReference;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerseEmbeddingError {
    #[error("underlying storage error: {0}")]
    Storage(String),
}

/// One verse's precomputed embedding, alongside enough identifying
/// information to resolve it back to real Bible text via
/// `BibleProvider::get_verse`. Never carries the verse text itself - this
/// is purely a vector index, not a second source of truth for what a verse
/// says.
#[derive(Debug, Clone, PartialEq)]
pub struct VerseEmbedding {
    pub reference: ScriptureReference,
    pub vector: Vec<f32>,
}

/// The provider/adaptor contract for retrieving precomputed verse
/// embeddings, for this module's meaning-based fallback. Deliberately
/// separate from `BibleProvider`: not every `BibleProvider` will have
/// embeddings computed for its content (a freshly imported dataset has none
/// yet, and generating them is a distinct, explicit operator action - see
/// `docs/phase-4-4-semantic-bible-search.md`), so this stays an
/// independent, optional capability rather than another method every
/// `BibleProvider` implementation must supply.
pub trait VerseEmbeddingStore: Send + Sync {
    /// Every stored embedding for `translation_id` produced by `model_id` -
    /// unfiltered, unranked (the caller scores and ranks; this trait's job
    /// is retrieval only, mirroring `BibleProvider::find_similar_verses`'s
    /// own division of responsibility). Rows stored under a different
    /// `model_id` are never returned - see `cip_core_ai::EmbeddingEngine`'s
    /// `model_id` doc comment for why mixing them would be meaningless.
    fn verse_embeddings(
        &self,
        translation_id: &str,
        model_id: &str,
    ) -> Result<Vec<VerseEmbedding>, VerseEmbeddingError>;
}

/// Scores `query_vector` against every embedding `store` has for
/// `translation_id`/`model_id`, returning the single best-scoring
/// reference and its similarity score, or `None` if nothing scores at or
/// above `min_similarity` (including when the store has nothing at all, or
/// only mismatched-dimension rows left over from a since-abandoned model).
/// The caller is responsible for validating the winning reference against a
/// real `BibleProvider` before trusting it - this function only ever
/// compares vectors, never touches Bible text.
pub fn best_semantic_match(
    store: &dyn VerseEmbeddingStore,
    translation_id: &str,
    model_id: &str,
    query_vector: &[f32],
    min_similarity: f32,
) -> Result<Option<(ScriptureReference, f32)>, VerseEmbeddingError> {
    let dimensions = query_vector.len();
    let candidates = store.verse_embeddings(translation_id, model_id)?;

    let mut best: Option<(ScriptureReference, f32)> = None;
    for candidate in candidates {
        if !is_valid_embedding(&candidate.vector, dimensions) {
            continue;
        }
        let score = cosine_similarity(query_vector, &candidate.vector);
        let is_better = best.as_ref().map(|(_, s)| score > *s).unwrap_or(true);
        if is_better {
            best = Some((candidate.reference, score));
        }
    }

    Ok(best.filter(|(_, score)| *score >= min_similarity))
}

/// Cosine similarity between two equal-length embedding vectors, in
/// `-1.0..=1.0` (in practice, for real sentence embeddings, almost always
/// `0.0..=1.0` - two genuinely unrelated sentences rarely score negative).
/// Returns `0.0` (never panics, never divides by zero) for empty vectors,
/// mismatched lengths, or a zero-magnitude vector on either side - all
/// honest "no meaningful comparison possible" cases rather than a
/// fabricated score.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

/// Whether `embedding` is safe to compare against vectors produced by the
/// currently configured embedding engine - a real, non-empty vector of
/// exactly `expected_dimensions` length. Guards against comparing an
/// embedding stored under a previous, differently-shaped model (or a
/// corrupt/truncated stored value) as if it meant something under the
/// current one - never a fabricated or silently-truncated comparison.
pub fn is_valid_embedding(embedding: &[f32], expected_dimensions: usize) -> bool {
    expected_dimensions > 0 && embedding.len() == expected_dimensions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_score_a_perfect_one() {
        let v = vec![0.1, 0.2, 0.3, 0.4];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_score_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn opposite_vectors_score_negative_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - -1.0).abs() < 1e-6);
    }

    #[test]
    fn scale_invariant_direction_only() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![2.0, 4.0, 6.0]; // same direction, different magnitude
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn empty_vectors_score_zero_rather_than_panicking() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn mismatched_lengths_score_zero_rather_than_panicking() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }

    #[test]
    fn zero_magnitude_vector_scores_zero_rather_than_dividing_by_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn valid_embedding_requires_exact_matching_nonzero_dimensions() {
        assert!(is_valid_embedding(&[0.0; 384], 384));
        assert!(!is_valid_embedding(&[0.0; 300], 384));
        assert!(!is_valid_embedding(&[], 384));
        assert!(!is_valid_embedding(&[0.0; 384], 0));
    }

    struct FakeVerseEmbeddingStore {
        entries: Vec<(String, VerseEmbedding)>,
    }

    impl FakeVerseEmbeddingStore {
        fn new(entries: Vec<(&str, VerseEmbedding)>) -> Self {
            Self {
                entries: entries
                    .into_iter()
                    .map(|(model_id, embedding)| (model_id.to_string(), embedding))
                    .collect(),
            }
        }

        fn reference(book: &str, chapter: u32, verse: u32) -> ScriptureReference {
            ScriptureReference::single("KJV", book, chapter, verse)
        }
    }

    impl VerseEmbeddingStore for FakeVerseEmbeddingStore {
        fn verse_embeddings(
            &self,
            translation_id: &str,
            model_id: &str,
        ) -> Result<Vec<VerseEmbedding>, VerseEmbeddingError> {
            Ok(self
                .entries
                .iter()
                .filter(|(m, e)| m == model_id && e.reference.translation_id == translation_id)
                .map(|(_, e)| e.clone())
                .collect())
        }
    }

    #[test]
    fn best_semantic_match_returns_the_highest_scoring_reference_above_the_threshold() {
        let store = FakeVerseEmbeddingStore::new(vec![
            (
                "all-MiniLM-L6-v2",
                VerseEmbedding {
                    reference: FakeVerseEmbeddingStore::reference("ROM", 8, 28),
                    vector: vec![1.0, 0.0],
                },
            ),
            (
                "all-MiniLM-L6-v2",
                VerseEmbedding {
                    reference: FakeVerseEmbeddingStore::reference("JHN", 3, 16),
                    vector: vec![0.0, 1.0],
                },
            ),
        ]);

        let result =
            best_semantic_match(&store, "KJV", "all-MiniLM-L6-v2", &[0.9, 0.1], 0.5).unwrap();
        let (reference, score) = result.expect("expected a match above the threshold");
        assert_eq!(reference.book, "ROM");
        assert!(score > 0.5);
    }

    #[test]
    fn best_semantic_match_returns_none_when_nothing_clears_the_threshold() {
        let store = FakeVerseEmbeddingStore::new(vec![(
            "all-MiniLM-L6-v2",
            VerseEmbedding {
                reference: FakeVerseEmbeddingStore::reference("ROM", 8, 28),
                vector: vec![1.0, 0.0],
            },
        )]);

        let result =
            best_semantic_match(&store, "KJV", "all-MiniLM-L6-v2", &[0.0, 1.0], 0.5).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn best_semantic_match_ignores_rows_from_a_different_model() {
        let store = FakeVerseEmbeddingStore::new(vec![(
            "some-other-model",
            VerseEmbedding {
                reference: FakeVerseEmbeddingStore::reference("ROM", 8, 28),
                vector: vec![1.0, 0.0],
            },
        )]);

        let result =
            best_semantic_match(&store, "KJV", "all-MiniLM-L6-v2", &[1.0, 0.0], 0.5).unwrap();
        assert!(
            result.is_none(),
            "a different model's embeddings must never be compared against"
        );
    }

    #[test]
    fn best_semantic_match_ignores_mismatched_dimension_rows() {
        let store = FakeVerseEmbeddingStore::new(vec![(
            "all-MiniLM-L6-v2",
            VerseEmbedding {
                reference: FakeVerseEmbeddingStore::reference("ROM", 8, 28),
                vector: vec![1.0, 0.0, 0.0],
            },
        )]);

        let result =
            best_semantic_match(&store, "KJV", "all-MiniLM-L6-v2", &[1.0, 0.0], 0.5).unwrap();
        assert!(
            result.is_none(),
            "a stale, mismatched-dimension embedding must never be compared against"
        );
    }

    #[test]
    fn best_semantic_match_returns_none_for_an_empty_store() {
        let store = FakeVerseEmbeddingStore::new(vec![]);
        let result =
            best_semantic_match(&store, "KJV", "all-MiniLM-L6-v2", &[1.0, 0.0], 0.0).unwrap();
        assert!(result.is_none());
    }
}
