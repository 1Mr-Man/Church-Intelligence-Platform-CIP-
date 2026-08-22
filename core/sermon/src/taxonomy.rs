//! [`SermonElementKind`]: the closed taxonomy of sermon structural/meaning
//! elements Phase 2.3 recognizes (spec section 6). Deliberately closed (no
//! open-ended "other" catch-all) so every detector's output is one of a
//! fixed, documented set - see `docs/sermon-intelligence.md` for what each
//! variant means and why `Illustration`/`Story`/`Example` share one
//! detector despite being three separate taxonomy entries.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SermonElementKind {
    /// The sermon's overall theme/subject - never a single-segment
    /// detection; always derived from accumulated evidence (see
    /// [`crate::theme::ThemeTracker`]).
    Theme,
    /// An explicit main structural point ("my first point is...").
    MainPoint,
    /// A point nested under the current main point ("under this point...").
    SubPoint,
    /// A Scripture reference in sermon-structural context (e.g. "Supporting
    /// Scripture: Romans 10:17" attached to a point). Never detected by
    /// this crate directly - the actual reference always comes from the
    /// Bible Intelligence Engine; this kind exists so the sermon adapter
    /// can label a cross-linked Bible finding's role in the sermon
    /// structure (spec section 16/47).
    ScriptureReference,
    /// A quoted phrase from a Scripture text, captured only from literal
    /// quotation marks already present in the transcript - never an
    /// invented quotation boundary (spec section 17).
    ScriptureQuotation,
    /// An explicit definition ("Faith is...", "by faith I mean...").
    Definition,
    /// A direct, quotable assertion ("The truth is...", "never forget...").
    KeyStatement,
    /// Strong, explicit declarative/imperative language ("I declare...",
    /// "in Jesus' name...") - never a bare future-tense sentence.
    Declaration,
    /// A question posed in the transcript (rhetorical, teaching, or
    /// audience-directed).
    Question,
    /// An illustrative image/comparison ("imagine...", "for instance...").
    Illustration,
    /// A personal or narrative anecdote ("I remember when...", "there was
    /// a man...").
    Story,
    /// A concrete example offered as evidence ("let me give you an
    /// example...", "for instance...").
    Example,
    /// A practical takeaway ("you need to...", "we must...").
    Application,
    /// An explicit call to prayer ("let's pray...", "pray that...").
    PrayerPoint,
    /// An explicit summary of what's been said ("to summarize...").
    Summary,
    /// A reflective/food-for-thought classification of a question -
    /// always an inference about the *purpose* of a question, never a
    /// claim the pastor used the words "food for thought" unless they did
    /// (spec section 25).
    Reflection,
    /// A structural move between sermon phases (introduction -> point,
    /// point -> point, teaching -> application, application -> conclusion).
    Transition,
    /// A signal that the sermon may be concluding ("in conclusion...",
    /// "finally...") - never treated as certain; the pastor may continue.
    Conclusion,
}

impl SermonElementKind {
    /// SCREAMING_SNAKE_CASE label, matching this codebase's established
    /// `AppEvent::name`/`ReferenceKind::label` convention.
    pub const fn label(self) -> &'static str {
        match self {
            SermonElementKind::Theme => "SERMON_THEME",
            SermonElementKind::MainPoint => "SERMON_MAIN_POINT",
            SermonElementKind::SubPoint => "SERMON_SUB_POINT",
            SermonElementKind::ScriptureReference => "SERMON_SCRIPTURE_REFERENCE",
            SermonElementKind::ScriptureQuotation => "SERMON_SCRIPTURE_QUOTATION",
            SermonElementKind::Definition => "SERMON_DEFINITION",
            SermonElementKind::KeyStatement => "SERMON_KEY_STATEMENT",
            SermonElementKind::Declaration => "SERMON_DECLARATION",
            SermonElementKind::Question => "SERMON_QUESTION",
            SermonElementKind::Illustration => "SERMON_ILLUSTRATION",
            SermonElementKind::Story => "SERMON_STORY",
            SermonElementKind::Example => "SERMON_EXAMPLE",
            SermonElementKind::Application => "SERMON_APPLICATION",
            SermonElementKind::PrayerPoint => "SERMON_PRAYER_POINT",
            SermonElementKind::Summary => "SERMON_SUMMARY",
            SermonElementKind::Reflection => "SERMON_REFLECTION",
            SermonElementKind::Transition => "SERMON_TRANSITION",
            SermonElementKind::Conclusion => "SERMON_CONCLUSION",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SermonElementKind; 17] = [
        SermonElementKind::Theme,
        SermonElementKind::MainPoint,
        SermonElementKind::SubPoint,
        SermonElementKind::ScriptureReference,
        SermonElementKind::ScriptureQuotation,
        SermonElementKind::Definition,
        SermonElementKind::KeyStatement,
        SermonElementKind::Declaration,
        SermonElementKind::Question,
        SermonElementKind::Illustration,
        SermonElementKind::Story,
        SermonElementKind::Example,
        SermonElementKind::Application,
        SermonElementKind::PrayerPoint,
        SermonElementKind::Summary,
        SermonElementKind::Reflection,
        SermonElementKind::Transition,
    ];

    #[test]
    fn every_label_is_distinct_and_screaming_snake_case() {
        let mut labels: Vec<&str> = ALL.iter().map(|k| k.label()).collect();
        labels.push(SermonElementKind::Conclusion.label());
        let unique_before = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), unique_before);
        for label in labels {
            assert_eq!(label, label.to_uppercase());
            assert!(label.starts_with("SERMON_"));
        }
    }
}
