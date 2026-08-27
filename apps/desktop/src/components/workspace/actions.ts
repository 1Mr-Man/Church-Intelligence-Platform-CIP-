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
export type UnifiedItemAction = "approve" | "reject" | "accept" | "acknowledge" | "review" | "dismiss";

/**
 * The actions available for one item, by domain and current status.
 * Mirrors exactly what the existing per-domain panels already offer -
 * never invents a new action a real command doesn't back:
 *
 * - `bible` (a `Suggestion`): approve / reject.
 * - `music` / `sermon` (an `IntelligenceFinding`): accept / reject.
 * - `service` (only ever an anomaly here - see `unifiedFeed.ts`'s docs on
 *   why transitions never reach the attention queue): acknowledge only.
 * - `content` (a `ContentCandidate`): accept / reject.
 * - `correlation` (an `IntelligenceCorrelation`): review / dismiss.
 */
export function actionsFor(domain: string): UnifiedItemAction[] {
  switch (domain) {
    case "bible":
      return ["approve", "reject"];
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
  reject: "Reject",
  accept: "Accept",
  acknowledge: "Acknowledge",
  review: "Review",
  dismiss: "Dismiss",
};
