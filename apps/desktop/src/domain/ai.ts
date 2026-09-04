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
 * Payload for the `TRANSCRIPT_CORRECTED` event (Phase 24.3, true dual-tier
 * Whisper). `correctedSegment` is a genuine new, final `TranscriptSegment`
 * (its own `id`, already routed through the same detection/persistence
 * pipeline every other final segment uses) - `originalSegmentId` only
 * links it back to the fast-tier segment it re-transcribes. The original
 * segment's own earlier `TRANSCRIPT_UPDATED` event/text is never replaced
 * or edited - see `docs/phase-24-3-audit.md` for why.
 */
export interface TranscriptCorrected {
  originalSegmentId: string;
  correctedSegment: TranscriptSegment;
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
  /** How many times this suggestion's reference was independently
   * redetected while still `pending`, within the live pipeline's
   * suggestion-dedup window (Phase 5.2, "temporal confirmation") -
   * corroborating evidence for a heuristic (paraphrase/semantic) guess,
   * never a reason a second suggestion was created. `0` for an explicit
   * citation or any suggestion never independently redetected. */
  confirmationCount: number;
  /** How many times this suggestion's own reference was independently
   * redetected (same paraphrase/semantic category) within the dedup
   * window *after* an operator explicitly `rejected` it (Phase 5.4,
   * "wrong-verse feedback loop") - the repeat is still silently
   * suppressed exactly as before (a rejected suggestion is never
   * resurrected to pending), but this count makes that already-existing
   * suppression observable instead of leaving no trace at all. `0` for
   * any suggestion never rejected, or rejected but never redetected
   * again. */
  rejectionEchoCount: number;
}

/**
 * How a piece of transcript text relates to a Bible reference. Mirrors
 * `core/bible::ReferenceKind` (Rust) - see its doc comments for exactly
 * which pipeline stage assigns which variant. `"paraphrase"` (Phase 4.1)
 * and `"semantic"` (Phase 4.4) are never citations - both are guesses
 * (lexical/keyword overlap, and embedding-based meaning similarity,
 * respectively) that the segment's wording is likely based on a specific
 * verse. `"fuzzy_book"` (Phase 20) *is* a citation shape (a word
 * immediately followed by a real chapter:verse pattern) but the word
 * itself only near-missed a known book name/alias - see
 * `core/bible::book_alias::fuzzy_match_book`'s doc comment for exactly
 * what it will and won't guess.
 * verse; both carry the same `pending`-suggestion guarantees as every
 * other kind, never auto-approved or auto-projected.
 */
export type ReferenceKind =
  | "direct"
  | "chapter"
  | "verse"
  | "sequential"
  | "ambiguous"
  | "unresolved"
  | "paraphrase"
  | "semantic"
  | "fuzzy_book";

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
