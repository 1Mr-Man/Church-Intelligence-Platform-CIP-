//! Logging categories.
//!
//! CIP logs through the standard `log` crate (wired to stdout + a rotating
//! file via `tauri-plugin-log` in `lib.rs`), but every call site should log
//! against one of the categories below rather than the crate's default
//! target, so log output can be filtered/routed by subsystem. A category is
//! just a `log` target string (`"cip::database"`, etc.) - this is
//! deliberately not a custom logging framework, just a naming convention
//! enforced by a typed enum instead of raw strings.
//!
//! ```
//! log::info!(target: cip_desktop_lib::logging::LogCategory::Database.target(), "migrations applied");
//! ```

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCategory {
    App,
    Database,
    Audio,
    Speech,
    Bible,
    Ai,
    Presentation,
    /// Content Registry / dataset import operations (Phase 1.5).
    Content,
    /// Music Intelligence (Phase 2.1): recognition analysis, dataset
    /// import, and operator finding review.
    Music,
    Network,
    Security,
    /// For errors that don't cleanly belong to one of the above (see
    /// `errors::AppError::category`, which prefers a specific category
    /// when one applies).
    Error,
}

impl LogCategory {
    pub const fn target(self) -> &'static str {
        match self {
            LogCategory::App => "cip::app",
            LogCategory::Database => "cip::database",
            LogCategory::Audio => "cip::audio",
            LogCategory::Speech => "cip::speech",
            LogCategory::Bible => "cip::bible",
            LogCategory::Ai => "cip::ai",
            LogCategory::Presentation => "cip::presentation",
            LogCategory::Content => "cip::content",
            LogCategory::Music => "cip::music",
            LogCategory::Network => "cip::network",
            LogCategory::Security => "cip::security",
            LogCategory::Error => "cip::error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_a_distinct_target() {
        let categories = [
            LogCategory::App,
            LogCategory::Database,
            LogCategory::Audio,
            LogCategory::Speech,
            LogCategory::Bible,
            LogCategory::Ai,
            LogCategory::Presentation,
            LogCategory::Content,
            LogCategory::Music,
            LogCategory::Network,
            LogCategory::Security,
            LogCategory::Error,
        ];
        let mut targets: Vec<&str> = categories.iter().map(|c| c.target()).collect();
        let unique_count_before = targets.len();
        targets.sort_unstable();
        targets.dedup();
        assert_eq!(targets.len(), unique_count_before);
    }
}
