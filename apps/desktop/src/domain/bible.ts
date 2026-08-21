/**
 * Bible domain contracts. Mirrors `core/bible` (Rust). The frontend never
 * implements a `BibleProvider` itself - implementations live behind Tauri
 * commands - but it needs the same shapes to type IPC responses and to
 * define what a future in-process provider (e.g. a cache) would look like.
 */
import type { ConfidenceResult } from "./confidence";

export interface ScriptureReference {
  translationId: string;
  book: string;
  chapter: number;
  verseStart: number;
  verseEnd: number | null;
}

export interface PartialScriptureReference {
  book: string | null;
  chapter: number | null;
  verseStart: number | null;
  verseEnd: number | null;
}

export interface BibleTranslation {
  id: string;
  name: string;
  abbreviation: string;
  language: string;
  isLocal: boolean;
}

export type Testament = "old" | "new";

export interface BibleBook {
  code: string;
  name: string;
  testament: Testament;
  chapterCount: number;
  order: number;
}

export interface BibleVerse {
  reference: ScriptureReference;
  text: string;
}

export interface BibleChapter {
  book: string;
  chapter: number;
  verses: BibleVerse[];
}

/**
 * The provider/adaptor contract for anything that can serve Bible content.
 * Concrete implementations (a local SQLite-backed provider today, network
 * providers later) live behind Tauri commands - the frontend depends only
 * on this shape, never on a specific source, mirroring `BibleProvider` in
 * `core/bible`.
 */
export interface BibleProvider {
  listTranslations(): Promise<BibleTranslation[]>;
  getBook(translationId: string, bookCode: string): Promise<BibleBook | null>;
  getChapter(translationId: string, bookCode: string, chapter: number): Promise<BibleChapter | null>;
  getVerse(reference: ScriptureReference): Promise<BibleVerse | null>;
  search(query: string, translationId: string): Promise<BibleVerse[]>;
}

// --- Scripture Context Manager: interface boundary only (Phase 1) --------
//
// Planned behavior (not implemented yet):
//   "Romans 8"            -> ACTIVE SCRIPTURE CONTEXT = Romans 8
//   "verse 28"             -> resolves to Romans 8:28
//   "verse 31"              -> resolves to Romans 8:31
//   "go back to verse 18"   -> resolves to Romans 8:18
// See `core/bible/src/context.rs` for the full rationale. Nothing calls
// this yet - it exists to reserve the architectural boundary, not to
// prescribe a final wire format - so `ContextResolution` here models the
// domain shape, not the literal (not yet camelCase-normalized) serde
// encoding of the Rust enum. Finalize both sides together when the
// resolution algorithm is actually implemented.

export interface ScriptureContext {
  reference: ScriptureReference;
  confidence: ConfidenceResult;
  establishedAt: string; // ISO-8601
}

export type ContextResolution =
  | { type: "established"; context: ScriptureContext }
  | { type: "resolved"; reference: ScriptureReference; confidence: ConfidenceResult }
  | { type: "replaced"; previous: ScriptureContext; current: ScriptureContext }
  | { type: "ambiguous"; candidates: ScriptureReference[] }
  | { type: "unresolved" };

export interface ScriptureContextManager {
  resolve(fragment: PartialScriptureReference): ContextResolution;
  activeContext(): ScriptureContext | null;
  recentReferences(limit: number): ScriptureReference[];
  confirmActive(): void;
  rejectActive(): void;
}
