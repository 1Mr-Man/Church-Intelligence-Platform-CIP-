/**
 * Music Intelligence domain contracts (Phase 2.1). Mirrors
 * `cip_core_music`/`cip_integrations_music` (Rust). Song recognition
 * itself produces `IntelligenceFinding`s (see `domain/intelligence.ts`) -
 * the types here are the manual-search result shape
 * (`SongRecognitionCandidate`) and the dataset import file format, the
 * music-domain counterparts to `BibleSearchResult`/`BibleDatasetInput` in
 * `domain/bible.ts`/`domain/content.ts`.
 */

import type { ConfidenceResult } from "./confidence";

export type SongType =
  | "hymn"
  | "worship_song"
  | "chorus"
  | "gospel_song"
  | "psalm"
  | "anthem"
  | "spiritual_song"
  | "other";

export type SectionKind =
  | "verse"
  | "chorus"
  | "bridge"
  | "refrain"
  | "stanza"
  | "intro"
  | "outro"
  | "other";

/** How `search_music` matched a candidate - see
 * `core/music::matcher`'s module docs for the full confidence hierarchy
 * this ordering reflects. `acoustic` (Phase 2.2) is now genuinely
 * constructed, by evidence fusion, from a real `AcousticMusicRecognizer`
 * result - never fabricated when no recognizer is configured (see
 * `docs/acoustic-music.md`). */
export type MatchType =
  | "explicit_title"
  | "alias"
  | "song_number"
  | "exact_lyric"
  | "multiple_lyric_lines"
  | "partial_lyric"
  | "contextual"
  | "acoustic";

/** One `search_music` result - always evidence-carrying, never a bare
 * song reference (Phase 2.1 spec section 7: "a finding should be
 * auditable"). */
export interface SongRecognitionCandidate {
  songId: string;
  matchType: MatchType;
  matchedText: string;
  confidence: ConfidenceResult;
  evidence: string[];
  /** The dataset (Content Registry id, e.g. `"music:dev-hymnbook"`) this
   * candidate's song belongs to. */
  source: string;
  ranking: number;
  explanation: string;
}

/** `search_music`'s query-shape parameter - which of a song's title,
 * number, or lyric text `query` should be matched against. */
export type MusicQueryType = "title" | "number" | "lyric";

/** A deterministic report of one music dataset import call - every
 * number is derived from the actual dataset and database state, never
 * hard-coded. The music-domain counterpart to `ImportReport` in
 * `domain/content.ts` (a distinct type, since the two datasets' shapes
 * genuinely differ - songs/lyric lines here, not books/chapters/verses). */
export interface MusicImportReport {
  contentId: string;
  datasetVersion: string;
  songsTotal: number;
  songsImported: number;
  songsAlreadyPresent: number;
  songsInvalid: number;
  lyricLinesImported: number;
  lyricLinesAlreadyPresent: number;
  errors: string[];
  checksum: string;
}

// --- music dataset import file format (docs/music-datasets.md) ------------
//
// The shape `import_music_dataset` expects as JSON text - mirrors
// `cip_integrations_music::importer::MusicDatasetInput`. Kept here (not
// just in the Rust docs) so the frontend's import UI can validate a
// selected file's shape before ever sending it to the backend.

export interface MusicSectionInput {
  id: string;
  kind: string;
  sequence: number;
}

export interface MusicLyricInput {
  sectionId?: string | null;
  sequence: number;
  text: string;
}

export interface MusicSongInput {
  id: string;
  title: string;
  aliases?: string[];
  songType: string;
  language: string;
  number?: string | null;
  author?: string | null;
  composer?: string | null;
  sections?: MusicSectionInput[];
  lyrics?: MusicLyricInput[];
}

export interface MusicDatasetInput {
  contentId: string;
  name: string;
  language: string;
  publisher?: string | null;
  copyright?: string | null;
  license?: string | null;
  distribution?: string | null;
  datasetVersion: string;
  songs: MusicSongInput[];
}

// --- acoustic recognition (Phase 2.2) --------------------------------------
//
// Mirrors `cip_core_music::acoustic`/`apps/desktop/src-tauri/src/acoustic.rs`
// (Rust). Acoustic recognition never runs automatically into the
// projector - the shapes here are diagnostic/read-model only. See
// `docs/acoustic-music.md`.

/** Whether an `AcousticMusicRecognizer` is even worth calling right now -
 * distinguishes "not installed"/`unavailable` from "installed but
 * currently broken"/`error` from "installed but turned off"/`disabled`.
 * Never treat `unavailable` as an error to work around: it is the honest,
 * expected state when no acoustic model is configured, and lyric/title
 * Music Intelligence keeps working regardless. */
export type AcousticRecognitionStatus = "available" | "unavailable" | "disabled" | "error";

/** How a recognizer is implemented - never a specific vendor/model name.
 * `none` means no recognizer is configured at all (the safe default). */
export type AcousticRecognitionMethod = "local_model" | "external_provider" | "test" | "none";

/** Part of `LiveStatus.acousticStatus` - `reason` is a real, human-readable
 * explanation (e.g. "no acoustic model directory configured"), never a
 * placeholder string, and always present when `status` is not
 * `available`. */
export interface AcousticEngineStatus {
  status: AcousticRecognitionStatus;
  method: AcousticRecognitionMethod;
  reason: string | null;
}

/** The song a human operator has explicitly confirmed as currently being
 * sung - never set automatically by acoustic/lyric confidence alone,
 * regardless of how high. Set only by `acceptMusicFinding` deriving one
 * from the accepted finding's evidence, and cleared only by
 * `clearCurrentSong`. See `docs/acoustic-music.md`'s Current Song section. */
export interface CurrentSong {
  contentId: string;
  songId: string;
  confidence: ConfidenceResult;
}

// --- real audio fingerprinting enrollment (Phase 7.2) ----------------------
//
// Mirrors `apps/desktop/src-tauri/src/commands.rs`'s `AcousticEnrollment`
// (Rust). Enrolling a reference recording never takes effect until CIP
// restarts - `listAcousticEnrollments` reads the on-disk manifest, not
// `AcousticEngineStatus` (which reflects whichever recognizer was built at
// the *last* startup). See `docs/phase-7-2-audit.md`.

/** One reference recording named in the acoustic manifest. */
export interface AcousticEnrollment {
  songId: string;
  contentId: string;
  audioPath: string;
}
