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

/** `{ book: "ROM", chapter: 8, verseStart: 28 }` -> `"ROM 8:28"` - mirrors
 * `core/bible::ScriptureReference`'s `Display` impl (Rust). Used to turn a
 * manual Bible search result into the reference string
 * `preview_scripture`/`create_manual_presentation` expect. */
export function formatScriptureReference(reference: ScriptureReference): string {
  if (reference.verseEnd !== null && reference.verseEnd !== reference.verseStart) {
    return `${reference.book} ${reference.chapter}:${reference.verseStart}-${reference.verseEnd}`;
  }
  return `${reference.book} ${reference.chapter}:${reference.verseStart}`;
}

export interface BibleChapter {
  book: string;
  chapter: number;
  verses: BibleVerse[];
}

/** One Bible search result (Phase 1.5) - mirrors
 * `cip_core_bible::search::BibleSearchResult` (Rust). Enough to render and
 * act on (Preview/Prepare) without a raw database row. */
export interface BibleSearchResult {
  translationId: string;
  book: string;
  chapter: number;
  verse: number;
  reference: string;
  text: string;
  /** `1.0` for an exact reference/chapter match, `null` for a free-text
   * match (no fabricated ranking - see the Rust type's docs). */
  relevance: number | null;
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

// --- Scripture Context Manager (implemented in Phase 1.1) -----------------
//
// Models how pastors actually speak: "Romans chapter 8" establishes a
// context; "verse 28" resolves against it even across unrelated
// intervening speech; a new chapter reference replaces it. See
// `docs/bible-intelligence.md` for the full behavior and
// `core/bible/src/context_manager.rs` (Rust) for the implementation this
// mirrors.

/** Mirrors `core/bible::ScriptureContext` (Rust). */
export interface ScriptureContext {
  translationId: string;
  /** Canonical book code (e.g. `"ROM"`), never a display name. */
  book: string;
  chapter: number;
  /** The most recently resolved verse in this context, or `null` right
   * after a bare chapter reference - no verse is ever invented. */
  lastVerse: number | null;
  confidence: ConfidenceResult;
  establishedAt: string; // ISO-8601
  valid: boolean;
}

/** One candidate offered when a bare verse is ambiguous between the
 * active context and a just-replaced one. Mirrors
 * `core/bible::AmbiguousCandidate`. */
export interface AmbiguousCandidate {
  reference: ScriptureReference;
  confidence: ConfidenceResult;
}

// `ContextResolution` (the Scripture Context Manager's internal
// resolve() outcome) is not exposed over IPC - only the higher-level
// `ScriptureDetection` (see `ai.ts`) is. See `core/bible/src/context.rs`
// for that internal type if you need it.

export interface ScriptureContextManager {
  resolve(fragment: PartialScriptureReference): unknown;
  activeContext(): ScriptureContext | null;
  recentReferences(limit: number): ScriptureReference[];
  confirmActive(): void;
  rejectActive(): void;
}
