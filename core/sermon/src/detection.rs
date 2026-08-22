//! Deterministic, phrase-anchored sermon structural detection (spec
//! sections 12-13, 18-25, 27-28, 32). Mirrors `cip_core_bible::detection`'s
//! `regex` + `LazyLock` shape-array pattern.
//!
//! Every detector here requires an explicit trigger phrase - "Phase 2.3
//! must initially be deterministic" (spec section 32). There is no
//! semantic/purely-statistical structural heuristic: a detection always
//! traces back to a specific matched phrase in `matched_phrase`, so the
//! adapter that turns a [`SermonDetection`] into an `IntelligenceFinding`
//! never has to guess why something was flagged. This is a deliberately
//! narrower scope than the spec's optional "semantic/structural heuristics
//! MAY also identify likely points" (section 12) - see
//! `docs/sermon-intelligence.md`'s "NOT AVAILABLE" section for why that
//! was left out rather than risking a fabricated structural claim.
//!
//! [`detect_elements`] never rewrites or invents text: every
//! [`SermonDetection::extracted_text`] is a verbatim substring of the input
//! (or `None`), and `raw_text` is always the exact segment text that
//! produced the detection.

use crate::taxonomy::SermonElementKind;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// One phrase-anchored detection from a single transcript segment. A
/// segment may produce zero, one, or several of these (e.g. a segment
/// containing both a definition and a key statement).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SermonDetection {
    pub kind: SermonElementKind,
    /// The exact trigger phrase that matched (e.g. `"my first point is"`) -
    /// always a verbatim substring of `raw_text`, never paraphrased.
    pub matched_phrase: String,
    /// The full segment text this detection came from.
    pub raw_text: String,
    /// A verbatim extracted span where the detector can identify one (a
    /// definition's term, a quotation's contents) - `None` when the whole
    /// segment is the relevant content and no narrower span applies.
    pub extracted_text: Option<String>,
}

struct Shape {
    pattern: LazyLock<Regex>,
    kind: SermonElementKind,
}

macro_rules! shape {
    ($pattern:literal, $kind:expr) => {
        Shape {
            pattern: LazyLock::new(|| Regex::new($pattern).unwrap()),
            kind: $kind,
        }
    };
}

// Order matters only in that every matching shape fires (unlike Bible
// detection's "first shape wins" - a sermon segment legitimately carries
// multiple simultaneous element kinds, e.g. a main point that is also a
// key statement).
#[rustfmt::skip]
static SHAPES: [Shape; 44] = [
    // --- Main points (section 12) ------------------------------------
    shape!(r"(?i)\bmy\s+(first|second|third|fourth|fifth)\s+point\b", SermonElementKind::MainPoint),
    shape!(r"(?i)\bpoint\s+number\s+(one|two|three|four|five|\d+)\b", SermonElementKind::MainPoint),
    shape!(r"(?i)\bthe\s+(first|second|third|fourth|fifth|next)\s+thing\s+is\b", SermonElementKind::MainPoint),
    shape!(r"(?i)\bthis\s+brings?\s+me\s+to\b", SermonElementKind::MainPoint),
    shape!(r"(?i)\banother\s+thing\b", SermonElementKind::MainPoint),

    // --- Sub-points (section 13) - distinct wording from main points so
    // the two never collide on the same trigger.
    shape!(r"(?i)\bunder\s+this\s+point\b", SermonElementKind::SubPoint),
    shape!(r"(?i)\bthere\s+are\s+(two|three|four|five|\d+)\s+things\b", SermonElementKind::SubPoint),
    shape!(r"(?i)\b(firstly|secondly|thirdly|fourthly)\b", SermonElementKind::SubPoint),

    // --- Definitions (section 18) --------------------------------------
    shape!(r"(?i)\bby\s+\w+\s+i\s+mean\b", SermonElementKind::Definition),
    shape!(r"(?i)\bthe\s+meaning\s+of\s+\w+(\s+\w+)?\s+is\b", SermonElementKind::Definition),
    shape!(r"(?i)\bthis\s+word\s+means\b", SermonElementKind::Definition),
    shape!(r"(?i)\bin\s+other\s+words\b", SermonElementKind::Definition),
    shape!(r"(?i)^\s*\w+\s+is\s+not\s+merely\b", SermonElementKind::Definition),

    // --- Key statements (section 19) -----------------------------------
    shape!(r"(?i)\byou\s+cannot\b", SermonElementKind::KeyStatement),
    shape!(r"(?i)\bthe\s+reason\b", SermonElementKind::KeyStatement),
    shape!(r"(?i)\bthis\s+is\s+important\b", SermonElementKind::KeyStatement),
    shape!(r"(?i)\bnever\s+forget\b", SermonElementKind::KeyStatement),
    shape!(r"(?i)\bthe\s+truth\s+is\b", SermonElementKind::KeyStatement),
    shape!(r"(?i)\bthe\s+key\s+is\b", SermonElementKind::KeyStatement),

    // --- Declarations (section 20) - deliberately excludes bare "you
    // will"/"God will": strong explicit language is required, not any
    // future-tense sentence.
    shape!(r"(?i)\bi\s+declare\b", SermonElementKind::Declaration),
    shape!(r"(?i)\breceive\s+(it|this|your)\b", SermonElementKind::Declaration),
    shape!(r"(?i)\bin\s+jesus.?\s+name\b", SermonElementKind::Declaration),

    // --- Illustrations / stories / examples (section 22) ---------------
    shape!(r"(?i)\blet\s+me\s+give\s+you\s+an\s+example\b", SermonElementKind::Example),
    shape!(r"(?i)\bfor\s+instance\b", SermonElementKind::Example),
    shape!(r"(?i)\bi\s+remember\s+when\b", SermonElementKind::Story),
    shape!(r"(?i)\bthere\s+(was|were)\s+a\s+(man|woman|boy|girl|person)\b", SermonElementKind::Story),
    shape!(r"(?i)\blet\s+me\s+tell\s+you\s+a\s+story\b", SermonElementKind::Story),
    shape!(r"(?i)\bimagine\b", SermonElementKind::Illustration),

    // --- Applications (section 23) --------------------------------------
    shape!(r"(?i)\bwhat\s+this\s+means\s+for\s+us\b", SermonElementKind::Application),
    shape!(r"(?i)\byou\s+need\s+to\b", SermonElementKind::Application),
    shape!(r"(?i)\bwe\s+must\b", SermonElementKind::Application),
    shape!(r"(?i)\btherefore\b", SermonElementKind::Application),
    shape!(r"(?i)\bthis\s+week\b", SermonElementKind::Application),
    shape!(r"(?i)\bput\s+this\s+into\s+practice\b", SermonElementKind::Application),

    // --- Prayer points (section 24) -------------------------------------
    shape!(r"(?i)\blet.?s\s+pray\b", SermonElementKind::PrayerPoint),
    shape!(r"(?i)\bpray\s+that\b", SermonElementKind::PrayerPoint),
    shape!(r"(?i)\bask\s+god\b", SermonElementKind::PrayerPoint),
    shape!(r"(?i)\bfather,?\s+help\s+us\b", SermonElementKind::PrayerPoint),

    // --- Summary / conclusion (section 27-28) ---------------------------
    shape!(r"(?i)\bto\s+summarize\b", SermonElementKind::Summary),
    shape!(r"(?i)\bin\s+conclusion\b", SermonElementKind::Conclusion),
    shape!(r"(?i)\bfinally\b", SermonElementKind::Conclusion),
    shape!(r"(?i)\blet\s+me\s+close\b", SermonElementKind::Conclusion),
    shape!(r"(?i)\bas\s+i\s+finish\b", SermonElementKind::Conclusion),
    shape!(r"(?i)\bone\s+last\s+thing\b", SermonElementKind::Conclusion),
];

/// A reflective/food-for-thought question shape (section 25) - fires only
/// in addition to, never instead of, the plain [`SermonElementKind::Question`]
/// detection below, since the reflection classification is an inference
/// about the question's purpose, not a replacement for the fact a question
/// was asked.
static REFLECTION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bwhat\s+would\b|\bhow\s+would\s+you\b").unwrap());

/// Literal quotation marks already present in the transcript - the only
/// source `ScriptureQuotation` ever draws from (never an invented
/// boundary, per section 17).
static QUOTATION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[\"\u{201c}]([^\"\u{201c}\u{201d}]{3,160})[\"\u{201d}]").unwrap());

/// Detect every sermon element present in one transcript segment. Pure and
/// deterministic: identical `text` always produces identical output, in
/// the same order (spec section 56).
pub fn detect_elements(text: &str) -> Vec<SermonDetection> {
    let mut detections = Vec::new();

    for shape in &SHAPES {
        if let Some(m) = shape.pattern.find(text) {
            detections.push(SermonDetection {
                kind: shape.kind,
                matched_phrase: m.as_str().to_string(),
                raw_text: text.to_string(),
                extracted_text: None,
            });
        }
    }

    // Questions: unambiguous, punctuation-anchored - the most reliable
    // detector in this module by construction.
    if text.contains('?') {
        detections.push(SermonDetection {
            kind: SermonElementKind::Question,
            matched_phrase: "?".to_string(),
            raw_text: text.to_string(),
            extracted_text: None,
        });
        if let Some(m) = REFLECTION_PATTERN.find(text) {
            detections.push(SermonDetection {
                kind: SermonElementKind::Reflection,
                matched_phrase: m.as_str().to_string(),
                raw_text: text.to_string(),
                extracted_text: None,
            });
        }
    }

    for captures in QUOTATION_PATTERN.captures_iter(text) {
        let quoted = captures.get(1).unwrap();
        detections.push(SermonDetection {
            kind: SermonElementKind::ScriptureQuotation,
            matched_phrase: captures.get(0).unwrap().as_str().to_string(),
            raw_text: text.to_string(),
            extracted_text: Some(quoted.as_str().to_string()),
        });
    }

    detections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<SermonElementKind> {
        detect_elements(text).into_iter().map(|d| d.kind).collect()
    }

    #[test]
    fn detects_an_explicit_main_point() {
        assert_eq!(
            kinds("My first point is that faith comes by hearing."),
            vec![SermonElementKind::MainPoint]
        );
    }

    #[test]
    fn detects_a_second_main_point_by_ordinal_thing_phrasing() {
        assert_eq!(
            kinds("The second thing is faith must be exercised."),
            vec![SermonElementKind::MainPoint]
        );
    }

    #[test]
    fn detects_a_sub_point() {
        assert_eq!(
            kinds("Under this point, there are two things to notice."),
            vec![SermonElementKind::SubPoint, SermonElementKind::SubPoint]
        );
    }

    #[test]
    fn plain_ordinal_words_alone_do_not_trigger_a_main_point() {
        // "First," alone (no "point"/"thing") never fires MainPoint.
        assert!(!kinds("First, let's begin.").contains(&SermonElementKind::MainPoint));
    }

    #[test]
    fn detects_a_definition() {
        let d = detect_elements("Faith is not merely believing; faith responds.");
        assert!(d.iter().any(|x| x.kind == SermonElementKind::Definition));
    }

    #[test]
    fn detects_a_key_statement() {
        assert_eq!(
            kinds("The truth is God is faithful."),
            vec![SermonElementKind::KeyStatement]
        );
    }

    #[test]
    fn bare_future_tense_is_never_a_declaration() {
        assert!(
            kinds("You will see God move this week.").is_empty()
                || !kinds("You will see God move this week.")
                    .contains(&SermonElementKind::Declaration)
        );
    }

    #[test]
    fn explicit_declaration_language_is_detected() {
        assert!(kinds("I declare that this house is blessed.")
            .contains(&SermonElementKind::Declaration));
        assert!(kinds("In Jesus' name, amen.").contains(&SermonElementKind::Declaration));
    }

    #[test]
    fn detects_a_story_illustration() {
        assert!(kinds("I remember when I was a young man.").contains(&SermonElementKind::Story));
        assert!(kinds("There was a man who had two sons.").contains(&SermonElementKind::Story));
    }

    #[test]
    fn detects_an_application() {
        assert!(kinds("This week, you need to act on what God has spoken.")
            .contains(&SermonElementKind::Application));
    }

    #[test]
    fn detects_a_prayer_point() {
        assert!(kinds("Let's pray together now.").contains(&SermonElementKind::PrayerPoint));
    }

    #[test]
    fn detects_a_conclusion_signal() {
        assert!(
            kinds("In conclusion, let us walk by faith.").contains(&SermonElementKind::Conclusion)
        );
    }

    #[test]
    fn detects_a_question_from_punctuation_alone() {
        assert!(kinds("Do you believe this today?").contains(&SermonElementKind::Question));
    }

    #[test]
    fn detects_a_reflection_question_alongside_the_plain_question() {
        let d = kinds("What would change in your life if you truly believed this?");
        assert!(d.contains(&SermonElementKind::Question));
        assert!(d.contains(&SermonElementKind::Reflection));
    }

    #[test]
    fn ordinary_statements_produce_no_reflection_without_a_question_mark() {
        assert!(
            !kinds("What would happen next was unclear to everyone in the room.")
                .contains(&SermonElementKind::Reflection)
        );
    }

    #[test]
    fn extracts_a_literal_quotation_verbatim() {
        let d = detect_elements(
            "Paul writes \u{201c}all things work together for good\u{201d} in Romans 8.",
        );
        let quotation = d
            .iter()
            .find(|x| x.kind == SermonElementKind::ScriptureQuotation)
            .expect("expected a quotation detection");
        assert_eq!(
            quotation.extracted_text.as_deref(),
            Some("all things work together for good")
        );
    }

    #[test]
    fn plain_prose_with_no_trigger_phrases_produces_no_detections() {
        assert!(detect_elements("Good morning church, it's wonderful to see you.").is_empty());
    }

    #[test]
    fn false_positive_thanksgiving_never_produces_a_theme_or_point_shaped_detection() {
        // "We thank God for today" has no structural trigger phrase at all.
        assert!(detect_elements("We thank God for today.").is_empty());
    }

    #[test]
    fn mentioning_faith_without_structural_language_produces_no_main_point() {
        assert!(!kinds("Faith is mentioned in the Bible many times.")
            .contains(&SermonElementKind::MainPoint));
    }

    #[test]
    fn identical_input_produces_identical_detections() {
        let text = "My first point is faith comes by hearing. Let me tell you a story.";
        assert_eq!(detect_elements(text), detect_elements(text));
    }
}
