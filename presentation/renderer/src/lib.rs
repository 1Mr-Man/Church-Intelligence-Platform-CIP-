//! Presentation renderer contract.
//!
//! Kept as a separate top-level `presentation/` subsystem (rather than
//! living inside `core/presentation`) so the AI/suggestion pipeline that
//! produces a `PresentationItem` never couples directly to how it ends up
//! on screen - a future NDI/window/OBS-output renderer plugs in here
//! without `core` changing. Phase 1 ships only [`NullRenderer`], enough to
//! prove the contract compiles and is wireable; the full presentation
//! designer is explicitly out of scope.

use cip_core_presentation::{PresentationContent, PresentationItem};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderError {
    #[error("unsupported presentation content: {0}")]
    Unsupported(String),
    /// The content is missing a required field, or a field is malformed
    /// (e.g. an empty reference or verse text) - the renderer refuses to
    /// produce output for it rather than rendering something broken.
    #[error("invalid presentation content: {0}")]
    InvalidContent(String),
}

/// The one deterministic template Phase 1.4 ships: reference, verse text,
/// and translation, laid out with predictable margins. See
/// [`render_content`].
pub const SCRIPTURE_DEFAULT_TEMPLATE: &str = "SCRIPTURE_DEFAULT";

/// The minimal template used for a plain text/title slide - no visual
/// design work has gone into this, it exists only so [`render_content`]
/// is total over every [`PresentationContent`] variant.
pub const TEXT_DEFAULT_TEMPLATE: &str = "TEXT_DEFAULT";

/// Safe-margin line width (in characters) used by the deterministic
/// word-wrap in [`render_content`]. Chosen to be a conservative, readable
/// width for a text-based slide - not tied to any specific screen
/// resolution, since Phase 1.4 proves the render pipeline, not final
/// visual design.
const WRAP_WIDTH: usize = 42;

/// A rendered slide: the deterministic, structured output of
/// [`render_content`]. Contains no styling - it's the renderer's job (a
/// future GUI/window/NDI backend) to turn this into pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedSlide {
    pub template: String,
    pub heading: String,
    pub body_lines: Vec<String>,
    pub footer: Option<String>,
}

/// Deterministically renders a [`PresentationContent`] into a
/// [`RenderedSlide`]: no AI generation, no randomness, no network access -
/// the same content always produces the same slide. Rejects content that's
/// missing a required field rather than producing broken output.
pub fn render_content(content: &PresentationContent) -> Result<RenderedSlide, RenderError> {
    match content {
        PresentationContent::Scripture {
            reference,
            translation_id,
            text,
        } => {
            if reference.trim().is_empty() {
                return Err(RenderError::InvalidContent(
                    "scripture reference is empty".to_string(),
                ));
            }
            if translation_id.trim().is_empty() {
                return Err(RenderError::InvalidContent(
                    "translation id is empty".to_string(),
                ));
            }
            if text.trim().is_empty() {
                return Err(RenderError::InvalidContent(
                    "verse text is empty".to_string(),
                ));
            }
            Ok(RenderedSlide {
                template: SCRIPTURE_DEFAULT_TEMPLATE.to_string(),
                heading: reference.clone(),
                body_lines: word_wrap(text, WRAP_WIDTH),
                footer: Some(translation_id.clone()),
            })
        }
        PresentationContent::Text { title, body } => {
            if body.trim().is_empty() {
                return Err(RenderError::InvalidContent(
                    "text body is empty".to_string(),
                ));
            }
            Ok(RenderedSlide {
                template: TEXT_DEFAULT_TEMPLATE.to_string(),
                heading: title.clone().unwrap_or_default(),
                body_lines: word_wrap(body, WRAP_WIDTH),
                footer: None,
            })
        }
        _ => Err(RenderError::Unsupported(
            "unsupported presentation content variant".to_string(),
        )),
    }
}

/// Deterministic greedy word-wrap: splits on whitespace and packs words
/// onto a line up to `width` characters, never mid-word. Pure function of
/// its inputs - same text and width always produce the same lines.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Turns a [`PresentationItem`] into on-screen output. What "rendering"
/// means (a window, an NDI feed, a browser-source overlay) is entirely up
/// to the implementation.
pub trait Renderer: Send + Sync {
    fn render(&mut self, item: &PresentationItem) -> Result<(), RenderError>;
    fn clear(&mut self) -> Result<(), RenderError>;
}

/// A renderer that accepts items but produces no output. Used to prove the
/// `Renderer` trait is wireable end to end before a real rendering backend
/// exists.
#[derive(Default)]
pub struct NullRenderer {
    pub last_rendered: Option<PresentationItem>,
}

impl Renderer for NullRenderer {
    fn render(&mut self, item: &PresentationItem) -> Result<(), RenderError> {
        self.last_rendered = Some(item.clone());
        Ok(())
    }

    fn clear(&mut self) -> Result<(), RenderError> {
        self.last_rendered = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cip_core_presentation::PresentationContent;
    use uuid::Uuid;

    #[test]
    fn null_renderer_tracks_the_last_rendered_item() {
        let mut renderer = NullRenderer::default();
        let item = PresentationItem::prepare(
            Uuid::new_v4(),
            PresentationContent::Text {
                title: None,
                body: "Welcome".into(),
            },
        );
        renderer.render(&item).unwrap();
        assert_eq!(renderer.last_rendered, Some(item));

        renderer.clear().unwrap();
        assert!(renderer.last_rendered.is_none());
    }

    fn romans_8_28() -> PresentationContent {
        PresentationContent::Scripture {
            reference: "Romans 8:28".into(),
            translation_id: "KJV".into(),
            text: "And we know that all things work together for good to them that love God, \
                   to them who are the called according to his purpose."
                .into(),
        }
    }

    #[test]
    fn renders_scripture_content_with_the_default_template() {
        let slide = render_content(&romans_8_28()).unwrap();
        assert_eq!(slide.template, SCRIPTURE_DEFAULT_TEMPLATE);
        assert_eq!(slide.heading, "Romans 8:28");
        assert_eq!(slide.footer.as_deref(), Some("KJV"));
        assert!(!slide.body_lines.is_empty());
        for line in &slide.body_lines {
            assert!(
                line.len() <= WRAP_WIDTH,
                "line exceeds safe margin: {line:?}"
            );
        }
        assert_eq!(slide.body_lines.join(" "), "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.");
    }

    #[test]
    fn render_content_is_deterministic() {
        let content = romans_8_28();
        assert_eq!(render_content(&content), render_content(&content));
    }

    #[test]
    fn rejects_scripture_with_empty_reference() {
        let content = PresentationContent::Scripture {
            reference: "".into(),
            translation_id: "KJV".into(),
            text: "some text".into(),
        };
        assert_eq!(
            render_content(&content),
            Err(RenderError::InvalidContent(
                "scripture reference is empty".to_string()
            ))
        );
    }

    #[test]
    fn rejects_scripture_with_empty_translation() {
        let content = PresentationContent::Scripture {
            reference: "Romans 8:28".into(),
            translation_id: "   ".into(),
            text: "some text".into(),
        };
        assert_eq!(
            render_content(&content),
            Err(RenderError::InvalidContent(
                "translation id is empty".to_string()
            ))
        );
    }

    #[test]
    fn rejects_scripture_with_empty_verse_text() {
        let content = PresentationContent::Scripture {
            reference: "Romans 8:28".into(),
            translation_id: "KJV".into(),
            text: "".into(),
        };
        assert_eq!(
            render_content(&content),
            Err(RenderError::InvalidContent(
                "verse text is empty".to_string()
            ))
        );
    }

    #[test]
    fn rejects_text_content_with_empty_body() {
        let content = PresentationContent::Text {
            title: Some("Welcome".into()),
            body: "".into(),
        };
        assert_eq!(
            render_content(&content),
            Err(RenderError::InvalidContent(
                "text body is empty".to_string()
            ))
        );
    }

    #[test]
    fn renders_text_content_with_the_text_template() {
        let content = PresentationContent::Text {
            title: None,
            body: "Welcome to service".into(),
        };
        let slide = render_content(&content).unwrap();
        assert_eq!(slide.template, TEXT_DEFAULT_TEMPLATE);
        assert_eq!(slide.heading, "");
        assert_eq!(slide.body_lines, vec!["Welcome to service".to_string()]);
    }

    #[test]
    fn word_wrap_never_splits_a_word() {
        let lines = word_wrap("a supercalifragilisticexpialidocious word", 10);
        for line in &lines {
            assert!(!line.is_empty());
        }
        assert_eq!(lines.join(" "), "a supercalifragilisticexpialidocious word");
    }
}
