import type { TimelineEntry } from "../domain";

/**
 * Turns one {@link TimelineEntry} into the human-readable line the
 * service timeline shows (Phase 1.3 section 8, e.g. "Romans 8:28
 * suggested - confidence 98%"). The backend deliberately stores only
 * `eventName` + a small `payload` (see `timeline.rs`) rather than a
 * pre-formatted description, so this is the one place that owns the
 * mapping - kept as a pure function so it's testable without a live
 * event stream.
 */
export function describeTimelineEntry(entry: TimelineEntry): string {
  const payload = entry.payload ?? {};
  const str = (key: string): string | undefined => {
    const value = payload[key];
    return typeof value === "string" ? value : undefined;
  };
  const num = (key: string): number | undefined => {
    const value = payload[key];
    return typeof value === "number" ? value : undefined;
  };
  const kindReference = (key: string): string | undefined => {
    const value = payload[key];
    if (value && typeof value === "object" && "reference" in value) {
      const reference = (value as { reference?: unknown }).reference;
      return typeof reference === "string" ? reference : undefined;
    }
    return undefined;
  };
  const confidencePercent = (): string => {
    const confidence = num("confidence");
    return confidence === undefined ? "" : ` - confidence ${Math.round(confidence * 100)}%`;
  };

  switch (entry.eventName) {
    case "SERVICE_STARTED":
      return `Service started${str("title") ? ` - ${str("title")}` : ""}`;
    case "SERVICE_PAUSED":
      return "Service paused";
    case "SERVICE_RESUMED":
      return "Service resumed";
    case "SERVICE_ENDED":
      return "Service ended";
    case "SCRIPTURE_DETECTED":
    case "SCRIPTURE_UPDATED":
      return str("reference") ? `${str("reference")} detected` : `${str("kind") ?? "Scripture"} detected`;
    case "SUGGESTION_CREATED":
      return `${kindReference("kind") ?? "Reference"} suggested${confidencePercent()}`;
    case "SUGGESTION_APPROVED":
      return `${kindReference("kind") ?? "Suggestion"} approved`;
    case "SUGGESTION_EDITED":
      return `Suggestion edited to ${kindReference("edited") ?? "?"}`;
    case "SUGGESTION_REJECTED":
      return `${kindReference("kind") ?? "Suggestion"} rejected`;
    case "AUDIO_STARTED":
      return "Audio capture started";
    case "AUDIO_STOPPED":
      return "Audio capture stopped";
    case "SPEECH_STARTED":
      return "Speech recognition started";
    case "SPEECH_STOPPED":
      return "Speech recognition stopped";
    case "ERROR_OCCURRED":
      return `Error (${str("context") ?? "unknown"}): ${str("error") ?? "unknown"}`;
    case "SCRIPTURE_CONTEXT_CORRECTED":
      return `Context corrected to ${str("corrected") ?? "?"}`;
    case "SCRIPTURE_AMBIGUOUS_RESOLVED":
      return `Ambiguous reference resolved to ${str("selected") ?? "?"}`;
    case "PRESENTATION_PREPARED":
      return `${str("reference") ?? "Item"} prepared for presentation`;
    default:
      return entry.eventName;
  }
}
