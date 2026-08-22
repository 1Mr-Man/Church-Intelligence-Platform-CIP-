//! Sermon domain (Phase 2.3): pure, deterministic sermon structural/
//! meaning detection with no dependency on `core/intelligence`,
//! persistence, or Tauri - see `docs/sermon-intelligence.md`.
//!
//! This crate answers "what sermon-shaped things are present in this
//! text" - it never assigns an [`AssertionLevel`]-equivalent epistemic
//! label, confidence score, or `IntelligenceFinding`; that translation is
//! `core/intelligence::sermon_adapter`'s job (mirroring `core/bible`'s
//! split from `core/intelligence::bible_adapter`). Dependency direction is
//! one-way: `core/intelligence` depends on this crate, never the reverse,
//! and this crate never depends on any other domain crate (spec's
//! cross-domain dependency rule).

pub mod detection;
pub mod state;
pub mod structure;
pub mod taxonomy;
pub mod theme;

pub use detection::{detect_elements, SermonDetection};
pub use state::{infer_state, SermonState};
pub use structure::{SermonPoint, SermonStructure, SermonSubPoint};
pub use taxonomy::SermonElementKind;
pub use theme::{ThemeCandidate, ThemeTracker};
