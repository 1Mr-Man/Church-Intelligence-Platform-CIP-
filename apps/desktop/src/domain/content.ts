/**
 * Content Registry domain contracts (Phase 1.5). Mirrors
 * `core/content`/`cip_integrations_bible::importer`/
 * `cip_core_bible::integrity` (Rust).
 */

export type ContentType = "bible" | "music" | "service" | "media" | "reference";

export type ContentStatus = "enabled" | "disabled";

/**
 * What CIP has independently concluded about a content item's right to be
 * stored/redistributed (real Bible dataset production import milestone) -
 * distinct from `license`/`distribution` below, which only record what a
 * source *said*. A bulk importer refuses to write anything while this
 * would be `"unknown"` or `"restricted"`. See
 * `docs/bible-production-dataset.md`.
 */
export type LicensingStatus =
  | "verified_public_domain"
  | "verified_redistributable"
  | "licensed_for_cip"
  | "unknown"
  | "restricted";

/**
 * Fine-grained, per-use-case licensing permissions (Phase 9). Mirrors
 * `cip_core_content::UsagePermissions`. Every field is `null` by default -
 * "not yet determined" - and must never be treated as permissive; only an
 * explicit `true` permits anything. Distinct from `LicensingStatus` above,
 * which only governs whether a dataset may be bulk-imported at all - this
 * governs what CIP is allowed to *do* with a translation once admitted.
 * See `docs/bible-translation-registry.md`.
 */
export interface UsagePermissions {
  rightsHolder: string | null;
  sourceProvider: string | null;
  sourceUrl: string | null;
  attributionText: string | null;
  licenseStart: string | null; // ISO-8601
  licenseExpiry: string | null; // ISO-8601
  distributionAllowed: boolean | null;
  offlineStorageAllowed: boolean | null;
  projectionAllowed: boolean | null;
  apiAllowed: boolean | null;
  commercialAllowed: boolean | null;
  /** The one permission CIP's own embedding pipeline actually enforces
   * (`commands::ensure_ai_processing_permitted`) - `generate_verse_embeddings`
   * refuses to run unless this is explicitly `true`. */
  aiProcessingAllowed: boolean | null;
  llmPromptAllowed: boolean | null;
  trainingAllowed: boolean | null;
}

/**
 * Provenance/licensing metadata for one locally-installed content item.
 * Every field describing a real-world fact CIP cannot independently
 * verify is `string | null` - `null` means *unknown*, recorded honestly
 * rather than guessed. See `docs/bible-datasets.md`.
 */
export interface ContentMetadata {
  id: string;
  contentType: ContentType;
  name: string;
  version: string;
  language: string;
  source: string;
  publisher: string | null;
  copyright: string | null;
  license: string | null;
  distribution: string | null;
  importedAt: string; // ISO-8601
  checksum: string | null;
  status: ContentStatus;
  licensingStatus: LicensingStatus;
  usage: UsagePermissions;
}

/** A deterministic report of one dataset import call - every number is
 * derived from the actual dataset and database state, never hard-coded. */
export interface ImportReport {
  translationId: string;
  datasetVersion: string;
  books: number;
  chapters: number;
  versesTotal: number;
  imported: number;
  alreadyPresent: number;
  invalid: number;
  errors: string[];
  checksum: string;
  licensingStatus: LicensingStatus;
}

export type IntegrityStatus = "valid" | "incomplete" | "invalid";

export interface IntegrityIssue {
  description: string;
}

export interface IntegrityReport {
  translationId: string;
  status: IntegrityStatus;
  booksPresent: number;
  booksExpected: number;
  chaptersChecked: number;
  versesChecked: number;
  issues: IntegrityIssue[];
}

// --- Bible dataset import file format (docs/bible-datasets.md) -------------
//
// The shape `import_bible_dataset` expects as JSON text - mirrors
// `cip_integrations_bible::importer::BibleDatasetInput`. Kept here (not
// just in the Rust docs) so the frontend's import UI can validate a
// selected file's shape before ever sending it to the backend.

export interface BibleDatasetTranslationInput {
  id: string;
  name: string;
  abbreviation: string;
  language: string;
  publisher?: string | null;
  copyright?: string | null;
  license?: string | null;
  distribution?: string | null;
  datasetVersion: string;
  /** Required - the production safety gate rejects the whole import
   * (writing nothing) unless this is `"verified_public_domain"`,
   * `"verified_redistributable"`, or `"licensed_for_cip"`. */
  licensingStatus: LicensingStatus;
  /** Optional (Phase 9) - a dataset file predating this field, or one
   * that simply omits it, still imports fine; every permission defaults
   * to `null` ("not yet determined"). */
  usage?: UsagePermissions;
}

export interface BibleDatasetVerseInput {
  book: string;
  chapter: number;
  verse: number;
  text: string;
}

export interface BibleDatasetInput {
  translation: BibleDatasetTranslationInput;
  verses: BibleDatasetVerseInput[];
}
