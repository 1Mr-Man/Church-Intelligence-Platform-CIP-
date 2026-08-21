/**
 * AI domain contracts. Mirrors `core/ai` (Rust).
 */
import type { ConfidenceResult } from "./confidence";

export interface TranscriptSegment {
  text: string;
  isFinal: boolean;
  confidence: ConfidenceResult;
  startMs: number;
  endMs: number;
}

/**
 * The provider/adaptor contract for speech-to-text. Mirrors `SpeechEngine`
 * in `core/ai` - no implementation exists yet (speech recognition is
 * explicitly out of scope for Phase 1).
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
  kind: SuggestionKind;
  status: SuggestionStatus;
  confidence: ConfidenceResult;
  createdAt: string; // ISO-8601
}
