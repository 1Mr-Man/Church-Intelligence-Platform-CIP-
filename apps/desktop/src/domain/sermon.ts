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

// --- Sermon Foundation (Phase 2.5, per the authoritative Phase 2 roadmap) --
//
// Mirrors `cip_core_sermon::foundation` (Rust) - the durable entity/
// lifecycle layer Phase 2.6's real Sermon Intelligence engine will operate
// on, distinct from the semantic taxonomy/state/theme types above (built
// under this repository's earlier internal "Phase 2.3" label - see
// `docs/sermon-foundation.md`'s "Roadmap note"). A `Sermon` answers "which
// message within the current service is being delivered" - never confused
// with `ServiceSession` (`domain/service.ts`), which answers "which
// service is currently happening."

/** The message lifecycle - a separate state machine from `ServiceStatus`
 * (`domain/service.ts`), never reused for this purpose. `planned` and
 * `cancelled` are modeled for completeness but not reachable from any
 * command in this phase (no "schedule a sermon ahead of time" workflow
 * yet) - see `docs/sermon-foundation.md`'s "NOT AVAILABLE" section. */
export type SermonStatus = "planned" | "active" | "paused" | "ended" | "cancelled";

export type SpeakerRole = "primary" | "guest";

/** Explicit, operator-supplied speaker identity - never biometric speaker
 * recognition or diarization, and never confused with a CIP user account. */
export interface Speaker {
  id: string;
  name: string;
  role: SpeakerRole;
}

/** A single message/sermon delivered within one `ServiceSession`. `title`/
 * `speaker` are `null` until an operator explicitly supplies them - never
 * guessed, never defaulted to a placeholder. */
export interface Sermon {
  id: string;
  serviceId: string;
  title: string | null;
  speaker: Speaker | null;
  status: SermonStatus;
  startedAt: string | null; // ISO-8601
  endedAt: string | null; // ISO-8601
  createdAt: string; // ISO-8601
}

/** A fixed, deterministic taxonomy of structural sermon sections - closed
 * (no open-ended "other"), never inferred from transcript content in this
 * phase (only explicit operator assignment, or the automatic
 * `system_boundary` Introduction section a sermon opens with). */
export type SermonSectionKind =
  | "introduction"
  | "scripture_reading"
  | "main_message"
  | "illustration"
  | "prayer"
  | "altar_call"
  | "conclusion";

/** How a section assignment was established. `inferred` is reserved for a
 * future phase - nothing in this phase ever produces it. */
export type SectionOrigin = "operator_assigned" | "system_boundary" | "inferred";

/** One open-or-closed structural span within a sermon. `endedAt: null`
 * means this section is still the sermon's current one; a new section
 * closes the previous one explicitly rather than deleting its history. */
export interface SermonSection {
  id: string;
  sermonId: string;
  kind: SermonSectionKind;
  origin: SectionOrigin;
  startedAt: string; // ISO-8601
  endedAt: string | null; // ISO-8601
  note: string | null;
}

/** "Which portion of the transcript belongs to this sermon" - a thin
 * linkage record, never a copy of the transcript text itself (follow
 * `transcriptSegmentId` back to the canonical transcript segment). */
export interface SermonSegment {
  id: string;
  sermonId: string;
  transcriptSegmentId: string;
  sequence: number;
  sectionId: string | null;
  linkedAt: string; // ISO-8601
}

/** The read-only summary `get_sermon_foundation_state` returns - what
 * structural state exists right now, independent of the `FindingQueue`'s
 * own operator-review status for any finding these commands produced. */
export interface SermonFoundationSummary {
  activeSermon: Sermon | null;
  currentSection: SermonSection | null;
}
