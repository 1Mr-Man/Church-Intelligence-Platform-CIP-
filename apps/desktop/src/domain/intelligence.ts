/**
 * Shared intelligence architecture domain contracts (Phase 2.0). Mirrors
 * `cip_core_intelligence` (Rust). Only `Bible` has a real engine behind it
 * in this phase - `Music`/`Sermon`/`Content`/`CrossDomain` reserve the
 * shape future phases will populate. See `docs/intelligence-architecture.md`.
 */

import type { ConfidenceResult } from "./confidence";

export type IntelligenceDomain =
  | "bible"
  | "music"
  | "service"
  | "sermon"
  | "content"
  | "cross_domain";

export type FindingKind =
  | "scripture"
  | "music"
  | "service_state"
  | "sermon"
  | "content"
  | "correlation";

export type FindingStatus = "detected" | "reviewed" | "accepted" | "rejected" | "expired";

/**
 * The mandatory epistemic-state distinction: CIP must never present one of
 * these as if it were another. `observed` = what was actually said;
 * `inferred` = a state CIP derived (e.g. an active Scripture context);
 * `suggested` = a specific proposal for human review (e.g. a concrete
 * verse reference); `generated` = synthesized content - reserved, not
 * produced by anything in Phase 2.0.
 */
export type AssertionLevel = "observed" | "inferred" | "suggested" | "generated";

/** Urgency, not certainty - see `IntelligenceFinding.confidence` for that. */
export type IntelligencePriority = "low" | "normal" | "high" | "critical";

/** Whether an engine is even worth calling right now - distinguishes "not
 * installed" from "installed but currently broken" from "installed but
 * turned off." */
export type EngineCapability = "available" | "unavailable" | "disabled" | "error";

export type EvidenceSource =
  | { kind: "transcript"; segmentIds: string[]; excerpt: string }
  | { kind: "content"; contentId: string }
  | { kind: "context"; description: string }
  | { kind: "temporal"; description: string }
  | { kind: "another_finding"; findingId: string }
  | { kind: "service_event"; description: string }
  | { kind: "operator_action"; description: string };

/** Where a finding's underlying content traces back to - references the
 * Phase 1.5 Content Registry by id rather than re-implementing its
 * licensing model. `contentId: null` means this finding has no
 * content-registry-backed source. */
export interface IntelligenceProvenance {
  contentId: string | null;
  note: string | null;
}

export interface IntelligenceFinding {
  id: string;
  serviceId: string;
  domain: IntelligenceDomain;
  kind: FindingKind;
  assertionLevel: AssertionLevel;
  status: FindingStatus;
  priority: IntelligencePriority;
  confidence: ConfidenceResult;
  summary: string;
  transcriptSegmentIds: string[];
  evidence: EvidenceSource[];
  provenance: IntelligenceProvenance;
  engineId: string;
  engineVersion: string;
  createdAt: string; // ISO-8601
}

export type CorrelationKind =
  | { kind: "temporal_proximity" }
  | { kind: "shared_context" }
  | { kind: "other"; detail: string };

export interface IntelligenceCorrelation {
  id: string;
  sourceFindingIds: string[];
  kind: CorrelationKind;
  confidence: ConfidenceResult;
  evidence: EvidenceSource[];
  createdAt: string;
}

/** One domain's real capability, from the `get_intelligence_capabilities`
 * diagnostic command - `engineId`/`engineVersion` are `null` for a domain
 * with no registered engine at all, never a placeholder value. */
export interface DomainCapabilityReport {
  domain: IntelligenceDomain;
  capability: EngineCapability;
  engineId: string | null;
  engineVersion: string | null;
}
