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

use crate::book_alias::BOOKS;
use crate::reference::PartialScriptureReference;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// How a piece of transcript text relates to a Bible reference.
///
/// [`detect_candidates`] only ever produces `Direct`, `Chapter`, or `Verse`
/// (syntactic classification). `Sequential`, `Ambiguous`, and `Unresolved`
/// are pipeline-level outcomes assigned by `core/service`'s orchestrator
/// once a `Verse` candidate has been resolved against context and validated
/// against a `BibleProvider` - they share this type because they are all
/// still "what kind of reference is this," just decided at a later stage
/// once more information (active context, Bible data) is available.
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
        }
    }
}

/// One syntactically-detected reference candidate, in the order it appeared
/// in the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedCandidate {
    /// Always `Direct`, `Chapter`, or `Verse` - see [`ReferenceKind`] docs.
    pub kind: ReferenceKind,
    pub partial: PartialScriptureReference,
    /// The exact substring that produced this candidate, kept for
    /// diagnostics/audit - never re-parsed.
    pub raw_text: String,
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
}
