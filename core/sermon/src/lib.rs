//! Sermon domain: pure, deterministic sermon logic with no dependency on
//! `core/intelligence`, persistence, or Tauri - two related but distinct
//! halves under one crate:
//!
//! - `detection`/`state`/`structure`/`taxonomy`/`theme` (built under this
//!   repository's earlier internal "Phase 2.3" label - see
//!   `docs/sermon-intelligence.md`): "what sermon-shaped *meaning* things
//!   are present in this text" - theme, main points, illustrations, and
//!   so on. Under this repository's authoritative Phase 2 roadmap this is
//!   understood to be Phase 2.6-equivalent semantic work; nothing here is
//!   modified to make room for `foundation` below.
//! - [`foundation`] (Phase 2.5, per the authoritative Phase 2 roadmap):
//!   "what *is* a sermon" - its identity, lifecycle, speaker, sections,
//!   and transcript-segment linkage, independent of any semantic claim
//!   about what was said. See `docs/sermon-foundation.md`.
//!
//! Neither half assigns an [`AssertionLevel`]-equivalent epistemic label,
//! confidence score, or `IntelligenceFinding` on its own; that translation
//! is `core/intelligence`'s job (`sermon_adapter` for the semantic half,
//! the Tauri `sermon_foundation` orchestration module for this one),
//! mirroring `core/bible`'s split from `core/intelligence::bible_adapter`.
//! Dependency direction is one-way: `core/intelligence` depends on this
//! crate, never the reverse, and this crate never depends on any other
//! domain crate (spec's cross-domain dependency rule).

pub mod detection;
pub mod foundation;
pub mod state;
pub mod structure;
pub mod taxonomy;
pub mod theme;

pub use detection::{detect_elements, SermonDetection};
pub use state::{candidate_section_for_state, infer_state, SermonState};
pub use structure::{SermonPoint, SermonStructure, SermonSubPoint};
pub use taxonomy::SermonElementKind;
pub use theme::{ThemeCandidate, ThemeTracker};
