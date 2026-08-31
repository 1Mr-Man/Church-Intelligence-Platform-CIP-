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

  PresentationPreviewed: "PRESENTATION_PREVIEWED",
  PresentationPrepared: "PRESENTATION_PREPARED",
  PresentationStarted: "PRESENTATION_STARTED",
  PresentationStopped: "PRESENTATION_STOPPED",
  PresentationCancelled: "PRESENTATION_CANCELLED",
  /** Phase 3.10.3: a specific screen was just switched from `held` back
   * to `live` and should re-pull current state - always targeted at one
   * display window (`emit_to`), never broadcast. No payload; the
   * receiving window re-fetches via `getPresentationDisplayState`, the
   * same hydration path it already uses on mount. */
  PresentationScreenSynced: "PRESENTATION_SCREEN_SYNCED",

  ServiceStarted: "SERVICE_STARTED",
  ServicePaused: "SERVICE_PAUSED",
  ServiceResumed: "SERVICE_RESUMED",
  ServiceEnded: "SERVICE_ENDED",

  SpeechStarted: "SPEECH_STARTED",
  SpeechStopped: "SPEECH_STOPPED",

  ErrorOccurred: "ERROR_OCCURRED",

  ScriptureContextCorrected: "SCRIPTURE_CONTEXT_CORRECTED",
  ScriptureAmbiguousResolved: "SCRIPTURE_AMBIGUOUS_RESOLVED",

  MusicFindingDetected: "MUSIC_FINDING_DETECTED",
  MusicFindingAccepted: "MUSIC_FINDING_ACCEPTED",
  MusicFindingRejected: "MUSIC_FINDING_REJECTED",

  /** The operator-confirmed "current song" changed (Phase 2.2) - payload
   * is `CurrentSong | null` (`null` when cleared). */
  CurrentSongChanged: "CURRENT_SONG_CHANGED",

  SermonFindingDetected: "SERMON_FINDING_DETECTED",
  SermonFindingAccepted: "SERMON_FINDING_ACCEPTED",
  SermonFindingRejected: "SERMON_FINDING_REJECTED",
  /** Payload is the updated `SermonPoint[]` (Phase 2.3). */
  SermonStructureUpdated: "SERMON_STRUCTURE_UPDATED",
  /** Payload is `ThemeCandidate | null` (Phase 2.3). */
  SermonThemeChanged: "SERMON_THEME_CHANGED",
  /** Payload is the new `SermonState` (Phase 2.3). */
  SermonStateChanged: "SERMON_STATE_CHANGED",

  /** Payload is the new `IntelligenceCorrelation` (Phase 2.4). */
  CrossDomainCorrelationDetected: "CROSS_DOMAIN_CORRELATION_DETECTED",
  CrossDomainCorrelationReviewed: "CROSS_DOMAIN_CORRELATION_REVIEWED",
  CrossDomainCorrelationDismissed: "CROSS_DOMAIN_CORRELATION_DISMISSED",

  /** Payload is the updated `IntelligenceFinding` (Phase 2.4, Service
   * Intelligence per the authoritative Phase 2 roadmap). */
  ServicePhaseChanged: "SERVICE_PHASE_CHANGED",
  ServicePhaseCorrected: "SERVICE_PHASE_CORRECTED",
  ServiceAnomalyDetected: "SERVICE_ANOMALY_DETECTED",
  ServiceAnomalyAcknowledged: "SERVICE_ANOMALY_ACKNOWLEDGED",

  /** Payload is a `Sermon` (Phase 2.5, per the authoritative Phase 2
   * roadmap) - distinct from the `SermonFinding*`/`SermonStructureUpdated`/
   * `SermonThemeChanged`/`SermonStateChanged` events above, which belong
   * to the earlier "Phase 2.3"-labeled semantic engine. */
  SermonStarted: "SERMON_STARTED",
  SermonPaused: "SERMON_PAUSED",
  SermonResumed: "SERMON_RESUMED",
  SermonEnded: "SERMON_ENDED",
  /** Payload is a `SermonSection` (Phase 2.5). */
  SermonSectionChanged: "SERMON_SECTION_CHANGED",
  /** Payload is a `Sermon` (Phase 2.5). */
  SermonSpeakerChanged: "SERMON_SPEAKER_CHANGED",
  SermonMetadataChanged: "SERMON_METADATA_CHANGED",
  /** Payload is a `SermonSegment` (Phase 2.5). */
  SermonSegmentLinked: "SERMON_SEGMENT_LINKED",
  /** Payload is a `SermonHarvest` (Phase 3.9) - a read-only aggregation,
   * never implies any new detection ran or any data changed. */
  SermonHarvested: "SERMON_HARVESTED",

  /** Payload is the new/updated `ContentCandidate` (Phase 2.7, per the
   * authoritative Phase 2 roadmap) - never a publish/schedule/presentation
   * event. */
  ContentCandidateDetected: "CONTENT_CANDIDATE_DETECTED",
  ContentCandidateAccepted: "CONTENT_CANDIDATE_ACCEPTED",
  ContentCandidateRejected: "CONTENT_CANDIDATE_REJECTED",
} as const;

export type AppEventName = (typeof AppEvents)[keyof typeof AppEvents];
