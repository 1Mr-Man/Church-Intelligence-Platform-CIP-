//! Presentation renderer contract.
//!
//! Kept as a separate top-level `presentation/` subsystem (rather than
//! living inside `core/presentation`) so the AI/suggestion pipeline that
//! produces a `PresentationItem` never couples directly to how it ends up
//! on screen - a future NDI/window/OBS-output renderer plugs in here
//! without `core` changing. Phase 1 ships only [`NullRenderer`], enough to
//! prove the contract compiles and is wireable; the full presentation
//! designer is explicitly out of scope.

use cip_core_presentation::PresentationItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    Unsupported(String),
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
}
