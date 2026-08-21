//! Event architecture foundation.
//!
//! CIP uses Tauri's built-in event system as its event bus rather than a
//! bespoke pub/sub implementation - `AppHandle::emit` already gives every
//! backend event a subscriber-agnostic broadcast to the frontend (and
//! `@tauri-apps/api/event`'s `listen` on the frontend side), which is all
//! Phase 1 needs. This module's only job is to give every event name a
//! single typed source of truth instead of scattering string literals,
//! mirrored on the frontend in `src/events/eventNames.ts`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    AudioStarted,
    AudioStopped,
    TranscriptUpdated,

    ScriptureDetected,
    ScriptureUpdated,
    ScriptureConfirmed,
    ScriptureRejected,

    SuggestionCreated,
    SuggestionApproved,
    SuggestionEdited,
    SuggestionRejected,

    PresentationPrepared,
    PresentationStarted,
    PresentationStopped,

    ServiceStarted,
    ServicePaused,
    ServiceEnded,
}

impl AppEvent {
    pub const fn name(self) -> &'static str {
        match self {
            AppEvent::AudioStarted => "AUDIO_STARTED",
            AppEvent::AudioStopped => "AUDIO_STOPPED",
            AppEvent::TranscriptUpdated => "TRANSCRIPT_UPDATED",

            AppEvent::ScriptureDetected => "SCRIPTURE_DETECTED",
            AppEvent::ScriptureUpdated => "SCRIPTURE_UPDATED",
            AppEvent::ScriptureConfirmed => "SCRIPTURE_CONFIRMED",
            AppEvent::ScriptureRejected => "SCRIPTURE_REJECTED",

            AppEvent::SuggestionCreated => "SUGGESTION_CREATED",
            AppEvent::SuggestionApproved => "SUGGESTION_APPROVED",
            AppEvent::SuggestionEdited => "SUGGESTION_EDITED",
            AppEvent::SuggestionRejected => "SUGGESTION_REJECTED",

            AppEvent::PresentationPrepared => "PRESENTATION_PREPARED",
            AppEvent::PresentationStarted => "PRESENTATION_STARTED",
            AppEvent::PresentationStopped => "PRESENTATION_STOPPED",

            AppEvent::ServiceStarted => "SERVICE_STARTED",
            AppEvent::ServicePaused => "SERVICE_PAUSED",
            AppEvent::ServiceEnded => "SERVICE_ENDED",
        }
    }
}

/// Emit an [`AppEvent`] with a serializable payload to every listening
/// webview. Thin wrapper over `tauri::Emitter::emit` so call sites use the
/// typed enum instead of a string literal.
pub fn emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: AppEvent,
    payload: impl Serialize + Clone,
) -> tauri::Result<()> {
    use tauri::Emitter;
    app.emit(event.name(), payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_name_is_distinct_and_screaming_snake_case() {
        let events = [
            AppEvent::AudioStarted,
            AppEvent::AudioStopped,
            AppEvent::TranscriptUpdated,
            AppEvent::ScriptureDetected,
            AppEvent::ScriptureUpdated,
            AppEvent::ScriptureConfirmed,
            AppEvent::ScriptureRejected,
            AppEvent::SuggestionCreated,
            AppEvent::SuggestionApproved,
            AppEvent::SuggestionEdited,
            AppEvent::SuggestionRejected,
            AppEvent::PresentationPrepared,
            AppEvent::PresentationStarted,
            AppEvent::PresentationStopped,
            AppEvent::ServiceStarted,
            AppEvent::ServicePaused,
            AppEvent::ServiceEnded,
        ];
        let mut names: Vec<&str> = events.iter().map(|e| e.name()).collect();
        let unique_before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), unique_before, "event names must be unique");

        for name in names {
            assert_eq!(
                name,
                name.to_uppercase(),
                "event names must be SCREAMING_SNAKE_CASE"
            );
            assert!(
                name.contains('_'),
                "event names must have at least one underscore: {name}"
            );
        }
    }
}
