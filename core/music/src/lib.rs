//! Music domain - placeholder for Phase 1.
//!
//! This crate exists to hold the `core/music` architectural boundary in
//! place. Song/lyric recognition (`integrations/music`) is explicitly out
//! of scope for the Phase 1 foundation; no domain logic lives here yet.

use serde::{Deserialize, Serialize};

/// Minimal identity marker so downstream code (and tests) can reference
/// "the music domain" before any real domain type exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicDomainPlaceholder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_type_is_constructible() {
        let _ = MusicDomainPlaceholder;
    }
}
