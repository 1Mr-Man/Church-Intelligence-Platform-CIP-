//! Centralized canonical Bible book registry and alias resolution.
//!
//! This is the *one* place book names/abbreviations are known to the
//! system. Every other module that needs to turn spoken/written text like
//! "Rom", "Rom.", or "Romans" into the canonical book used throughout the
//! rest of the pipeline (`ScriptureReference::book`, `BibleProvider`
//! lookups, `BibleBook::code`) goes through [`canonicalize_book`] -
//! nothing else hard-codes a book name or alias list.
//!
//! Book codes match the `code` values `integrations/bible` stores in
//! `bible_books.code` (e.g. `"ROM"`, `"JHN"`), so a canonicalized book code
//! can be passed straight to `BibleProvider` without any further mapping.

use crate::provider::Testament;

/// A canonical Bible book entry: its stable code, canonical display name,
/// testament, and the aliases that resolve to it.
pub struct CanonicalBook {
    pub code: &'static str,
    pub name: &'static str,
    pub testament: Testament,
    /// Additional forms that resolve to this book, beyond the canonical
    /// `name` itself (which always matches). Matching is case-insensitive
    /// and ignores trailing periods, so `"Rom"`, `"rom"`, and `"Rom."` are
    /// all covered by listing `"rom"` once. `pub(crate)` rather than
    /// private so [`crate::detection`] can build its book-name pattern from
    /// this same table - the alias list still has exactly one owner.
    pub(crate) aliases: &'static [&'static str],
}

macro_rules! book {
    ($code:literal, $name:literal, $testament:expr, [$($alias:literal),* $(,)?]) => {
        CanonicalBook { code: $code, name: $name, testament: $testament, aliases: &[$($alias),*] }
    };
}

/// The 66-book Protestant canon, canonical name + common abbreviations.
/// Not every historical abbreviation variant is included - "Create the
/// architecture so additional normalization rules can be added cleanly"
/// is satisfied by this being the single table to extend, not by the table
/// being exhaustive on day one.
pub const BOOKS: &[CanonicalBook] = &[
    book!("GEN", "Genesis", Testament::Old, ["gen", "ge"]),
    book!("EXO", "Exodus", Testament::Old, ["exod", "exo", "ex"]),
    book!("LEV", "Leviticus", Testament::Old, ["lev", "le"]),
    book!("NUM", "Numbers", Testament::Old, ["num", "nu"]),
    book!("DEU", "Deuteronomy", Testament::Old, ["deut", "deu", "dt"]),
    book!("JOS", "Joshua", Testament::Old, ["josh", "jos"]),
    book!("JDG", "Judges", Testament::Old, ["judg", "jdg"]),
    book!("RUT", "Ruth", Testament::Old, ["rut", "ru"]),
    book!(
        "1SA",
        "1 Samuel",
        Testament::Old,
        ["1 sam", "1sam", "1 sa", "first samuel"]
    ),
    book!(
        "2SA",
        "2 Samuel",
        Testament::Old,
        ["2 sam", "2sam", "2 sa", "second samuel"]
    ),
    book!(
        "1KI",
        "1 Kings",
        Testament::Old,
        ["1 kgs", "1kgs", "1 ki", "first kings"]
    ),
    book!(
        "2KI",
        "2 Kings",
        Testament::Old,
        ["2 kgs", "2kgs", "2 ki", "second kings"]
    ),
    book!(
        "1CH",
        "1 Chronicles",
        Testament::Old,
        ["1 chr", "1chr", "first chronicles"]
    ),
    book!(
        "2CH",
        "2 Chronicles",
        Testament::Old,
        ["2 chr", "2chr", "second chronicles"]
    ),
    book!("EZR", "Ezra", Testament::Old, ["ezr"]),
    book!("NEH", "Nehemiah", Testament::Old, ["neh"]),
    book!("EST", "Esther", Testament::Old, ["esth", "est"]),
    book!("JOB", "Job", Testament::Old, []),
    book!("PSA", "Psalms", Testament::Old, ["ps", "psa", "psalm"]),
    book!("PRO", "Proverbs", Testament::Old, ["prov", "pro"]),
    book!("ECC", "Ecclesiastes", Testament::Old, ["eccl", "ecc"]),
    book!(
        "SNG",
        "Song of Solomon",
        Testament::Old,
        ["song", "sos", "song of songs"]
    ),
    book!("ISA", "Isaiah", Testament::Old, ["isa"]),
    book!("JER", "Jeremiah", Testament::Old, ["jer"]),
    book!("LAM", "Lamentations", Testament::Old, ["lam"]),
    book!("EZK", "Ezekiel", Testament::Old, ["ezek", "ezk"]),
    book!("DAN", "Daniel", Testament::Old, ["dan"]),
    book!("HOS", "Hosea", Testament::Old, ["hos"]),
    book!("JOL", "Joel", Testament::Old, ["joel", "jol"]),
    book!("AMO", "Amos", Testament::Old, ["amos", "amo"]),
    book!("OBA", "Obadiah", Testament::Old, ["obad", "oba"]),
    book!("JON", "Jonah", Testament::Old, ["jon", "jnh"]),
    book!("MIC", "Micah", Testament::Old, ["mic"]),
    book!("NAM", "Nahum", Testament::Old, ["nah", "nam"]),
    book!("HAB", "Habakkuk", Testament::Old, ["hab"]),
    book!("ZEP", "Zephaniah", Testament::Old, ["zeph", "zep"]),
    book!("HAG", "Haggai", Testament::Old, ["hag"]),
    book!("ZEC", "Zechariah", Testament::Old, ["zech", "zec"]),
    book!("MAL", "Malachi", Testament::Old, ["mal"]),
    book!("MAT", "Matthew", Testament::New, ["matt", "mat"]),
    book!("MRK", "Mark", Testament::New, ["mrk", "mk"]),
    book!("LUK", "Luke", Testament::New, ["luk", "lk"]),
    book!("JHN", "John", Testament::New, ["jn", "jhn"]),
    book!("ACT", "Acts", Testament::New, ["act"]),
    book!("ROM", "Romans", Testament::New, ["rom"]),
    book!(
        "1CO",
        "1 Corinthians",
        Testament::New,
        ["1 cor", "1cor", "first corinthians"]
    ),
    book!(
        "2CO",
        "2 Corinthians",
        Testament::New,
        ["2 cor", "2cor", "second corinthians"]
    ),
    book!("GAL", "Galatians", Testament::New, ["gal"]),
    book!("EPH", "Ephesians", Testament::New, ["eph"]),
    book!("PHP", "Philippians", Testament::New, ["phil", "php"]),
    book!("COL", "Colossians", Testament::New, ["col"]),
    book!(
        "1TH",
        "1 Thessalonians",
        Testament::New,
        ["1 thess", "1thess", "first thessalonians"]
    ),
    book!(
        "2TH",
        "2 Thessalonians",
        Testament::New,
        ["2 thess", "2thess", "second thessalonians"]
    ),
    book!(
        "1TI",
        "1 Timothy",
        Testament::New,
        ["1 tim", "1tim", "first timothy"]
    ),
    book!(
        "2TI",
        "2 Timothy",
        Testament::New,
        ["2 tim", "2tim", "second timothy"]
    ),
    book!("TIT", "Titus", Testament::New, ["tit"]),
    book!("PHM", "Philemon", Testament::New, ["phlm", "phm"]),
    book!("HEB", "Hebrews", Testament::New, ["heb"]),
    book!("JAS", "James", Testament::New, ["jas"]),
    book!(
        "1PE",
        "1 Peter",
        Testament::New,
        ["1 pet", "1pet", "first peter"]
    ),
    book!(
        "2PE",
        "2 Peter",
        Testament::New,
        ["2 pet", "2pet", "second peter"]
    ),
    book!(
        "1JN",
        "1 John",
        Testament::New,
        ["1 jn", "1jn", "1 john", "first john"]
    ),
    book!(
        "2JN",
        "2 John",
        Testament::New,
        ["2 jn", "2jn", "2 john", "second john"]
    ),
    book!(
        "3JN",
        "3 John",
        Testament::New,
        ["3 jn", "3jn", "3 john", "third john"]
    ),
    book!("JUD", "Jude", Testament::New, ["jud"]),
    book!("REV", "Revelation", Testament::New, ["rev"]),
];

/// Normalize a raw book token (arbitrary case/punctuation) for comparison:
/// lowercase, trailing periods stripped, surrounding whitespace trimmed,
/// internal whitespace collapsed to single spaces.
fn normalize_token(input: &str) -> String {
    input
        .trim()
        .trim_end_matches('.')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve arbitrary book text (`"Romans"`, `"Rom"`, `"Rom."`, `"rom"`, or
/// the book's own stable code `"ROM"`) to its [`CanonicalBook`]. Returns
/// `None` if nothing matches - callers must not guess a book on a failed
/// lookup.
///
/// Matching a book's own `code` (case-insensitively) is independent of
/// its `aliases` list: several codes (e.g. `"1SA"`, `"SNG"`) aren't
/// themselves listed as aliases (the alias list has `"1 sa"`/`"1sam"`,
/// not `"1sa"`), since aliases were curated for spoken/written text a
/// pastor might actually say, not for exhaustively covering every stable
/// code. A structured caller (the dataset importer, `core/bible::search`)
/// that already has the canonical code must still resolve it - see
/// `docs/bible-datasets.md`.
pub fn canonicalize_book(input: &str) -> Option<&'static CanonicalBook> {
    let needle = normalize_token(input);
    if needle.is_empty() {
        return None;
    }
    BOOKS.iter().find(|book| {
        book.code.eq_ignore_ascii_case(&needle)
            || normalize_token(book.name) == needle
            || book.aliases.iter().any(|alias| *alias == needle)
    })
}

/// Look up a [`CanonicalBook`] by its stable code (e.g. `"ROM"`).
pub fn book_by_code(code: &str) -> Option<&'static CanonicalBook> {
    BOOKS
        .iter()
        .find(|book| book.code.eq_ignore_ascii_case(code))
}

/// Classic Levenshtein (single-character insert/delete/substitute) edit
/// distance between two strings, operating on `char`s so it stays correct
/// for non-ASCII input rather than silently truncating/miscounting it.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev_row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut curr_row = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr_row[j + 1] = (prev_row[j] + cost)
                .min(prev_row[j + 1] + 1)
                .min(curr_row[j] + 1);
        }
        prev_row = curr_row;
    }
    prev_row[b.len()]
}

/// The maximum edit distance a near-miss book name may still be trusted
/// at, scaled to how long the canonical name is - a 1-character slip in a
/// 4-letter name ("Ruth" -> "Ruht") is proportionally a much bigger change
/// than the same slip in a 10-letter name ("Revelation" -> "Revelaton"),
/// so a single fixed distance would either reject obvious short-name typos
/// or accept wild long-name guesses.
fn max_allowed_distance(canonical_len: usize) -> usize {
    match canonical_len {
        0..=5 => 1,
        6..=9 => 2,
        _ => 3,
    }
}

/// Fuzzy (near-miss) match a single spoken/transcribed word against the
/// canonical, single-word Bible book names - the tolerant counterpart to
/// [`canonicalize_book`] for when Whisper mis-transcribes a book name
/// closely enough that no exact alias matches at all (e.g. `"Roman"` for
/// `"Romans"`, `"Corinthans"` for `"Corinthians"`, `"Revelations"` for
/// `"Revelation"`).
///
/// Deliberately scoped to books whose canonical `name` has no internal
/// space - `"1 Corinthians"`, `"2 Timothy"`, `"Song of Solomon"`, and
/// every other multi-word name are excluded, since fuzzy-matching a single
/// mis-heard word against a multi-word name (or worse, guessing which of
/// two numbered variants, `"1 John"` vs `"2 John"`, a bare near-miss word
/// meant) is a fundamentally different, higher-risk problem than this
/// function solves - those books are already reachable through
/// [`canonicalize_book`]'s exact alias table (`"1 cor"`, `"2 tim"`, ...),
/// so this only needs to cover the remaining gap: an *exact* spelling
/// this codebase doesn't already know, for a book with only one plausible
/// referent.
///
/// Returns `None` - never guesses - when: the input is too short to
/// fuzzy-match reliably (under 4 characters), no book comes within its
/// length-scaled distance budget ([`max_allowed_distance`]), or more than
/// one book ties for the closest match (an ambiguous near-miss, e.g. a
/// word equidistant from two different short book names, must never be
/// silently resolved to either).
///
/// Callers must always try [`canonicalize_book`] first - this function
/// does not check for an exact match itself, so calling it on text that
/// already resolves exactly would still return a (correct, but redundant
/// and unnecessarily uncertain-looking) fuzzy result.
pub fn fuzzy_match_book(input: &str) -> Option<(&'static CanonicalBook, f32)> {
    let needle = normalize_token(input);
    if needle.chars().count() < 4 || !needle.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    let mut best: Option<(&'static CanonicalBook, usize)> = None;
    let mut best_is_unique = true;

    for book in BOOKS {
        if book.name.contains(' ') {
            continue;
        }
        let canonical = normalize_token(book.name);
        let distance = levenshtein(&needle, &canonical);
        if distance > max_allowed_distance(canonical.chars().count()) {
            continue;
        }
        match &best {
            None => {
                best = Some((book, distance));
                best_is_unique = true;
            }
            Some((_, best_distance)) if distance < *best_distance => {
                best = Some((book, distance));
                best_is_unique = true;
            }
            Some((_, best_distance)) if distance == *best_distance => {
                best_is_unique = false;
            }
            _ => {}
        }
    }

    let (book, distance) = best?;
    if !best_is_unique {
        return None;
    }
    let canonical_len = normalize_token(book.name).chars().count().max(1);
    let similarity = 1.0 - (distance as f32 / canonical_len.max(needle.chars().count()) as f32);
    Some((book, similarity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_canonical_name_case_insensitively() {
        assert_eq!(canonicalize_book("romans").unwrap().code, "ROM");
        assert_eq!(canonicalize_book("Romans").unwrap().code, "ROM");
    }

    #[test]
    fn resolves_abbreviation_and_punctuated_abbreviation() {
        assert_eq!(canonicalize_book("Rom").unwrap().code, "ROM");
        assert_eq!(canonicalize_book("Rom.").unwrap().code, "ROM");
        assert_eq!(canonicalize_book("ROM.").unwrap().code, "ROM");
    }

    #[test]
    fn resolves_multi_word_ordinal_books() {
        assert_eq!(canonicalize_book("1 Corinthians").unwrap().code, "1CO");
        assert_eq!(canonicalize_book("First Corinthians").unwrap().code, "1CO");
    }

    #[test]
    fn unknown_book_text_does_not_resolve() {
        assert!(canonicalize_book("Frobnicate").is_none());
        assert!(canonicalize_book("").is_none());
    }

    #[test]
    fn every_book_alias_resolves_back_to_its_own_code() {
        for book in BOOKS {
            assert_eq!(canonicalize_book(book.name).unwrap().code, book.code);
            for alias in book.aliases {
                assert_eq!(
                    canonicalize_book(alias).unwrap().code,
                    book.code,
                    "alias {alias:?} did not resolve back to {}",
                    book.code
                );
            }
        }
    }

    #[test]
    fn every_canonical_code_resolves_to_itself_case_insensitively() {
        // Not every code is also listed as an alias (e.g. "1SA"'s aliases
        // are "1 sa"/"1sam", not "1sa"; "SNG" has no "sng" alias at all) -
        // a structured caller that already has the code must still
        // resolve it regardless. Phase 1.5's dataset importer and search
        // dispatcher both depend on this.
        for book in BOOKS {
            assert_eq!(canonicalize_book(book.code).unwrap().code, book.code);
            assert_eq!(
                canonicalize_book(&book.code.to_lowercase()).unwrap().code,
                book.code
            );
        }
    }

    #[test]
    fn a_code_not_listed_as_its_own_alias_still_resolves() {
        assert_eq!(canonicalize_book("1SA").unwrap().code, "1SA");
        assert_eq!(canonicalize_book("SNG").unwrap().code, "SNG");
    }

    // --- Phase 20: fuzzy_match_book ---------------------------------

    #[test]
    fn fuzzy_matches_a_plausible_near_miss_spelling() {
        // "Roman" for "Romans" - a real shape of Whisper mishearing a
        // trailing "s".
        let (book, similarity) = fuzzy_match_book("Roman").unwrap();
        assert_eq!(book.code, "ROM");
        assert!(similarity > 0.5, "similarity was {similarity}");
    }

    #[test]
    fn fuzzy_matches_a_plural_confusion() {
        let (book, _) = fuzzy_match_book("Revelations").unwrap();
        assert_eq!(book.code, "REV");
    }

    #[test]
    fn fuzzy_matches_a_single_dropped_letter() {
        // "Galatins" is "Galatians" missing one internal "a".
        let (book, _) = fuzzy_match_book("Galatins").unwrap();
        assert_eq!(book.code, "GAL");
    }

    #[test]
    fn fuzzy_match_is_case_insensitive() {
        assert_eq!(fuzzy_match_book("ROMAN").unwrap().0.code, "ROM");
        assert_eq!(fuzzy_match_book("roman").unwrap().0.code, "ROM");
    }

    #[test]
    fn refuses_to_fuzzy_match_input_under_four_characters() {
        // Too short to fuzzy-match reliably, even though "Amo" is only one
        // deletion away from "Amos" - a 3-character needle is close to
        // almost every short book name at once.
        assert!(fuzzy_match_book("Amo").is_none());
        assert!(fuzzy_match_book("").is_none());
    }

    #[test]
    fn refuses_to_fuzzy_match_non_alphabetic_input() {
        assert!(fuzzy_match_book("1234").is_none());
        assert!(fuzzy_match_book("8:28").is_none());
    }

    #[test]
    fn never_fuzzy_matches_a_multi_word_canonical_name() {
        // "1 Corinthians"/"2 Corinthians" both contain a space, so the
        // bare word "corinthians" - which would otherwise be a plausible
        // near-miss of either - is deliberately never fuzzy-matched: see
        // fuzzy_match_book's own doc comment for why guessing between two
        // numbered variants is out of scope here.
        assert!(fuzzy_match_book("corinthians").is_none());
        assert!(fuzzy_match_book("thessalonians").is_none());
    }

    #[test]
    fn single_character_typos_of_single_word_names_never_resolve_to_the_wrong_book() {
        // A systematic sweep: for every book with a single-word canonical
        // name, dropping its last letter (a plausible "Whisper cut it
        // short" mishearing) either resolves back to that exact book, or
        // is honestly refused (e.g. an ambiguous near-tie, or now too
        // short) - it must never resolve to a *different* book.
        for book in BOOKS {
            if book.name.contains(' ') {
                continue;
            }
            let mut truncated = book.name.to_lowercase();
            truncated.pop();
            if truncated.chars().count() < 4 {
                continue;
            }
            if let Some((matched, _)) = fuzzy_match_book(&truncated) {
                assert_eq!(
                    matched.code, book.code,
                    "{truncated:?} (from {:?}) incorrectly matched {} instead",
                    book.name, matched.code
                );
            }
        }
    }
}
