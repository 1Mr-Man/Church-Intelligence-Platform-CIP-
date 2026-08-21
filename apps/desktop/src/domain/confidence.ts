/**
 * Confidence scoring contract shared across CIP's domains.
 *
 * Mirrors `cip-core-confidence` (Rust). The frontend never recomputes a
 * confidence result itself in Phase 1 - these are read-only contracts for
 * data the backend already scored - but the bucketing function is kept in
 * sync with the Rust implementation so UI code and unit tests can reason
 * about thresholds without a round trip.
 */

export type ConfidenceSource = "heuristic" | "model" | "human";

export type ConfidenceLevel = "low" | "medium" | "high";

export interface ConfidenceResult {
  score: number;
  level: ConfidenceLevel;
  source: ConfidenceSource;
  reason: string | null;
}

/**
 * Bucket a raw `0.0..=1.0` score into a {@link ConfidenceLevel}.
 * Mirrors `ConfidenceLevel::from_score` in `core/confidence/src/lib.rs` -
 * keep the thresholds identical if either side changes.
 */
export function confidenceLevelFromScore(score: number): ConfidenceLevel {
  if (score >= 0.8) return "high";
  if (score >= 0.5) return "medium";
  return "low";
}

/** Phase 1 policy: only `high` confidence auto-applies. */
export function isAutoAcceptable(result: Pick<ConfidenceResult, "level">): boolean {
  return result.level === "high";
}
