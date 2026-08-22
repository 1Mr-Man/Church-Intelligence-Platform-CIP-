//! Deterministic title/lyric normalization for matching (Phase 2.1
//! spec section 8).
//!
//! ## Policy
//!
//! [`normalize_for_matching`] is a *comparison* transform, not a display
//! one - the original `text`/`title` is always kept alongside it (see
//! `Song`/`LyricLine`), so nothing here is ever shown to an operator.
//! Applied transforms, in order:
//!
//! 1. Unicode case-folding to lowercase (not English-only - Rust's
//!    `char::to_lowercase` is Unicode-aware).
//! 2. Typographic quote/apostrophe variants (`’‘“”`) collapsed to a plain
//!    `'`/`"` before being dropped in step 4, so `"don't"` and
//!    `"don't"` (curly apostrophe) normalize identically.
//! 3. Hyphen/dash variants (`‐‑‒–—−`) collapsed to a plain `-`, then
//!    treated as word-separating whitespace - `"non-stop"` normalizes the
//!    same as `"non stop"`.
//! 4. Every remaining character that is not alphanumeric or whitespace is
//!    dropped (punctuation, symbols).
//! 5. Runs of whitespace (including line breaks) collapsed to a single
//!    space; leading/trailing whitespace trimmed.
//!
//! ## What this deliberately does NOT do
//!
//! - No phonetic guessing (no soundex/metaphone) - two different words
//!   that merely sound alike must never normalize to the same thing.
//! - No stemming/lemmatization - words are not rewritten, only
//!   case/punctuation/whitespace are normalized.
//! - No theological or lyrical word substitution of any kind.
//! - No English-specific rules (no removal of articles, no spelled-number
//!   conversion) - the same transform applies to a lyric in any language.

const CURLY_QUOTES: &[char] = &['\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}'];
const DASH_VARIANTS: &[char] = &[
    '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2212}',
];

/// Normalize `text` for deterministic title/lyric matching - see module
/// docs for the exact policy.
pub fn normalize_for_matching(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = true; // suppress leading whitespace

    for ch in text.chars() {
        let is_separator = ch.is_whitespace() || DASH_VARIANTS.contains(&ch) || ch == '-';

        if CURLY_QUOTES.contains(&ch) {
            // apostrophes/quotes are dropped entirely, not spaced
        } else if is_separator {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
            last_was_space = false;
        }
        // any other punctuation/symbol is dropped
    }

    if result.ends_with(' ') {
        result.pop();
    }
    result
}

/// Split already-normalized text into words, for word-count-based
/// distinctiveness scoring (see `matcher.rs`).
pub fn word_count(normalized: &str) -> usize {
    normalized.split(' ').filter(|w| !w.is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_trims() {
        assert_eq!(
            normalize_for_matching("  Great Is Thy Faithfulness  "),
            "great is thy faithfulness"
        );
    }

    #[test]
    fn collapses_repeated_whitespace_and_line_breaks() {
        assert_eq!(
            normalize_for_matching("Great is\n\nthy   faithfulness"),
            "great is thy faithfulness"
        );
    }

    #[test]
    fn strips_punctuation_without_rewriting_words() {
        assert_eq!(
            normalize_for_matching("Great is Thy faithfulness!"),
            "great is thy faithfulness"
        );
        assert_eq!(
            normalize_for_matching("O God, my Father,"),
            "o god my father"
        );
    }

    #[test]
    fn curly_and_straight_apostrophes_normalize_identically() {
        assert_eq!(
            normalize_for_matching("don\u{2019}t"),
            normalize_for_matching("don't")
        );
    }

    #[test]
    fn hyphen_variants_act_as_a_word_separator() {
        assert_eq!(normalize_for_matching("non-stop"), "non stop");
        assert_eq!(normalize_for_matching("non\u{2013}stop"), "non stop");
    }

    #[test]
    fn does_not_alter_non_english_letters() {
        // Unicode-aware lowercasing, no transliteration/ASCII-folding.
        assert_eq!(
            normalize_for_matching("GRANDE ES TU FIDELIDAD"),
            "grande es tu fidelidad"
        );
        assert_eq!(normalize_for_matching("Ça va"), "ça va");
    }

    #[test]
    fn word_count_ignores_empty_tokens() {
        assert_eq!(word_count("great is thy faithfulness"), 4);
        assert_eq!(word_count(""), 0);
    }
}
