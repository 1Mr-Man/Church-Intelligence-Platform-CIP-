//! Bible dataset integrity checking (Phase 1.5) - "is what's actually
//! stored internally consistent, and how much of the 66-book canon does
//! it cover?"
//!
//! Deliberately checks only *structural* properties derivable from
//! [`BibleProvider`] itself (book presence against the canonical catalog,
//! well-formed/non-zero chapter and verse numbers, duplicate
//! chapters/verses, empty verse text, `book_order` consistency) - never a
//! hard-coded "Romans has 16 chapters, chapter 8 has 39 verses" table of
//! canonical ground truth. Baking in canonical chapter/verse *counts*
//! would mean inventing Bible content facts this crate has no
//! authoritative source for, and would also misclassify any legitimate
//! partial dataset (a development fixture with only a handful of verses)
//! as broken. Checking *internal consistency* of whatever is actually
//! stored - never requiring chapters/verses to start at 1 or have no
//! gaps - needs no such table, still catches real defects (duplicates,
//! empty text, malformed/zero numbering), and correctly leaves "this
//! dataset just doesn't cover everything yet" as `Incomplete`, not
//! `Invalid`.

use crate::book_alias::BOOKS;
use crate::provider::{BibleProvider, BibleProviderError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    /// Every canonical book is present and everything checked is
    /// internally consistent.
    Valid,
    /// Nothing inconsistent was found, but not every canonical book is
    /// present - e.g. a development fixture. Never treated as broken.
    Incomplete,
    /// A structural inconsistency was found (gap, duplicate, empty text,
    /// malformed numbering, or an ordering mismatch).
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityIssue {
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub translation_id: String,
    pub status: IntegrityStatus,
    pub books_present: usize,
    /// The size of the canonical 66-book catalog - a fixed constant, not
    /// a claim about how many books *should* exist in every translation
    /// (some canons differ); it's the denominator for "how complete is
    /// this dataset relative to the catalog CIP already knows."
    pub books_expected: usize,
    pub chapters_checked: usize,
    pub verses_checked: usize,
    pub issues: Vec<IntegrityIssue>,
}

fn issue(description: impl Into<String>) -> IntegrityIssue {
    IntegrityIssue {
        description: description.into(),
    }
}

/// Checks one translation's stored content against `provider`. Never
/// mutates or deletes anything - a caller decides what (if anything) to
/// do with an `Incomplete`/`Invalid` result (section 11: "do not
/// automatically delete incomplete datasets").
pub fn check_bible_integrity(
    provider: &dyn BibleProvider,
    translation_id: &str,
) -> Result<IntegrityReport, BibleProviderError> {
    let mut issues = Vec::new();
    let mut chapters_checked = 0usize;
    let mut verses_checked = 0usize;
    let mut present_in_canonical_order: Vec<(&'static str, u32)> = Vec::new();

    for book in BOOKS {
        let Some(book_meta) = provider.get_book(translation_id, book.code)? else {
            continue;
        };
        present_in_canonical_order.push((book.code, book_meta.order));

        let chapters = provider.list_chapters(translation_id, book.code)?;
        if chapters.is_empty() {
            issues.push(issue(format!(
                "{}: book present but has no chapters",
                book.name
            )));
            continue;
        }

        // Deliberately does NOT require chapters/verses to be contiguous
        // from 1 - a legitimate partial/development dataset (e.g. only
        // Romans 8, only verses 18 and 28-31 of it) is exactly the
        // `Incomplete` case, not `Invalid`. What's checked instead is
        // purely structural: numbers can never be zero, can never repeat,
        // and text can never be empty - none of that requires knowing
        // the true canonical chapter/verse counts.
        let mut seen_chapters = HashSet::new();
        for &chapter_number in &chapters {
            chapters_checked += 1;
            if chapter_number == 0 {
                issues.push(issue(format!("{}: malformed chapter number 0", book.name)));
                continue;
            }
            if !seen_chapters.insert(chapter_number) {
                issues.push(issue(format!(
                    "{} {chapter_number}: duplicate chapter number",
                    book.name
                )));
            }

            let Some(chapter_data) =
                provider.get_chapter(translation_id, book.code, chapter_number)?
            else {
                issues.push(issue(format!(
                    "{} {chapter_number}: listed by list_chapters but get_chapter found nothing",
                    book.name
                )));
                continue;
            };
            if chapter_data.verses.is_empty() {
                issues.push(issue(format!(
                    "{} {chapter_number}: chapter present but has no verses",
                    book.name
                )));
                continue;
            }

            let mut seen_verses = HashSet::new();
            for verse in &chapter_data.verses {
                verses_checked += 1;
                let n = verse.reference.verse_start;
                if n == 0 {
                    issues.push(issue(format!(
                        "{} {chapter_number}: malformed verse number 0",
                        book.name
                    )));
                    continue;
                }
                if !seen_verses.insert(n) {
                    issues.push(issue(format!(
                        "{} {chapter_number}:{n}: duplicate verse reference",
                        book.name
                    )));
                }
                if verse.text.trim().is_empty() {
                    issues.push(issue(format!(
                        "{} {chapter_number}:{n}: empty verse text",
                        book.name
                    )));
                }
            }
        }
    }

    // Canonical book ordering: whichever books are present, their stored
    // `book_order` must sort into the same relative sequence the
    // canonical catalog itself lists them in - checkable without knowing
    // anything about books that aren't present at all.
    let mut sorted_by_stored_order = present_in_canonical_order.clone();
    sorted_by_stored_order.sort_by_key(|(_, order)| *order);
    let canonical_sequence: Vec<&str> =
        present_in_canonical_order.iter().map(|(c, _)| *c).collect();
    let stored_sequence: Vec<&str> = sorted_by_stored_order.iter().map(|(c, _)| *c).collect();
    if canonical_sequence != stored_sequence {
        issues.push(issue(
            "book_order values are inconsistent with canonical book ordering",
        ));
    }

    let books_present = present_in_canonical_order.len();
    let books_expected = BOOKS.len();
    let status = if !issues.is_empty() {
        IntegrityStatus::Invalid
    } else if books_present < books_expected {
        IntegrityStatus::Incomplete
    } else {
        IntegrityStatus::Valid
    };

    Ok(IntegrityReport {
        translation_id: translation_id.to_string(),
        status,
        books_present,
        books_expected,
        chapters_checked,
        verses_checked,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FakeBibleProvider;

    #[test]
    fn a_development_fixture_is_reported_incomplete_not_invalid() {
        let provider = FakeBibleProvider::kjv_fixture();
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Incomplete);
        assert!(
            report.issues.is_empty(),
            "a self-consistent fixture must have no issues: {:?}",
            report.issues
        );
        assert_eq!(report.books_present, 2);
        assert_eq!(report.books_expected, 66);
    }

    #[test]
    fn a_complete_self_consistent_catalog_is_valid() {
        // One chapter, one verse per canonical book - complete in the
        // "every book present" sense the checker measures, without
        // claiming to be a real, full-length Bible.
        let entries: Vec<(&str, u32, u32, &str)> = BOOKS
            .iter()
            .map(|b| (b.code, 1, 1, "In the beginning..."))
            .collect();
        let provider = FakeBibleProvider::new("KJV", &entries);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Valid);
        assert_eq!(report.books_present, 66);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn a_verse_number_gap_is_not_flagged_as_invalid_for_a_partial_dataset() {
        // Chapter has verses 1 and 3, skipping 2 - a legitimate partial
        // dataset (like the real dev fixture, which only has verses
        // 18/28-31 of Romans 8), not a defect.
        let provider = FakeBibleProvider::new("KJV", &[("ROM", 1, 1, "a"), ("ROM", 1, 3, "b")]);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert!(report.issues.is_empty());
        assert_eq!(report.status, IntegrityStatus::Incomplete);
    }

    #[test]
    fn empty_verse_text_is_reported_invalid() {
        let provider = FakeBibleProvider::new("KJV", &[("ROM", 1, 1, "")]);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Invalid);
        assert!(report
            .issues
            .iter()
            .any(|i| i.description.contains("empty verse text")));
    }

    #[test]
    fn a_chapter_number_gap_is_not_flagged_as_invalid_for_a_partial_dataset() {
        // Chapters 1 and 3 exist, chapter 2 is missing entirely - again a
        // legitimate partial dataset, not a defect.
        let provider = FakeBibleProvider::new("KJV", &[("ROM", 1, 1, "a"), ("ROM", 3, 1, "b")]);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert!(report.issues.is_empty());
        assert_eq!(report.status, IntegrityStatus::Incomplete);
    }

    #[test]
    fn a_zero_verse_number_is_reported_invalid() {
        let provider = FakeBibleProvider::new("KJV", &[("ROM", 1, 0, "malformed")]);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Invalid);
        assert!(report
            .issues
            .iter()
            .any(|i| i.description.contains("malformed verse number")));
    }

    #[test]
    fn a_zero_chapter_number_is_reported_invalid() {
        let provider = FakeBibleProvider::new("KJV", &[("ROM", 0, 1, "malformed")]);
        let report = check_bible_integrity(&provider, "KJV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Invalid);
        assert!(report
            .issues
            .iter()
            .any(|i| i.description.contains("malformed chapter number")));
    }

    #[test]
    fn an_unregistered_translation_is_reported_incomplete_with_no_books() {
        let provider = FakeBibleProvider::kjv_fixture();
        let report = check_bible_integrity(&provider, "NIV").unwrap();
        assert_eq!(report.status, IntegrityStatus::Incomplete);
        assert_eq!(report.books_present, 0);
        assert!(report.issues.is_empty());
    }
}
