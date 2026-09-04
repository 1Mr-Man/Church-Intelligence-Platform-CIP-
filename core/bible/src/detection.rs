//! Deterministic Bible reference detection - the
//! `TEXT NORMALIZATION -> SCRIPTURE REFERENCE DETECTION` stage of the Bible
//! Intelligence Core pipeline.
//!
//! [`detect_candidates`] is pure syntax: it finds explicit `Book`,
//! `Book chapter`, and `Book chapter:verse` patterns (in any of the
//! supported spoken/written forms) plus bare `verse N` fragments, and
//! returns them as [`DetectedCandidate`]s in the order they appear in the
//! text. It does **not** know whether a chapter or verse actually exists in
//! any Bible translation (that's `BIBLE VALIDATION`, done by the caller
//! against a `BibleProvider`) and it does not resolve bare verse fragments
//! against an active context (that's `core/service`'s job, via
//! `ScriptureContextManager`) - this module only answers "what reference
//! shapes are present in this text."
//!
//! Run text through [`crate::normalize::normalize_text`] first so number
//! words are already digits; this module only matches digit patterns.
//!
//! A segment with no citation shape at all may still be a paraphrase of a
//! verse - that lexical-overlap detection is a separate, later fallback
//! (see `crate::paraphrase` and `core/service`'s orchestrator), not part of
//! this module's pure syntax matching.

use crate::book_alias::BOOKS;
use crate::reference::PartialScriptureReference;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// How a piece of transcript text relates to a Bible reference.
///
/// [`detect_candidates`] only ever produces `Direct`, `Chapter`, or `Verse`
/// (syntactic classification). `Sequential`, `Ambiguous`, `Unresolved`, and
/// `Paraphrase` are pipeline-level outcomes assigned by `core/service`'s
/// orchestrator - `Sequential`/`Ambiguous`/`Unresolved` once a `Verse`
/// candidate has been resolved against context and validated against a
/// `BibleProvider`, and `Paraphrase` when a segment produced no syntactic
/// candidate at all but its wording overlaps a verse's text closely enough
/// to suggest the operator paraphrased it without citing it - they share
/// this type because they are all still "what kind of reference is this,"
/// just decided at a later stage once more information (active context,
/// Bible data) is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Fully explicit: book, chapter, and verse all stated together
    /// (`"Romans 8:28"`).
    Direct,
    /// Book and chapter only, no verse (`"Romans 8"`) - establishes
    /// context; never resolves to a suggestion on its own.
    Chapter,
    /// A bare `"verse N"` fragment, resolved against an active context -
    /// the first verse pulled from that context.
    Verse,
    /// A bare `"verse N"` fragment resolved against an active context that
    /// already had a previously-resolved verse (i.e. continuing within the
    /// same context, regardless of direction - "verse 31" and "verse 18"
    /// after "verse 28" are both `Sequential`).
    Sequential,
    /// More than one validated candidate reference is plausible; a human
    /// must disambiguate.
    Ambiguous,
    /// Could not be resolved into a validated reference at all.
    Unresolved,
    /// No book/chapter/verse was cited at all, but the segment's wording
    /// shares enough distinctive vocabulary with a specific verse that it
    /// was very likely paraphrased from it (e.g. "all things work together
    /// for good" -> Romans 8:28). This is **lexical/keyword-overlap
    /// matching**, not semantic/neural understanding - see
    /// `core/bible::paraphrase`'s module docs for exactly what it can and
    /// cannot detect. Always `Pending` and always requires operator
    /// approval, like every other suggestion - never auto-projected.
    Paraphrase,
    /// No book/chapter/verse was cited, and `Paraphrase`'s lexical-overlap
    /// heuristic also found nothing above its threshold, but a local
    /// embedding model judged the segment's *meaning* close enough to a
    /// specific verse (e.g. "Jesus said we should love our enemies" ->
    /// Matthew 5:44, sharing almost no vocabulary at all) - see
    /// `core/bible::semantic`'s module docs. Only ever produced when Phase
    /// 4.4's `semantic-search` feature is enabled and a real embedding
    /// model is configured; the fallback chain silently skips this tier
    /// otherwise, exactly as it always has. Always `Pending` and always
    /// requires operator approval - never auto-projected.
    Semantic,
    /// A real citation shape (a word immediately followed by a
    /// chapter:verse or chapter-verse pattern) was found, but the word
    /// itself didn't match any known book name or alias exactly - it came
    /// close enough, per [`crate::book_alias::fuzzy_match_book`], to a
    /// single unambiguous book name to be worth surfacing (e.g. "Roman
    /// 8:28" -> Romans 8:28, a plausible Whisper mishearing of the
    /// trailing "s"). This *is* produced directly by [`detect_candidates`]
    /// (unlike `Paraphrase`/`Semantic`, which are pipeline-level
    /// fallbacks) because it's still fundamentally the same syntactic
    /// citation shape, just with a tolerant book-name match - but it is
    /// never as trustworthy as an exact `Direct` match, so it always
    /// carries a real, non-`Unresolved` reference yet a deliberately
    /// dampened confidence score. Always `Pending` and always requires
    /// operator approval - never auto-projected, and never mutates the
    /// active Scripture context the way a real citation would (see
    /// `core/service`'s `resolve_fuzzy_book`).
    FuzzyBook,
}

impl ReferenceKind {
    /// The SCREAMING_SNAKE_CASE label used in logs/events/UI, matching the
    /// convention `apps/desktop/src-tauri/src/events.rs`'s `AppEvent::name`
    /// already established for this codebase.
    pub const fn label(self) -> &'static str {
        match self {
            ReferenceKind::Direct => "DIRECT_REFERENCE",
            ReferenceKind::Chapter => "CHAPTER_REFERENCE",
            ReferenceKind::Verse => "VERSE_REFERENCE",
            ReferenceKind::Sequential => "SEQUENTIAL_REFERENCE",
            ReferenceKind::Ambiguous => "AMBIGUOUS_REFERENCE",
            ReferenceKind::Unresolved => "UNRESOLVED_REFERENCE",
            ReferenceKind::Paraphrase => "PARAPHRASE_REFERENCE",
            ReferenceKind::Semantic => "SEMANTIC_REFERENCE",
            ReferenceKind::FuzzyBook => "FUZZY_BOOK_REFERENCE",
        }
    }
}

/// One syntactically-detected reference candidate, in the order it appeared
/// in the source text.
///
/// `Eq` was dropped from this derive when `fuzzy_score: Option<f32>` was
/// added (Phase 20) - `f32` implements only `PartialEq`, not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedCandidate {
    /// Always `Direct`, `Chapter`, `Verse`, or `FuzzyBook` - see
    /// [`ReferenceKind`] docs.
    pub kind: ReferenceKind,
    pub partial: PartialScriptureReference,
    /// The exact substring that produced this candidate, kept for
    /// diagnostics/audit - never re-parsed.
    pub raw_text: String,
    /// Only present for `FuzzyBook`: the book-name similarity score
    /// [`crate::book_alias::fuzzy_match_book`] returned (`0.0..=1.0`),
    /// carried through so the caller can derive an honestly dampened
    /// confidence rather than reusing a fixed exact-match score. Always
    /// `None` for every other kind.
    pub fuzzy_score: Option<f32>,
}

fn book_pattern() -> Regex {
    let mut alternatives: Vec<String> = Vec::new();
    for book in BOOKS {
        alternatives.push(regex::escape(book.name));
        for alias in book.aliases {
            alternatives.push(regex::escape(alias));
        }
    }
    // Longest alternatives first so e.g. "1 corinthians" is preferred over
    // any shorter alias that happens to be a prefix of it at a given
    // position (regex alternation is leftmost-first, not leftmost-longest).
    alternatives.sort_by_key(|a| std::cmp::Reverse(a.len()));
    Regex::new(&format!(r"(?i)\b(?:{})\.?", alternatives.join("|"))).unwrap()
}

static BOOK_PATTERN: LazyLock<Regex> = LazyLock::new(book_pattern);

/// Shape patterns tried, in priority order, against the text immediately
/// following a matched book name. Each captures either (chapter, verse) or
/// (chapter,) alone.
struct Shape {
    pattern: LazyLock<Regex>,
    kind: ReferenceKind,
}

macro_rules! shape {
    ($pattern:literal, $kind:expr) => {
        Shape {
            pattern: LazyLock::new(|| Regex::new($pattern).unwrap()),
            kind: $kind,
        }
    };
}

// Tried in order; the first that matches at the start of the
// post-book-name text wins. More specific (two-number) shapes precede the
// chapter-only fallbacks so e.g. "8:28" isn't mistaken for just "8".
static SHAPES: [Shape; 6] = [
    // "Romans 8:28" / "Romans 8 : 28"
    shape!(
        r"(?i)^[.,]?\s*(\d{1,3})\s*:\s*(\d{1,3})\b",
        ReferenceKind::Direct
    ),
    // "Romans chapter 8 verse 28"
    shape!(
        r"(?i)^[.,]?\s*chapter\s+(\d{1,3})\s+verse\s+(\d{1,3})\b",
        ReferenceKind::Direct
    ),
    // "Romans 8 verse 28"
    shape!(
        r"(?i)^[.,]?\s*(\d{1,3})\s+verse\s+(\d{1,3})\b",
        ReferenceKind::Direct
    ),
    // "Romans 8 28" (spoken without "chapter"/"verse" at all)
    shape!(r"^[.,]?\s*(\d{1,3})\s+(\d{1,3})\b", ReferenceKind::Direct),
    // "Romans chapter 8" (chapter only)
    shape!(
        r"(?i)^[.,]?\s*chapter\s+(\d{1,3})\b",
        ReferenceKind::Chapter
    ),
    // "Romans 8" (chapter only)
    shape!(r"^[.,]?\s*(\d{1,3})\b", ReferenceKind::Chapter),
];

static BARE_VERSE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bverse\s+(\d{1,3})\b").unwrap());

/// A half-open byte range `[start, end)` in the source text already claimed
/// by a book-anchored candidate, so the bare-verse pass doesn't also emit
/// the same verse number as a second, redundant candidate.
fn overlaps(range: &(usize, usize), point: usize) -> bool {
    point >= range.0 && point < range.1
}

/// Every alphabetic word in the text, independent of any book/alias
/// vocabulary - the fuzzy-book pass's starting point, since it has to
/// consider words `BOOK_PATTERN` didn't already match.
static WORD_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b[a-z]+\b").unwrap());

/// The two-number shapes only (chapter *and* verse both present) -
/// `SHAPES`' first four entries. The fuzzy-book pass deliberately never
/// considers the chapter-only shapes (`SHAPES[4..]`): a near-miss book
/// name paired with only a chapter number would have no verse to suggest
/// and, per [`ReferenceKind::FuzzyBook`]'s docs, must never be trusted
/// enough to establish the active Scripture context the way a real
/// citation does - so a chapter-only fuzzy "match" would do nothing at
/// all except risk a false positive for no benefit.
const FUZZY_SHAPES: usize = 4;

/// Find every Bible reference candidate in `text`, in order. `text` should
/// already be normalized via [`crate::normalize::normalize_text`].
pub fn detect_candidates(text: &str) -> Vec<DetectedCandidate> {
    let mut candidates: Vec<(usize, DetectedCandidate)> = Vec::new();
    let mut consumed: Vec<(usize, usize)> = Vec::new();

    for book_match in BOOK_PATTERN.find_iter(text) {
        let Some(canonical) = crate::book_alias::canonicalize_book(book_match.as_str()) else {
            continue;
        };
        let rest = &text[book_match.end()..];

        for shape in &SHAPES {
            let Some(captures) = shape.pattern.captures(rest) else {
                continue;
            };
            let full_match = captures.get(0).unwrap();
            let start = book_match.start();
            let end = book_match.end() + full_match.end();

            let chapter: u32 = captures[1].parse().unwrap_or(0);
            let verse: Option<u32> = captures.get(2).map(|m| m.as_str().parse().unwrap_or(0));

            let partial = PartialScriptureReference {
                book: Some(canonical.code.to_string()),
                chapter: Some(chapter),
                verse_start: verse,
                verse_end: None,
            };

            candidates.push((
                start,
                DetectedCandidate {
                    kind: shape.kind,
                    partial,
                    raw_text: text[start..end].to_string(),
                    fuzzy_score: None,
                },
            ));
            consumed.push((start, end));
            break;
        }
    }

    // Fuzzy-book pass (Phase 20): for every word `BOOK_PATTERN` didn't
    // already claim, try a near-miss match against the single-word
    // canonical book names, but only trust it enough to emit a candidate
    // when it's also immediately followed by a real chapter:verse shape -
    // the same precision guard the exact pass gets from `BOOK_PATTERN`
    // itself. A fuzzy-matched word with no citation shape after it is far
    // too weak a signal on its own to ever surface.
    for word_match in WORD_PATTERN.find_iter(text) {
        if consumed
            .iter()
            .any(|range| overlaps(range, word_match.start()))
        {
            continue;
        }
        let word = word_match.as_str();
        if crate::book_alias::canonicalize_book(word).is_some() {
            // An exact match `BOOK_PATTERN` should already have claimed -
            // never let the fuzzy pass re-guess a word an exact alias
            // already owns cleanly.
            continue;
        }
        let Some((book, score)) = crate::book_alias::fuzzy_match_book(word) else {
            continue;
        };
        let rest = &text[word_match.end()..];

        for shape in &SHAPES[..FUZZY_SHAPES] {
            let Some(captures) = shape.pattern.captures(rest) else {
                continue;
            };
            let full_match = captures.get(0).unwrap();
            let start = word_match.start();
            let end = word_match.end() + full_match.end();

            let chapter: u32 = captures[1].parse().unwrap_or(0);
            let verse: u32 = captures[2].parse().unwrap_or(0);

            candidates.push((
                start,
                DetectedCandidate {
                    kind: ReferenceKind::FuzzyBook,
                    partial: PartialScriptureReference {
                        book: Some(book.code.to_string()),
                        chapter: Some(chapter),
                        verse_start: Some(verse),
                        verse_end: None,
                    },
                    raw_text: text[start..end].to_string(),
                    fuzzy_score: Some(score),
                },
            ));
            consumed.push((start, end));
            break;
        }
    }

    for captures in BARE_VERSE_PATTERN.captures_iter(text) {
        let full_match = captures.get(0).unwrap();
        if consumed
            .iter()
            .any(|range| overlaps(range, full_match.start()))
        {
            continue;
        }
        let verse: u32 = captures[1].parse().unwrap_or(0);

        candidates.push((
            full_match.start(),
            DetectedCandidate {
                kind: ReferenceKind::Verse,
                partial: PartialScriptureReference {
                    book: None,
                    chapter: None,
                    verse_start: Some(verse),
                    verse_end: None,
                },
                raw_text: full_match.as_str().to_string(),
                fuzzy_score: None,
            },
        ));
    }

    candidates.sort_by_key(|(start, _)| *start);
    candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(text: &str) -> DetectedCandidate {
        let mut candidates = detect_candidates(text);
        assert_eq!(
            candidates.len(),
            1,
            "expected exactly one candidate in {text:?}: {candidates:?}"
        );
        candidates.remove(0)
    }

    #[test]
    fn detects_direct_colon_reference() {
        let c = only("Romans 8:28 says...");
        assert_eq!(c.kind, ReferenceKind::Direct);
        assert_eq!(c.partial.book.as_deref(), Some("ROM"));
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_direct_colon_reference_with_spaces() {
        let c = only("Romans 8 : 28");
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_chapter_only_reference() {
        let c = only("Turn with me to Romans 8.");
        assert_eq!(c.kind, ReferenceKind::Chapter);
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, None);
    }

    #[test]
    fn detects_spoken_chapter_only_reference() {
        let c = only("Turn with me to Romans chapter 8.");
        assert_eq!(c.kind, ReferenceKind::Chapter);
        assert_eq!(c.partial.chapter, Some(8));
    }

    #[test]
    fn detects_spoken_full_reference() {
        let c = only("Romans chapter 8 verse 28");
        assert_eq!(c.kind, ReferenceKind::Direct);
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_bare_two_number_reference() {
        // Already normalized: "Romans eight twenty-eight" -> "Romans 8 28"
        let c = only("Romans 8 28");
        assert_eq!(c.kind, ReferenceKind::Direct);
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_abbreviation() {
        let c = only("Rom 8:28");
        assert_eq!(c.partial.book.as_deref(), Some("ROM"));
    }

    #[test]
    fn detects_punctuated_abbreviation() {
        let c = only("Rom. 8:28");
        assert_eq!(c.partial.book.as_deref(), Some("ROM"));
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_bare_verse_fragment() {
        let c = only("Look at verse 28.");
        assert_eq!(c.kind, ReferenceKind::Verse);
        assert!(c.partial.book.is_none());
        assert_eq!(c.partial.verse_start, Some(28));
    }

    #[test]
    fn detects_bare_verse_fragment_with_leading_filler() {
        let c = only("Go back to verse 18.");
        assert_eq!(c.partial.verse_start, Some(18));
    }

    #[test]
    fn detects_multiple_references_in_order() {
        let candidates = detect_candidates("Compare Romans 8:28 with John 3:16.");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].partial.book.as_deref(), Some("ROM"));
        assert_eq!(candidates[0].partial.verse_start, Some(28));
        assert_eq!(candidates[1].partial.book.as_deref(), Some("JHN"));
        assert_eq!(candidates[1].partial.verse_start, Some(16));
    }

    #[test]
    fn does_not_double_count_the_verse_inside_a_direct_reference() {
        let candidates = detect_candidates("Romans chapter 8 verse 28");
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn plain_prose_produces_no_candidates() {
        assert!(detect_candidates("Let us pray together this morning.").is_empty());
    }

    // --- Phase 20: fuzzy-book detection -----------------------------

    #[test]
    fn detects_a_fuzzy_near_miss_book_name_with_chapter_and_verse() {
        let c = only("Roman 8:28");
        assert_eq!(c.kind, ReferenceKind::FuzzyBook);
        assert_eq!(c.partial.book.as_deref(), Some("ROM"));
        assert_eq!(c.partial.chapter, Some(8));
        assert_eq!(c.partial.verse_start, Some(28));
        assert!(c.fuzzy_score.unwrap() > 0.5);
    }

    #[test]
    fn exact_book_matches_never_produce_a_duplicate_fuzzy_candidate() {
        // "Romans" canonicalizes exactly, so the fuzzy pass must never
        // also emit a second, redundant candidate for the same text.
        let candidates = detect_candidates("Romans 8:28");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, ReferenceKind::Direct);
        assert!(candidates[0].fuzzy_score.is_none());
    }

    #[test]
    fn a_near_miss_book_name_with_no_following_chapter_verse_produces_nothing() {
        // A fuzzy book-name guess alone, with no citation shape after it,
        // is far too weak a signal to ever surface - see FuzzyBook's docs
        // on why this never establishes context the way a real chapter
        // reference does.
        assert!(detect_candidates("Roman was a great empire.").is_empty());
        assert!(detect_candidates("Roman 8").is_empty());
    }

    #[test]
    fn an_unrelated_word_never_fuzzy_matches() {
        assert!(detect_candidates("Pizza 8:28").is_empty());
    }

    #[test]
    fn fuzzy_book_detection_composes_with_an_exact_reference_in_the_same_segment() {
        let candidates = detect_candidates("Compare Roman 8:28 with John 3:16.");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].kind, ReferenceKind::FuzzyBook);
        assert_eq!(candidates[0].partial.book.as_deref(), Some("ROM"));
        assert_eq!(candidates[1].kind, ReferenceKind::Direct);
        assert_eq!(candidates[1].partial.book.as_deref(), Some("JHN"));
    }
}
