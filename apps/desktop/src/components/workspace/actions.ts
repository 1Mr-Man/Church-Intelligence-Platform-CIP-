/**
 * The small set of operator actions a `UnifiedIntelligenceItem` can carry
 * (Phase 2.9, per the authoritative Phase 2 roadmap). Deliberately just a
 * label set, not a dispatch mechanism of its own - `LiveChurchBrain` (the
 * only place that already owns every real command handler) decides what
 * each action actually calls, based on `item.domain`. This keeps
 * `AttentionQueue`/`IntelligenceCard` free of any command-name knowledge,
 * so they can never duplicate IPC logic already living in one place (spec
 * rule 10).
 */
export type UnifiedItemAction =
  | "approve"
  | "display"
  | "reject"
  | "accept"
  | "acknowledge"
  | "review"
  | "dismiss";

/**
 * The actions available for one item, by domain and current status.
 * Mirrors exactly what the existing per-domain panels already offer -
 * never invents a new action a real command doesn't back:
 *
 * - `bible` (a `Suggestion`): display / reject. "Display" replaces the old
 *   "Approve" here (operator feedback: a live Bible reference shouldn't
 *   need a second trip to the Presentation card and an extra click to
 *   reach the screen) - it chains the same `approve_suggestion` +
 *   `prepare_presentation` + `display_presentation` commands the
 *   Presentation card's own three separate buttons already call, in one
 *   operator action. No new backend command; see `handleUnifiedAction` in
 *   `LiveChurchBrain.tsx`.
 * - `music` / `sermon` (an `IntelligenceFinding`): accept / reject.
 * - `service` (only ever an anomaly here - see `unifiedFeed.ts`'s docs on
 *   why transitions never reach the attention queue): acknowledge only.
 * - `content` (a `ContentCandidate`): accept / reject.
 * - `correlation` (an `IntelligenceCorrelation`): review / dismiss.
 */
export function actionsFor(domain: string): UnifiedItemAction[] {
  switch (domain) {
    case "bible":
      return ["display", "reject"];
    case "music":
    case "sermon":
      return ["accept", "reject"];
    case "service":
      return ["acknowledge"];
    case "content":
      return ["accept", "reject"];
    case "correlation":
      return ["review", "dismiss"];
    default:
      return [];
  }
}

export const ACTION_LABELS: Record<UnifiedItemAction, string> = {
  approve: "Approve",
  display: "Display",
  reject: "Reject",
  accept: "Accept",
  acknowledge: "Acknowledge",
  review: "Review",
  dismiss: "Dismiss",
};
