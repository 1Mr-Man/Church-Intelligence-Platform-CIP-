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

    /// A non-mutating preview render was produced (Phase 1.4) - never
    /// implies anything was persisted or prepared.
    PresentationPreviewed,
    PresentationPrepared,
    PresentationStarted,
    PresentationStopped,
    /// A prepared item was cancelled/retracted before being displayed
    /// (Phase 1.4).
    PresentationCancelled,

    ServiceStarted,
    ServicePaused,
    ServiceResumed,
    ServiceEnded,

    /// Emitted alongside `AudioStarted`/`AudioStopped` - in this
    /// architecture speech processing is driven entirely by audio chunks
    /// arriving (see `commands::handle_audio_chunk`), so "speech started"
    /// and "speech stopped" are the same real transition as audio
    /// capture starting/stopping, not a separately observable engine
    /// state. See `docs/live-service.md`.
    SpeechStarted,
    SpeechStopped,

    /// A recorded, non-fatal failure (audio device lost, speech engine
    /// error, persistence failure) - never emitted for routine input
    /// validation errors, only for the failure-recovery scenarios
    /// Phase 1.3 requires the operator be able to see. See
    /// `docs/live-service.md`'s recovery section.
    ErrorOccurred,

    /// The operator manually corrected the active Scripture context
    /// (Phase 1.3) - distinct from an automatic `ScriptureUpdated`.
    ScriptureContextCorrected,
    /// The operator resolved an `Ambiguous` detection by choosing one of
    /// the offered candidates (Phase 1.3).
    ScriptureAmbiguousResolved,
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

            AppEvent::PresentationPreviewed => "PRESENTATION_PREVIEWED",
            AppEvent::PresentationPrepared => "PRESENTATION_PREPARED",
            AppEvent::PresentationStarted => "PRESENTATION_STARTED",
            AppEvent::PresentationStopped => "PRESENTATION_STOPPED",
            AppEvent::PresentationCancelled => "PRESENTATION_CANCELLED",

            AppEvent::ServiceStarted => "SERVICE_STARTED",
            AppEvent::ServicePaused => "SERVICE_PAUSED",
            AppEvent::ServiceResumed => "SERVICE_RESUMED",
            AppEvent::ServiceEnded => "SERVICE_ENDED",

            AppEvent::SpeechStarted => "SPEECH_STARTED",
            AppEvent::SpeechStopped => "SPEECH_STOPPED",

            AppEvent::ErrorOccurred => "ERROR_OCCURRED",

            AppEvent::ScriptureContextCorrected => "SCRIPTURE_CONTEXT_CORRECTED",
            AppEvent::ScriptureAmbiguousResolved => "SCRIPTURE_AMBIGUOUS_RESOLVED",
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
            AppEvent::PresentationPreviewed,
            AppEvent::PresentationPrepared,
            AppEvent::PresentationStarted,
            AppEvent::PresentationStopped,
            AppEvent::PresentationCancelled,
            AppEvent::ServiceStarted,
            AppEvent::ServicePaused,
            AppEvent::ServiceResumed,
            AppEvent::ServiceEnded,
            AppEvent::SpeechStarted,
            AppEvent::SpeechStopped,
            AppEvent::ErrorOccurred,
            AppEvent::ScriptureContextCorrected,
            AppEvent::ScriptureAmbiguousResolved,
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
