//! Sermon Foundation (Phase 2.5, per the authoritative Phase 2 roadmap):
//! the durable entity/lifecycle/structural layer Phase 2.6's real Sermon
//! Intelligence engine will operate on.
//!
//! ## Foundation vs. the sibling modules in this crate
//!
//! Everything else in `cip_core_sermon` (`detection`, `state`, `structure`,
//! `taxonomy`, `theme`) is semantic/structural-meaning detection over
//! transcript *text* - "what sermon-shaped things are present in this
//! text," built under this repository's earlier internal "Phase 2.3"
//! label. Under the authoritative Phase 2 roadmap that label is now
//! understood to be Phase 2.6-equivalent work; nothing there is modified,
//! renamed, or relocated to make room for this module (see
//! `docs/sermon-foundation.md`'s "Roadmap note").
//!
//! This module answers a completely different, prior question: "what *is*
//! a sermon, as a durable thing with an identity, a lifecycle, a speaker,
//! and a set of transcript segments/sections belonging to it?" It never
//! reads transcript *content* to decide anything - every fact here is
//! either an explicit operator action or a deterministic structural
//! association (a transcript segment id, a timestamp, a status
//! transition). It never claims a theme, a doctrine, a main point, or any
//! other semantic meaning - see each type's own docs.
//!
//! ## No dependency on `core/intelligence`, persistence, or Tauri
//!
//! Exactly like every other module in this crate, `foundation` is pure
//! domain logic: no `IntelligenceFinding`, no `EvidenceSource`, no SQL, no
//! `AppHandle`. The Tauri orchestration layer
//! (`apps/desktop/src-tauri/src/sermon_foundation.rs`) is where these
//! plain values become `IntelligenceFinding`s (reusing the existing
//! `EvidenceSource` variants - `OperatorAction`, `ServiceEvent`,
//! `Transcript` - rather than this crate inventing a second evidence
//! system) and get persisted.

pub mod section;
pub mod segment;
pub mod sermon;
pub mod speaker;

pub use section::{SectionOrigin, SermonSection, SermonSectionKind};
pub use segment::SermonSegment;
pub use sermon::{is_valid_transition, Sermon, SermonStatus};
pub use speaker::{Speaker, SpeakerRole};
