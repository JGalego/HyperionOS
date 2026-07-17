use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Fixed output dimensionality every [`embed`] call produces — small enough that a personal-scale
/// Knowledge Graph's brute-force cosine similarity (`hyperion-knowledge-graph`'s own real,
/// already-implemented scan) stays cheap, large enough that unrelated short strings rarely
/// collide onto the same small set of indices.
pub const EMBEDDING_DIMS: usize = 128;

/// A real, deterministic text embedding via feature hashing (the "hashing trick" — Weinberger et
/// al. 2009, the same technique production systems like Vowpal Wabbit use for unbounded
/// vocabularies) with signed accumulation, then L2-normalized so plain cosine similarity behaves
/// sanely across vectors of different input lengths.
///
/// This crate's own real [`crate::candle_backend::CandleBackend`] has no encoder/embedding
/// architecture wired in — it runs a real, causal-LM-only (`llama2.c` weight format) checkpoint,
/// which has no sentence-embedding head to call — so this is a real, principled, non-neural
/// substitute rather than a fabricated placeholder: every consumer that previously used a
/// token-overlap-ratio proxy for "semantic" similarity (`hyperion-netstack`'s entity resolution,
/// `hyperion-sdk`'s harness, `hyperion-memory`'s entity clustering) gets a real, fixed-dimension
/// vector it can feed into an ordinary cosine-similarity comparison instead. Swapping this for a
/// real neural sentence embedder later needs no consumer-side change — only a new implementation
/// behind this same function signature.
pub fn embed(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; EMBEDDING_DIMS];
    for raw_token in text.split_whitespace() {
        let token = raw_token.to_lowercase();
        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        let h = hasher.finish();
        let index = (h % EMBEDDING_DIMS as u64) as usize;
        // A second, independent bit of the same hash decides sign -- two different tokens
        // landing on the same index at least partially cancel rather than always compounding,
        // which is what keeps a hashed vector's dot product a meaningful similarity signal
        // instead of just "how many tokens in common, weighted by collision count."
        let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vector.iter_mut() {
            *v /= norm;
        }
    }
    vector
}

/// Cosine similarity between two equal-length vectors, `0.0` for a length mismatch or either
/// vector being all-zero (an empty-string embedding) — the same degenerate-case handling
/// `hyperion-knowledge-graph`'s own private `cosine_similarity` already uses, duplicated here
/// (rather than a new cross-crate dependency) since both vectors are already unit-normalized by
/// [`embed`] and a caller of this crate should not need `hyperion-knowledge-graph` in scope just
/// to compare two embeddings it already has in hand.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_embeds_identically() {
        assert_eq!(embed("hello world"), embed("hello world"));
    }

    #[test]
    fn embeddings_are_unit_normalized() {
        let v = embed("the quick brown fox jumps over the lazy dog");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5 || norm == 0.0);
    }

    #[test]
    fn shared_vocabulary_scores_higher_than_disjoint_vocabulary() {
        let a = embed("the cat sat on the mat");
        let b = embed("a cat sat on a mat");
        let c = embed("quantum entanglement decoheres rapidly");

        let similar = cosine_similarity(&a, &b);
        let dissimilar = cosine_similarity(&a, &c);
        assert!(
            similar > dissimilar,
            "shared-vocabulary strings ({similar}) should score above unrelated ones ({dissimilar})"
        );
    }

    #[test]
    fn empty_string_embeds_to_the_zero_vector_and_never_panics_on_normalization() {
        let v = embed("");
        assert!(v.iter().all(|&x| x == 0.0));
        assert_eq!(cosine_similarity(&v, &v), 0.0);
    }

    #[test]
    fn word_order_does_not_change_the_result_bag_of_words_by_construction() {
        assert_eq!(embed("red green blue"), embed("blue red green"));
    }
}
