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

    /// The Music Intelligence engine produced a new song-recognition
    /// finding (Phase 2.1) - never implies a presentation item was
    /// created; see `docs/music-intelligence.md`.
    MusicFindingDetected,
    /// The operator accepted a music finding (Phase 2.1) - a review
    /// decision only, still no presentation side effect.
    MusicFindingAccepted,
    MusicFindingRejected,

    /// The operator-confirmed "current song" changed (Phase 2.2) - set by
    /// `accept_music_finding`/`resolve_music_ambiguity` (operator
    /// acceptance) or cleared by `clear_current_song`. Never emitted for
    /// a merely-detected/candidate song - see `cip_core_music::CurrentSong`'s
    /// docs. Deliberately the only genuinely new Phase 2.2 event: acoustic
    /// findings reuse `MusicFindingDetected` (Phase 2.1's event already
    /// carries an `IntelligenceFinding` regardless of whether lyric or
    /// acoustic evidence produced it - see `docs/acoustic-music.md`'s
    /// "event reuse over event proliferation" note), so there is no
    /// separate acoustic-candidate or song-transition event.
    CurrentSongChanged,

    /// The Sermon Intelligence engine produced a new finding (Phase 2.3) -
    /// never implies anything was presented or auto-approved; see
    /// `docs/sermon-intelligence.md`.
    SermonFindingDetected,
    /// The operator accepted a sermon finding (Phase 2.3) - a review
    /// decision only, still no presentation side effect.
    SermonFindingAccepted,
    SermonFindingRejected,
    /// The sermon structure (a new main/sub-point recorded) changed
    /// (Phase 2.3) - never implies any point was rewritten; earlier
    /// points remain exactly as recorded.
    SermonStructureUpdated,
    /// The current theme candidate changed (Phase 2.3) - always
    /// `Inferred`; never claims certainty the evidence doesn't support.
    SermonThemeChanged,
    /// The lightweight derived sermon state changed (Phase 2.3) - a
    /// classification, never a rigid state machine transition.
    SermonStateChanged,

    /// The Phase 2.4 correlation engine produced a new cross-domain
    /// correlation - never implies anything was presented, approved, or
    /// projected; see `docs/cross-domain-intelligence.md`.
    CrossDomainCorrelationDetected,
    /// The operator reviewed a correlation without dismissing it (Phase
    /// 2.4) - informational only, mirrors `IntelligenceFinding::review`'s
    /// semantics.
    CrossDomainCorrelationReviewed,
    /// The operator explicitly dismissed a correlation (Phase 2.4) - never
    /// automatic, and never alters the source findings it was built from.
    CrossDomainCorrelationDismissed,

    /// The Service Intelligence engine detected a phase transition from
    /// transcript evidence (Phase 2.4, per the authoritative Phase 2
    /// roadmap - distinct from the correlation work above) - never implies
    /// anything was presented or auto-approved.
    ServicePhaseChanged,
    /// The operator explicitly marked or corrected the current phase -
    /// distinct from an automatically detected `ServicePhaseChanged`.
    ServicePhaseCorrected,
    /// An unexpected (backward) phase transition was flagged for operator
    /// review - never blocks the transition itself.
    ServiceAnomalyDetected,
    /// The operator acknowledged (accepted) an anomaly finding.
    ServiceAnomalyAcknowledged,

    /// A sermon became active (Phase 2.5, per the authoritative Phase 2
    /// roadmap) - distinct from the historical `SermonFindingDetected`
    /// etc. above, which belong to the earlier "Phase 2.3"-labeled
    /// semantic engine (`sermon.rs`). See `docs/sermon-foundation.md`.
    SermonStarted,
    SermonPaused,
    SermonResumed,
    SermonEnded,
    /// The operator explicitly assigned or changed the active sermon's
    /// current structural section - never inferred from transcript
    /// content in this phase.
    SermonSectionChanged,
    SermonSpeakerChanged,
    /// The operator supplied or corrected sermon metadata (currently:
    /// title) - `SermonSpeakerChanged` is emitted separately for speaker
    /// assignment, which carries its own richer payload shape.
    SermonMetadataChanged,
    /// An existing transcript segment was explicitly linked to the active
    /// sermon (`link_transcript_segment_to_sermon`) - never implies the
    /// transcript segment itself was created, modified, or reassigned
    /// silently.
    SermonSegmentLinked,
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

            AppEvent::MusicFindingDetected => "MUSIC_FINDING_DETECTED",
            AppEvent::MusicFindingAccepted => "MUSIC_FINDING_ACCEPTED",
            AppEvent::MusicFindingRejected => "MUSIC_FINDING_REJECTED",

            AppEvent::CurrentSongChanged => "CURRENT_SONG_CHANGED",

            AppEvent::SermonFindingDetected => "SERMON_FINDING_DETECTED",
            AppEvent::SermonFindingAccepted => "SERMON_FINDING_ACCEPTED",
            AppEvent::SermonFindingRejected => "SERMON_FINDING_REJECTED",
            AppEvent::SermonStructureUpdated => "SERMON_STRUCTURE_UPDATED",
            AppEvent::SermonThemeChanged => "SERMON_THEME_CHANGED",
            AppEvent::SermonStateChanged => "SERMON_STATE_CHANGED",

            AppEvent::CrossDomainCorrelationDetected => "CROSS_DOMAIN_CORRELATION_DETECTED",
            AppEvent::CrossDomainCorrelationReviewed => "CROSS_DOMAIN_CORRELATION_REVIEWED",
            AppEvent::CrossDomainCorrelationDismissed => "CROSS_DOMAIN_CORRELATION_DISMISSED",

            AppEvent::ServicePhaseChanged => "SERVICE_PHASE_CHANGED",
            AppEvent::ServicePhaseCorrected => "SERVICE_PHASE_CORRECTED",
            AppEvent::ServiceAnomalyDetected => "SERVICE_ANOMALY_DETECTED",
            AppEvent::ServiceAnomalyAcknowledged => "SERVICE_ANOMALY_ACKNOWLEDGED",

            AppEvent::SermonStarted => "SERMON_STARTED",
            AppEvent::SermonPaused => "SERMON_PAUSED",
            AppEvent::SermonResumed => "SERMON_RESUMED",
            AppEvent::SermonEnded => "SERMON_ENDED",
            AppEvent::SermonSectionChanged => "SERMON_SECTION_CHANGED",
            AppEvent::SermonSpeakerChanged => "SERMON_SPEAKER_CHANGED",
            AppEvent::SermonMetadataChanged => "SERMON_METADATA_CHANGED",
            AppEvent::SermonSegmentLinked => "SERMON_SEGMENT_LINKED",
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
            AppEvent::MusicFindingDetected,
            AppEvent::MusicFindingAccepted,
            AppEvent::MusicFindingRejected,
            AppEvent::CurrentSongChanged,
            AppEvent::SermonFindingDetected,
            AppEvent::SermonFindingAccepted,
            AppEvent::SermonFindingRejected,
            AppEvent::SermonStructureUpdated,
            AppEvent::SermonThemeChanged,
            AppEvent::SermonStateChanged,
            AppEvent::CrossDomainCorrelationDetected,
            AppEvent::CrossDomainCorrelationReviewed,
            AppEvent::CrossDomainCorrelationDismissed,
            AppEvent::ServicePhaseChanged,
            AppEvent::ServicePhaseCorrected,
            AppEvent::ServiceAnomalyDetected,
            AppEvent::ServiceAnomalyAcknowledged,
            AppEvent::SermonStarted,
            AppEvent::SermonPaused,
            AppEvent::SermonResumed,
            AppEvent::SermonEnded,
            AppEvent::SermonSectionChanged,
            AppEvent::SermonSpeakerChanged,
            AppEvent::SermonMetadataChanged,
            AppEvent::SermonSegmentLinked,
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
