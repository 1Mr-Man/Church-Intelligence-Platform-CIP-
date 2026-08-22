/**
 * Sermon Intelligence domain contracts (Phase 2.3). Mirrors
 * `cip_core_sermon`/`cip_core_intelligence::sermon_adapter` (Rust). Sermon
 * findings themselves are ordinary `IntelligenceFinding`s (see
 * `domain/intelligence.ts`, `domain: "sermon"`, `kind: "sermon"`) - the
 * types here are the taxonomy label and the read-only theme/state/
 * structure snapshot `getSermonState` returns.
 */

/** The closed sermon element taxonomy (spec section 6) - informational
 * only on the frontend (every real finding already carries a human-
 * readable `summary`); useful for icon/label lookups if ever needed. */
export type SermonElementKind =
  | "theme"
  | "main_point"
  | "sub_point"
  | "scripture_reference"
  | "scripture_quotation"
  | "definition"
  | "key_statement"
  | "declaration"
  | "question"
  | "illustration"
  | "story"
  | "example"
  | "application"
  | "prayer_point"
  | "summary"
  | "reflection"
  | "transition"
  | "conclusion";

/** A lightweight classification of current message structure - never a
 * rigid state machine; the pastor may move freely between these. */
export type SermonState =
  | "introduction"
  | "teaching"
  | "main_point"
  | "illustration"
  | "application"
  | "conclusion"
  | "prayer"
  | "unknown";

export interface SermonSubPoint {
  sequence: number;
  rawText: string;
}

/** One recorded main point - append-only; an earlier point is never
 * rewritten when a later one is recorded (spec section 11/52). */
export interface SermonPoint {
  sequence: number;
  rawText: string;
  subPoints: SermonSubPoint[];
}

/** The current theme candidate, only once evidence clears both the
 * repetition and structural-mention thresholds (spec section 14/26) -
 * always `Inferred`, never presented as something the pastor stated
 * verbatim. */
export interface ThemeCandidate {
  label: string;
  confidence: number;
  repetitionCount: number;
  structuralMentions: number;
}

/** The read-only snapshot `get_sermon_state` returns - what
 * `SermonIntelligenceEngine` has derived so far, independent of any
 * pending/accepted/rejected finding review state. */
export interface SermonStateSnapshot {
  state: SermonState;
  theme: ThemeCandidate | null;
  points: SermonPoint[];
}
