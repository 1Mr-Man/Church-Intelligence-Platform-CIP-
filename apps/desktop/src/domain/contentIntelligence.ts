/**
 * Content Intelligence domain contracts (Phase 2.7, per the authoritative
 * Phase 2 roadmap). Mirrors `cip_core_intelligence::content_candidate`/
 * `content_intelligence` (Rust) exactly.
 *
 * `ContentCandidate` is deliberately its own type, not folded into
 * `IntelligenceFinding` (`domain/intelligence.ts`) - mirroring
 * `IntelligenceCorrelation`'s own precedent: a candidate's meaning ("this
 * detected information appears suitable for a particular future content
 * purpose") is structurally different from a finding's meaning ("this was
 * detected"). See `docs/content-intelligence.md`.
 */

import type { ConfidenceResult } from "./confidence";
import type { AssertionLevel, EvidenceSource, FindingStatus, IntelligenceProvenance } from "./intelligence";

/** A closed, minimal taxonomy of future content opportunity types -
 * justified entirely by real Phase 2.6 Sermon Intelligence finding
 * categories, not implemented speculatively. See
 * `content_intelligence.rs`'s `SUMMARY_PREFIX_MAPPINGS` for the exact
 * source mapping. */
export type ContentCandidateType =
  | "theme"
  | "teaching"
  | "reflection"
  | "takeaway"
  | "food_for_thought"
  | "quote"
  | "discussion_question"
  | "scripture_reflection"
  | "illustration";

/**
 * A future content opportunity CIP has structured from an already-proven
 * finding - never final copy. `titleOrLabel`/`workingConcept` are
 * deterministic, unpolished labels/paraphrase scaffolding, never
 * marketing prose.
 *
 * `confidence` is reused unchanged from the source finding - it still
 * means exactly what it always has ("how certain is this fact").
 * `contentPotential` is a deliberately separate 0.0-1.0 dimension ("how
 * suitable this finding appears as a future content opportunity") -
 * explicitly NOT a replacement for `confidence`. A highly-confident
 * finding does not automatically score high content potential, and vice
 * versa.
 */
export interface ContentCandidate {
  id: string;
  serviceId: string;
  /** The active `Sermon`'s id (Phase 2.5 foundation) this candidate was
   * derived under, if any - `null` when the source finding carried none.
   * Never fabricated. */
  sermonId: string | null;
  /** The finding(s) this candidate was derived from - always at least
   * one; a candidate that cannot explain its source is never produced. */
  sourceFindingIds: string[];
  candidateType: ContentCandidateType;
  /** A short, deterministic working label - never polished marketing
   * copy. */
  titleOrLabel: string;
  /** For `quote` candidates, the exact verbatim transcript excerpt; for
   * every other type, the source finding's own summary text, never
   * paraphrased into new prose. */
  workingConcept: string;
  /** Inherited from the source finding - never upgraded to look more
   * certain merely because a finding became a candidate. */
  assertionLevel: AssertionLevel;
  /** Reused `FindingStatus` exactly - a candidate lifecycle distinct from
   * the source finding's own status but never a second enum. */
  status: FindingStatus;
  confidence: ConfidenceResult;
  /** 0.0-1.0: "how suitable this finding appears as a future content
   * opportunity" - explicitly NOT a truth-confidence score. See this
   * file's own module docs. */
  contentPotential: number;
  evidence: EvidenceSource[];
  provenance: IntelligenceProvenance;
  engineId: string;
  engineVersion: string;
  createdAt: string; // ISO-8601
}
