//! Transcript text normalization - the first stage of the Bible Intelligence
//! Core pipeline (`TRANSCRIPT TEXT -> TEXT NORMALIZATION -> ...`).
//!
//! Phase 1.1 normalization is limited to converting spoken number words
//! into digits, since [`crate::detection`] otherwise only needs to
//! recognize digit patterns. Everything else (book aliasing, punctuation
//! tolerance in "chapter"/"verse"/colon patterns) is handled directly by
//! the detector's patterns, so it doesn't need a separate normalization
//! pass here.
//!
//! Number-word conversion is scoped to a single whitespace-separated token
//! at a time, with hyphenated compounds (`"twenty-eight"`) treated as one
//! token internally split on `-`. This deliberately does *not* merge
//! separate whitespace-separated number words into one larger number
//! (`"eight twenty eight"` stays three separate tokens: `8`, `20`, `8`) -
//! collapsing that would make `"Romans eight twenty-eight"` (chapter 8,
//! verse 28) ambiguous with a hypothetical run-on reading. Every number
//! word CIP needs to support in Phase 1.1 is always spoken as a single
//! token or a hyphenated compound, so this keeps the algorithm simple and
//! unambiguous rather than attempting general English number parsing.

const ONES_AND_TEENS: &[(&str, u32)] = &[
    ("zero", 0),
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
    ("thirteen", 13),
    ("fourteen", 14),
    ("fifteen", 15),
    ("sixteen", 16),
    ("seventeen", 17),
    ("eighteen", 18),
    ("nineteen", 19),
];

const TENS: &[(&str, u32)] = &[
    ("twenty", 20),
    ("thirty", 30),
    ("forty", 40),
    ("fifty", 50),
    ("sixty", 60),
    ("seventy", 70),
    ("eighty", 80),
    ("ninety", 90),
];

fn word_value(word: &str) -> Option<u32> {
    ONES_AND_TEENS
        .iter()
        .chain(TENS.iter())
        .find(|(w, _)| *w == word)
        .map(|(_, v)| *v)
}

fn is_number_word(word: &str) -> bool {
    word == "hundred" || word_value(word).is_some()
}

/// Ordinal forms CIP needs to recognize, scoped to 1st-3rd only: every
/// canonical Bible book numbering prefix (1/2/3 Samuel, Kings, Chronicles,
/// Corinthians, Thessalonians, Timothy, Peter, John - see `book_alias.rs`)
/// tops out at "3rd", since no book is ever numbered past three. A general
/// ordinal parser (fourth, fifth, ... twentieth) would add real scope for
/// no real gap it closes - chapter/verse numbers are always spoken as
/// cardinals, never ordinals ("Romans eight twenty-eight", never "Romans
/// eighth twenty-eighth"). Word forms ("first") were already reachable via
/// per-book literal aliases in `book_alias.rs`; this is a general rule
/// instead, so it also covers digit-suffix forms ("1st") that alias table
/// never listed, and any future numbered-book alias no longer needs its
/// own hand-written "first "/"second "/"third " entry.
const ORDINALS: &[(&str, u32)] = &[
    ("first", 1),
    ("second", 2),
    ("third", 3),
    ("1st", 1),
    ("2nd", 2),
    ("3rd", 3),
];

fn ordinal_value(word: &str) -> Option<u32> {
    ORDINALS.iter().find(|(w, _)| *w == word).map(|(_, v)| *v)
}

/// Combine a hyphen-split (or single-word) run of number words into one
/// value, e.g. `["twenty", "eight"]` -> `28`, `["one", "hundred", "fifty"]`
/// -> `150`. Returns `None` if any part isn't a recognized number word.
fn words_to_number(parts: &[String]) -> Option<u32> {
    if parts.is_empty() {
        return None;
    }
    let mut current: u32 = 0;
    for part in parts {
        if part == "hundred" {
            current = if current == 0 { 100 } else { current * 100 };
        } else {
            current += word_value(part)?;
        }
    }
    Some(current)
}

fn normalize_word_token(token: &str) -> String {
    let trimmed = token.trim_matches(|c: char| matches!(c, ',' | ';' | '!' | '?'));

    if trimmed.contains('-') {
        let parts: Vec<String> = trimmed.split('-').map(|p| p.to_lowercase()).collect();
        if parts.iter().all(|p| is_number_word(p)) {
            if let Some(n) = words_to_number(&parts) {
                return n.to_string();
            }
        }
        return token.to_string();
    }

    let lower = trimmed.to_lowercase();
    if is_number_word(&lower) {
        if let Some(n) = words_to_number(std::slice::from_ref(&lower)) {
            return n.to_string();
        }
    }
    if let Some(n) = ordinal_value(&lower) {
        return n.to_string();
    }

    token.to_string()
}

/// Normalize transcript text for the detector: spoken number words become
/// digits, everything else passes through unchanged.
///
/// ```
/// # use cip_core_bible::normalize::normalize_text;
/// assert_eq!(normalize_text("Romans chapter eight verse twenty-eight"), "Romans chapter 8 verse 28");
/// assert_eq!(normalize_text("Romans 8:28 says..."), "Romans 8:28 says...");
/// ```
pub fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(normalize_word_token)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_text_with_no_number_words() {
        assert_eq!(normalize_text("Romans 8:28 says..."), "Romans 8:28 says...");
    }

    #[test]
    fn converts_single_number_words() {
        assert_eq!(normalize_text("Romans chapter eight"), "Romans chapter 8");
    }

    #[test]
    fn converts_hyphenated_compound_number_words() {
        assert_eq!(
            normalize_text("Romans chapter eight verse twenty-eight"),
            "Romans chapter 8 verse 28"
        );
    }

    #[test]
    fn keeps_separate_whitespace_number_words_separate() {
        // "Romans eight twenty-eight" -> chapter 8, verse 28 (two distinct
        // tokens), not one run-on number.
        assert_eq!(normalize_text("Romans eight twenty-eight"), "Romans 8 28");
    }

    #[test]
    fn is_idempotent_on_already_normalized_text() {
        let once = normalize_text("Romans chapter eight verse twenty-eight");
        let twice = normalize_text(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn converts_ordinal_word_forms_to_digits() {
        assert_eq!(
            normalize_text("First Corinthians 13:4"),
            "1 Corinthians 13:4"
        );
        assert_eq!(
            normalize_text("Second Timothy chapter 3"),
            "2 Timothy chapter 3"
        );
        assert_eq!(normalize_text("Third John 4"), "3 John 4");
    }

    #[test]
    fn converts_digit_suffix_ordinal_forms_to_digits() {
        // A form the per-book literal aliases in book_alias.rs never
        // listed - Whisper's own text normalization can plausibly produce
        // "1st"/"2nd"/"3rd" for spoken "first"/"second"/"third".
        assert_eq!(normalize_text("1st Corinthians 13:4"), "1 Corinthians 13:4");
        assert_eq!(
            normalize_text("2nd Timothy chapter 3"),
            "2 Timothy chapter 3"
        );
        assert_eq!(normalize_text("3rd John 4"), "3 John 4");
    }

    #[test]
    fn never_touches_ordinals_past_third() {
        // No canonical Bible book is ever numbered past 3, so "fourth" is
        // deliberately left unconverted rather than guessed at.
        assert_eq!(normalize_text("fourth"), "fourth");
        assert_eq!(normalize_text("4th"), "4th");
    }
}
