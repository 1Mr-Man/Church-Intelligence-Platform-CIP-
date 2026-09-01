//! Pure post-processing for turning a BERT-family model's per-token output
//! into one fixed-length sentence embedding - mean pooling over the
//! non-padding tokens, then L2 normalization, exactly the recipe
//! `sentence-transformers/all-MiniLM-L6-v2`'s own model card specifies
//! (not every BERT embedding model uses mean pooling - some use the `[CLS]`
//! token instead - so this is deliberately not presented as universal).
//!
//! Deliberately plain `Vec<f32>` in, `Vec<f32>` out - no `candle` types
//! anywhere in this module, so it compiles and is unit-testable
//! unconditionally, never gated behind the `semantic-search` feature the
//! real model-loading/inference code needs. `CandleEmbeddingEngine`
//! converts its tensor output to/from this shape at the boundary.

/// Mean-pools `token_embeddings` (one `dimensions`-length vector per input
/// token, in sequence order) into a single sentence vector, counting only
/// positions where `attention_mask` is non-zero (real tokens, not padding).
/// Returns an all-zero vector of the same width (never panics, never
/// divides by zero) if every position is masked out or the inputs are
/// empty/mismatched - an honest "nothing to pool" result rather than a
/// fabricated one.
pub fn mean_pool(token_embeddings: &[Vec<f32>], attention_mask: &[u32]) -> Vec<f32> {
    let dimensions = token_embeddings.first().map_or(0, Vec::len);
    if dimensions == 0 || token_embeddings.len() != attention_mask.len() {
        return vec![0.0; dimensions];
    }

    let mut sum = vec![0.0f32; dimensions];
    let mut count = 0u32;
    for (embedding, &mask) in token_embeddings.iter().zip(attention_mask) {
        if mask == 0 {
            continue;
        }
        for (s, v) in sum.iter_mut().zip(embedding) {
            *s += v;
        }
        count += 1;
    }

    if count == 0 {
        return vec![0.0; dimensions];
    }
    for s in &mut sum {
        *s /= count as f32;
    }
    sum
}

/// L2-normalizes `v` in place - divides every element by the vector's
/// magnitude, so its length becomes `1.0`. A zero-magnitude vector (all
/// zeros, e.g. `mean_pool`'s own "nothing to pool" result) is left
/// unchanged rather than dividing by zero.
pub fn l2_normalize(v: &mut [f32]) {
    let magnitude: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude == 0.0 {
        return;
    }
    for x in v.iter_mut() {
        *x /= magnitude;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pool_averages_only_unmasked_positions() {
        let tokens = vec![
            vec![2.0, 4.0],
            vec![4.0, 8.0],
            // padding - must be excluded from the average.
            vec![100.0, 100.0],
        ];
        let mask = [1, 1, 0];
        let pooled = mean_pool(&tokens, &mask);
        assert_eq!(pooled, vec![3.0, 6.0]);
    }

    #[test]
    fn mean_pool_handles_a_single_real_token() {
        let tokens = vec![vec![1.0, 2.0, 3.0]];
        let mask = [1];
        assert_eq!(mean_pool(&tokens, &mask), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn mean_pool_returns_zeros_when_everything_is_masked_out() {
        let tokens = vec![vec![9.0, 9.0], vec![9.0, 9.0]];
        let mask = [0, 0];
        assert_eq!(mean_pool(&tokens, &mask), vec![0.0, 0.0]);
    }

    #[test]
    fn mean_pool_returns_empty_for_empty_input_rather_than_panicking() {
        assert_eq!(mean_pool(&[], &[]), Vec::<f32>::new());
    }

    #[test]
    fn mean_pool_returns_zeros_for_mismatched_lengths_rather_than_panicking() {
        let tokens = vec![vec![1.0, 2.0]];
        let mask = [1, 1]; // one extra mask entry, no corresponding token
        assert_eq!(mean_pool(&tokens, &mask), vec![0.0, 0.0]);
    }

    #[test]
    fn l2_normalize_produces_a_unit_vector() {
        let mut v = vec![3.0, 4.0]; // magnitude 5
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
        let magnitude: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_leaves_a_zero_vector_unchanged() {
        let mut v = vec![0.0, 0.0, 0.0];
        l2_normalize(&mut v);
        assert_eq!(v, vec![0.0, 0.0, 0.0]);
    }
}
