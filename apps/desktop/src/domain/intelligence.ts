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
  | { kind: "operator_action"; description: string }
  /** Derived from acoustic (audio-fingerprint/embedding) recognition
   * (Phase 2.2) - `method`/`durationMs` are honest facts about how the
   * underlying acoustic candidate was produced, never a claim that
   * acoustic recognition is more certain than `confidence` already says. */
  | { kind: "acoustic"; segmentId: string; method: string; durationMs: number };

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
  /** The active Sermon Foundation `Sermon`'s id this finding was produced
   * under (Phase 2.6) - `null` both when no sermon was active and for
   * every non-Sermon domain, which never attaches one. Never guessed. */
  sermonId: string | null;
  evidence: EvidenceSource[];
  provenance: IntelligenceProvenance;
  engineId: string;
  engineVersion: string;
  createdAt: string; // ISO-8601
}

/**
 * Closed, deterministic taxonomy (Phase 2.4 spec section 5, extended in
 * Phase 2.8) - `other` keeps this extensible without a schema change.
 * `temporal_proximity` (Phase 2.0) is the same concept the Phase 2.4 spec
 * calls "TemporalAssociation" - reused under its original name rather than
 * adding a duplicate variant. `sermon_content`/`multi_domain_convergence`
 * are Phase 2.8's only two new variants - see
 * `docs/cross-domain-intelligence.md`'s taxonomy section for why no
 * dedicated `service_music`/`service_scripture` kind was added (Service
 * now participates in `temporal_proximity` instead).
 */
export type CorrelationKind =
  | { kind: "temporal_proximity" }
  | { kind: "shared_context" }
  | { kind: "scripture_sermon" }
  | { kind: "scripture_music" }
  | { kind: "sermon_music" }
  | { kind: "theme_scripture" }
  | { kind: "theme_music" }
  | { kind: "service_transition" }
  | { kind: "sermon_content" }
  | { kind: "multi_domain_convergence" }
  | { kind: "other"; detail: string };

/**
 * A typed record that two or more findings are related (Phase 2.4),
 * produced by `core/intelligence::cross_domain::CrossDomainCorrelationEngine`.
 * Deliberately its own type, not folded into `IntelligenceFinding` - a
 * correlation's meaning ("these findings are related, here is why") is
 * structurally different from a finding's meaning ("this was detected").
 * See `docs/cross-domain-intelligence.md`.
 */
export interface IntelligenceCorrelation {
  id: string;
  serviceId: string;
  sourceFindingIds: string[];
  /** Every `IntelligenceDomain` represented among `sourceFindingIds`. */
  domains: IntelligenceDomain[];
  kind: CorrelationKind;
  /** Always `inferred` in Phase 2.4 - a correlation is always CIP's own
   * derived judgment that two findings relate, never a verbatim
   * observation and never generated content. */
  assertionLevel: AssertionLevel;
  /** Operator-review lifecycle, reusing `FindingStatus` exactly. */
  status: FindingStatus;
  confidence: ConfidenceResult;
  /** Short human-readable description of the relationship - never a vague
   * claim like "these things seem related." */
  summary: string;
  evidence: EvidenceSource[];
  /** Deterministic identity of the rule that produced this correlation
   * (e.g. `"scripture_sermon_v1"`) - provenance/rule-versioning, not a
   * plugin system. */
  ruleId: string;
  ruleVersion: string;
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
