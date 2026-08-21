use serde::{Deserialize, Serialize};
use std::fmt;

/// A fully resolved reference to a single verse or verse range, e.g.
/// "Romans 8:28" or "Romans 8:28-30".
///
/// `book` is a stable machine-readable code (e.g. "ROM"), never a display
/// name - localized/display names are a presentation concern, not a domain
/// one, so they are intentionally not stored here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptureReference {
    pub translation_id: String,
    pub book: String,
    pub chapter: u32,
    pub verse_start: u32,
    pub verse_end: Option<u32>,
}

impl ScriptureReference {
    pub fn single(
        translation_id: impl Into<String>,
        book: impl Into<String>,
        chapter: u32,
        verse: u32,
    ) -> Self {
        Self {
            translation_id: translation_id.into(),
            book: book.into(),
            chapter,
            verse_start: verse,
            verse_end: None,
        }
    }

    pub fn is_range(&self) -> bool {
        self.verse_end.is_some_and(|end| end != self.verse_start)
    }
}

impl fmt::Display for ScriptureReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.verse_end {
            Some(end) if end != self.verse_start => {
                write!(
                    f,
                    "{} {}:{}-{}",
                    self.book, self.chapter, self.verse_start, end
                )
            }
            _ => write!(f, "{} {}:{}", self.book, self.chapter, self.verse_start),
        }
    }
}

/// A partially spoken reference, as it arrives incrementally from speech
/// recognition before the Scripture Context Manager resolves it against the
/// active context. Any field left `None` must be filled in from context.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialScriptureReference {
    pub book: Option<String>,
    pub chapter: Option<u32>,
    pub verse_start: Option<u32>,
    pub verse_end: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_single_verse_without_range() {
        let r = ScriptureReference::single("KJV", "ROM", 8, 28);
        assert_eq!(r.to_string(), "ROM 8:28");
    }

    #[test]
    fn displays_verse_range() {
        let mut r = ScriptureReference::single("KJV", "ROM", 8, 28);
        r.verse_end = Some(30);
        assert_eq!(r.to_string(), "ROM 8:28-30");
        assert!(r.is_range());
    }
}
