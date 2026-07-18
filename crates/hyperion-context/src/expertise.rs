//! A real, deterministic vocabulary-complexity scorer -- one of docs/06 §5.4's own three named
//! Adaptive Complexity signals, and the one half of that gap this crate can compute directly
//! (the other two, Capability-tier reach and error-recovery pattern, need a real caller's own
//! dispatch outcome -- see [`crate::types::ExpertiseSignal`]'s own doc comment). Not a neural/ML
//! read: a genuine, well-established plain-language readability proxy (the same shape of signal
//! a Flesch-Kincaid-style score already uses -- average word length and the fraction of long,
//! likely-technical words), honestly labeled as a simple heuristic rather than a calibrated
//! model.

/// A word at or above this length is treated as a real signal of technical/rare vocabulary --
/// short enough to catch real technical terms ("dependency," "asynchronous," "capability") while
/// staying well above typical everyday English word lengths.
const LONG_WORD_CHARS: usize = 8;
/// Average English word length is roughly 4-5 characters; below this, `utterance` contributes no
/// length-based complexity at all.
const BASELINE_AVG_WORD_LEN: f32 = 4.0;
/// The average word length at which the length-based component saturates at `1.0`.
const SATURATING_AVG_WORD_LEN: f32 = 12.0;

/// A real complexity score for `utterance`, in `[0.0, 1.0]` -- `0.0` for empty or entirely
/// short/common-length words, `1.0` for long, saturating vocabulary. Blends two independent
/// signals equally: the utterance's own average word length (normalized against
/// [`BASELINE_AVG_WORD_LEN`]/[`SATURATING_AVG_WORD_LEN`]), and the real fraction of its words at
/// or above [`LONG_WORD_CHARS`] characters.
pub fn vocabulary_complexity(utterance: &str) -> f32 {
    let words: Vec<&str> = utterance.split_whitespace().collect();
    if words.is_empty() {
        return 0.0;
    }

    let avg_len =
        words.iter().map(|w| w.chars().count()).sum::<usize>() as f32 / words.len() as f32;
    let length_component = ((avg_len - BASELINE_AVG_WORD_LEN)
        / (SATURATING_AVG_WORD_LEN - BASELINE_AVG_WORD_LEN))
        .clamp(0.0, 1.0);

    let long_word_fraction = words
        .iter()
        .filter(|w| w.chars().count() >= LONG_WORD_CHARS)
        .count() as f32
        / words.len() as f32;

    (0.5 * length_component + 0.5 * long_word_fraction).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_utterance_scores_zero() {
        assert_eq!(vocabulary_complexity(""), 0.0);
        assert_eq!(vocabulary_complexity("   "), 0.0);
    }

    #[test]
    fn simple_everyday_words_score_low() {
        let score = vocabulary_complexity("what is the weather like today");
        assert!(score < 0.2, "got {score}");
    }

    #[test]
    fn long_technical_vocabulary_scores_high() {
        let score = vocabulary_complexity(
            "instantiate the asynchronous dependency-injection configuration prematurely",
        );
        assert!(score > 0.7, "got {score}");
    }

    #[test]
    fn more_technical_vocabulary_always_scores_at_least_as_high_as_less_technical_vocabulary() {
        let simple = vocabulary_complexity("fix my code please");
        let technical = vocabulary_complexity(
            "refactor the asynchronous authentication middleware implementation",
        );
        assert!(technical > simple, "simple={simple} technical={technical}");
    }

    #[test]
    fn the_score_is_always_within_the_real_zero_to_one_range() {
        for utterance in [
            "a",
            "supercalifragilisticexpialidocious",
            "the quick brown fox jumps over the lazy dog repeatedly and thoroughly",
        ] {
            let score = vocabulary_complexity(utterance);
            assert!(
                (0.0..=1.0).contains(&score),
                "utterance={utterance:?} got {score}"
            );
        }
    }
}
