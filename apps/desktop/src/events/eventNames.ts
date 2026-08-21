/**
 * Event architecture foundation - frontend mirror of
 * `apps/desktop/src-tauri/src/events.rs`.
 *
 * CIP uses Tauri's built-in event system as its event bus: the backend
 * emits through `AppHandle::emit`, the frontend subscribes through
 * `@tauri-apps/api/event`'s `listen`. This module is only the single typed
 * source of truth for event *names*, kept in sync by hand with the Rust
 * `AppEvent` enum (both sides are small enough that generating this file is
 * unnecessary complexity for Phase 1).
 */

export const AppEvents = {
  AudioStarted: "AUDIO_STARTED",
  AudioStopped: "AUDIO_STOPPED",
  TranscriptUpdated: "TRANSCRIPT_UPDATED",

  ScriptureDetected: "SCRIPTURE_DETECTED",
  ScriptureUpdated: "SCRIPTURE_UPDATED",
  ScriptureConfirmed: "SCRIPTURE_CONFIRMED",
  ScriptureRejected: "SCRIPTURE_REJECTED",

  SuggestionCreated: "SUGGESTION_CREATED",
  SuggestionApproved: "SUGGESTION_APPROVED",
  SuggestionEdited: "SUGGESTION_EDITED",
  SuggestionRejected: "SUGGESTION_REJECTED",

  PresentationPrepared: "PRESENTATION_PREPARED",
  PresentationStarted: "PRESENTATION_STARTED",
  PresentationStopped: "PRESENTATION_STOPPED",

  ServiceStarted: "SERVICE_STARTED",
  ServicePaused: "SERVICE_PAUSED",
  ServiceResumed: "SERVICE_RESUMED",
  ServiceEnded: "SERVICE_ENDED",

  SpeechStarted: "SPEECH_STARTED",
  SpeechStopped: "SPEECH_STOPPED",

  ErrorOccurred: "ERROR_OCCURRED",

  ScriptureContextCorrected: "SCRIPTURE_CONTEXT_CORRECTED",
  ScriptureAmbiguousResolved: "SCRIPTURE_AMBIGUOUS_RESOLVED",
} as const;

export type AppEventName = (typeof AppEvents)[keyof typeof AppEvents];
