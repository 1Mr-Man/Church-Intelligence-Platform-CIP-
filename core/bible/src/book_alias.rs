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

/// Resolve arbitrary book text (`"Romans"`, `"Rom"`, `"Rom."`, `"rom"`) to
/// its [`CanonicalBook`]. Returns `None` if nothing matches - callers must
/// not guess a book on a failed lookup.
pub fn canonicalize_book(input: &str) -> Option<&'static CanonicalBook> {
    let needle = normalize_token(input);
    if needle.is_empty() {
        return None;
    }
    BOOKS.iter().find(|book| {
        normalize_token(book.name) == needle || book.aliases.iter().any(|alias| *alias == needle)
    })
}

/// Look up a [`CanonicalBook`] by its stable code (e.g. `"ROM"`).
pub fn book_by_code(code: &str) -> Option<&'static CanonicalBook> {
    BOOKS
        .iter()
        .find(|book| book.code.eq_ignore_ascii_case(code))
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
}
