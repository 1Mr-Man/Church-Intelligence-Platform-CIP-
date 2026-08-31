/**
 * AI domain contracts. Mirrors `core/ai` (Rust).
 */
import type { ConfidenceResult } from "./confidence";
import type { AmbiguousCandidate, ScriptureContext, ScriptureReference } from "./bible";

/** Mirrors `core/ai::TranscriptSegment` (Rust). */
export interface TranscriptSegment {
  id: string;
  /** Monotonically increasing per capture session; interim revisions of
   * the same segment do not consume a new number. */
  sequence: number;
  text: string;
  isFinal: boolean;
  confidence: ConfidenceResult;
  /** Audio-relative milliseconds since capture started - not wall-clock
   * time. See `core/ai::TranscriptSegment`'s doc comment. */
  startMs: number;
  endMs: number;
  language: string | null;
  /** Never invented - `null` unless the engine performs speaker ID
   * (none of Phase 1.2's engines do). */
  speakerId: string | null;
}

/**
 * The provider/adaptor contract for speech-to-text. Mirrors `SpeechEngine`
 * in `core/ai`. The frontend never talks to an engine directly - it calls
 * `start_listening`/`stop_listening` and receives `TRANSCRIPT_UPDATED`
 * events - but keeps this shape for documentation/type parity.
 */
export interface SpeechEngine {
  isReady(): boolean;
  feedAudio(samples: Int16Array): Promise<TranscriptSegment[]>;
  flush(): Promise<TranscriptSegment[]>;
}

export type SuggestionStatus = "pending" | "approved" | "edited" | "rejected";

export type SuggestionKind =
  | { type: "scripture"; reference: string }
  | { type: "other"; label: string };

/**
 * An AI-produced proposal awaiting human review. Every suggestion starts
 * `pending`; nothing in the frontend may treat a suggestion as applied
 * until its status has moved via an explicit human action, mirroring the
 * human-controlled guarantee in `core/ai::Suggestion`.
 */
export interface Suggestion {
  id: string;
  serviceId: string;
  kind: SuggestionKind;
  status: SuggestionStatus;
  confidence: ConfidenceResult;
  createdAt: string; // ISO-8601
  /** The transcript segment this suggestion was produced from, if known
   * (Phase 1.3 traceability). `null` for a suggestion with no single
   * originating segment (e.g. an operator's manually resolved ambiguous
   * reference). */
  transcriptSegmentId: string | null;
  /** The transcript substring that produced this suggestion - "what did
   * the pastor say" next to a suggestion in the queue. */
  sourceText: string | null;
}

/**
 * How a piece of transcript text relates to a Bible reference. Mirrors
 * `core/bible::ReferenceKind` (Rust) - see its doc comments for exactly
 * which pipeline stage assigns which variant. `"paraphrase"` (Phase 4.1) is
 * never a citation - it's a lexical/keyword-overlap guess that the
 * segment's wording echoes a specific verse closely enough to be worth an
 * operator's review; it carries the same `pending`-suggestion guarantees
 * as every other kind, never auto-approved or auto-projected.
 */
export type ReferenceKind =
  | "direct"
  | "chapter"
  | "verse"
  | "sequential"
  | "ambiguous"
  | "unresolved"
  | "paraphrase";

/**
 * One reference candidate after context resolution and Bible validation -
 * the Bible Intelligence Core's per-candidate output. Mirrors
 * `core/service::ScriptureDetection` (Rust).
 */
export interface ScriptureDetection {
  kind: ReferenceKind;
  /** Only present for `direct`/`verse`/`sequential` - never for `chapter`
   * (no verse is ever invented), `ambiguous`, or `unresolved`. */
  reference: ScriptureReference | null;
  context: ScriptureContext | null;
  /** Populated only for `ambiguous` detections. */
  candidates: AmbiguousCandidate[];
  confidence: ConfidenceResult;
  rawText: string;
}

/** Mirrors `core/service::ProcessedSegment` (Rust) - the result of running
 * one transcript segment through the full Bible Intelligence Core pipeline. */
export interface ProcessedSegment {
  serviceId: string;
  detections: ScriptureDetection[];
  suggestions: Suggestion[];
}
