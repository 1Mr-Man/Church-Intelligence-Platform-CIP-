/**
 * Search domain contract. Mirrors `core/search` (Rust): a single
 * source-agnostic contract the UI queries regardless of which domain
 * (Bible, sermon, presentation) backs a given result.
 */
import type { ConfidenceResult } from "./confidence";

export type SearchResultSource = "bible" | "sermon" | "presentation";

export interface SearchResult {
  source: SearchResultSource;
  referenceId: string;
  title: string;
  snippet: string;
  relevance: ConfidenceResult;
}

export interface SearchEngine {
  search(query: string): Promise<SearchResult[]>;
}
