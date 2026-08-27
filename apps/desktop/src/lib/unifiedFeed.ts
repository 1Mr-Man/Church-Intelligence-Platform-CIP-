/**
 * Unified Operator Workspace (Phase 2.9, per the authoritative Phase 2
 * roadmap) - the frontend-only projection that lets the operator see
 * Bible/Music/Sermon/Service/Content/Cross-Domain activity as one
 * chronological, bounded, deterministically-ordered feed, instead of
 * mentally assembling six separate panels.
 *
 * This is explicitly NOT a second backend intelligence type (spec section
 * 6/17): `UnifiedIntelligenceItem` is a UI view-model built from data the
 * existing panels already fetch through existing commands/events - a
 * `Suggestion`, `IntelligenceFinding`, `ContentCandidate`, or
 * `IntelligenceCorrelation`. Nothing here re-detects, re-infers, or
 * upgrades a confidence/assertion value; every item keeps a reference to
 * its real `source` object so evidence/provenance stay reachable (spec
 * rule 8: "the operator must be able to answer why did CIP produce this").
 */
import type {
  AssertionLevel,
  ConfidenceResult,
  ContentCandidate,
  IntelligenceCorrelation,
  IntelligenceFinding,
  Suggestion,
} from "../domain";

export type UnifiedIntelligenceDomain = "bible" | "music" | "sermon" | "service" | "content" | "correlation";

/**
 * Bound for the merged feed (spec rule 12: "define explicit UI limits...
 * do not arbitrarily load thousands of historical findings"). Same order
 * of magnitude as the backend's own `DEFAULT_MAX_RECENT_FINDINGS`-style
 * bounds (`core/intelligence::context`, 20 per domain) scaled up slightly
 * since this merges six domains into one list.
 */
export const MAX_VISIBLE_INTELLIGENCE_ITEMS = 50;

/**
 * A single unified feed entry. Deliberately thin: everything the operator
 * needs to judge "why did CIP produce this" (confidence, assertion level,
 * status, evidence count, and the real underlying object) is retained,
 * never flattened into an unexplained colored card (spec rule 8).
 */
export interface UnifiedIntelligenceItem {
  /** Stable id of the underlying object - never invented. */
  id: string;
  domain: UnifiedIntelligenceDomain;
  summary: string;
  confidence: ConfidenceResult;
  assertionLevel: AssertionLevel;
  /** The literal status string from the underlying object
   * (`SuggestionStatus` or `FindingStatus`) - never lossily remapped, so
   * the real lifecycle state stays inspectable. */
  rawStatus: string;
  /** Whether this item is still awaiting an operator decision - the
   * attention-queue filter reuses this rather than re-deriving it
   * per-caller. */
  needsAttention: boolean;
  createdAt: string; // ISO-8601
  /** A short secondary line giving provenance context (source transcript
   * text, rule id, or candidate type) - `null` when the underlying object
   * carries none. Never fabricated. */
  detailLine: string | null;
  evidenceCount: number;
  source: Suggestion | IntelligenceFinding | ContentCandidate | IntelligenceCorrelation;
}

export interface UnifiedFeedSources {
  /** Bible domain: the live Scripture-detection pipeline's own
   * `Suggestion`s (Phase 1.3) - not a fetched `IntelligenceFinding` list,
   * since no command to list Bible-domain findings exists (the
   * `analyze_bible_transcript` bridge, per Phase 2.8, is write-only, for
   * cross-domain correlation - see `docs/cross-domain-intelligence.md`).
   * This is the real, already-displayed mechanism operators act on. */
  suggestions: Suggestion[];
  musicFindings: IntelligenceFinding[];
  sermonFindings: IntelligenceFinding[];
  /** Service domain, transitions: a historical log of phase changes the
   * existing Service Intelligence panel already lists - genuinely
   * informational, since the real UI offers no accept/reject action for
   * one (only anomalies do). Always mapped to `needsAttention: false`,
   * regardless of its raw `FindingStatus` - claiming otherwise would
   * dangle an action the operator cannot actually take. */
  serviceTransitions: IntelligenceFinding[];
  /** Service domain, anomalies: the one Service Intelligence finding kind
   * with a real operator action (`acknowledgeServiceAnomaly`) - these do
   * participate in the attention queue like every other actionable
   * finding. */
  serviceAnomalies: IntelligenceFinding[];
  contentCandidates: ContentCandidate[];
  correlations: IntelligenceCorrelation[];
}

function suggestionNeedsAttention(status: Suggestion["status"]): boolean {
  return status === "pending" || status === "edited";
}

function findingNeedsAttention(status: IntelligenceFinding["status"]): boolean {
  return status === "detected" || status === "reviewed";
}

function fromSuggestion(s: Suggestion): UnifiedIntelligenceItem {
  return {
    id: s.id,
    domain: "bible",
    summary: s.kind.type === "scripture" ? s.kind.reference : s.kind.label,
    confidence: s.confidence,
    // A `Suggestion` is, by its own definition, a specific proposal
    // awaiting human review - exactly what `AssertionLevel.suggested`
    // means. This is an accurate categorical label, not an invented one:
    // `core/ai::Suggestion` carries no `assertionLevel` field of its own
    // because Phase 1.3 predates the Phase 2.0 intelligence architecture.
    assertionLevel: "suggested",
    rawStatus: s.status,
    needsAttention: suggestionNeedsAttention(s.status),
    createdAt: s.createdAt,
    detailLine: s.sourceText,
    evidenceCount: s.sourceText ? 1 : 0,
    source: s,
  };
}

function fromFinding(
  f: IntelligenceFinding,
  domain: UnifiedIntelligenceDomain,
  actionable: boolean,
): UnifiedIntelligenceItem {
  return {
    id: f.id,
    domain,
    summary: f.summary,
    confidence: f.confidence,
    assertionLevel: f.assertionLevel,
    rawStatus: f.status,
    needsAttention: actionable && findingNeedsAttention(f.status),
    createdAt: f.createdAt,
    detailLine: null,
    evidenceCount: f.evidence.length,
    source: f,
  };
}

function fromContentCandidate(c: ContentCandidate): UnifiedIntelligenceItem {
  return {
    id: c.id,
    domain: "content",
    summary: c.titleOrLabel,
    confidence: c.confidence,
    assertionLevel: c.assertionLevel,
    rawStatus: c.status,
    needsAttention: findingNeedsAttention(c.status),
    createdAt: c.createdAt,
    detailLine: c.candidateType.replace(/_/g, " "),
    evidenceCount: c.evidence.length,
    source: c,
  };
}

function fromCorrelation(c: IntelligenceCorrelation): UnifiedIntelligenceItem {
  return {
    id: c.id,
    domain: "correlation",
    summary: c.summary,
    confidence: c.confidence,
    assertionLevel: c.assertionLevel,
    rawStatus: c.status,
    needsAttention: findingNeedsAttention(c.status),
    createdAt: c.createdAt,
    detailLine: c.ruleId,
    evidenceCount: c.evidence.length,
    source: c,
  };
}

/**
 * Deterministic multi-key comparator (spec rule 13): newest first, then
 * confidence descending, then domain, then id as a final stable tiebreak -
 * mirrors `core/intelligence::cross_domain::sort_deterministically`'s own
 * discipline on the Rust side. Never depends on object/array insertion
 * order or any iteration-order-sensitive structure.
 */
function compareItems(a: UnifiedIntelligenceItem, b: UnifiedIntelligenceItem): number {
  if (a.createdAt !== b.createdAt) return a.createdAt < b.createdAt ? 1 : -1;
  if (a.confidence.score !== b.confidence.score) return b.confidence.score - a.confidence.score;
  if (a.domain !== b.domain) return a.domain < b.domain ? -1 : 1;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * Merge every domain's already-fetched intelligence output into one
 * bounded, deterministically-ordered feed. Pure and synchronous - no IPC,
 * no re-derivation, no mutation of the inputs.
 */
export function buildUnifiedFeed(sources: UnifiedFeedSources): UnifiedIntelligenceItem[] {
  const items: UnifiedIntelligenceItem[] = [
    ...sources.suggestions.map(fromSuggestion),
    ...sources.musicFindings.map((f) => fromFinding(f, "music", true)),
    ...sources.sermonFindings.map((f) => fromFinding(f, "sermon", true)),
    ...sources.serviceTransitions.map((f) => fromFinding(f, "service", false)),
    ...sources.serviceAnomalies.map((f) => fromFinding(f, "service", true)),
    ...sources.contentCandidates.map(fromContentCandidate),
    ...sources.correlations.map(fromCorrelation),
  ];
  items.sort(compareItems);
  return items.slice(0, MAX_VISIBLE_INTELLIGENCE_ITEMS);
}
