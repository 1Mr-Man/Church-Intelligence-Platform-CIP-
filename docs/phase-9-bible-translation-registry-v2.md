# Phase 9: Bible Translation Registry v2 — Licensing Metadata & Enforcement

## Baseline

Trigger: user-supplied licensing-strategy research (YouVersion Platform
vs. API.Bible vs. direct publisher licensing, a Nigeria-priority
translation list, and a recommended `UsagePermissions`-shaped schema and
`BibleProvider` capability surface), with the instruction "consider this
and 'keep going'" following Phase 8's completion. Full reasoning in
[`docs/phase-9-audit.md`](phase-9-audit.md).

The codebase already had a coarse admission gate
(`cip_core_content::LicensingStatus`, enforced in
`cip_integrations_bible::import_bible_dataset`) but nothing governing
*what CIP may do* with an already-admitted translation - concretely,
`commands::generate_verse_embeddings` sent every verse of a translation's
text into a local AI embedding model with zero licensing check, safe only
by accident (BSB, the sole translation ever embedded, happens to be
public domain).

## Design choices

See `docs/phase-9-audit.md`'s "Design choices" section in full. In
summary: a new `UsagePermissions` type lives alongside `LicensingStatus`
on `ContentMetadata`/`content_registry` (same home, different axis -
admission vs. use-case permission); every field defaults to `None`/
unknown, never permissive; the one enforcement point this phase wires is
`ai_processing_allowed`, gating `generate_verse_embeddings` fail-closed
(deliberately the opposite default from `ensure_translation_selectable`'s
fail-open, since AI processing needs affirmative evidence, not absence of
a denial); and, since this phase touches those exact call sites anyway,
it fixes a real pre-existing bug where `generate_verse_embeddings`/
`get_embedding_capabilities` hardcoded the `"KJV"` dev-fixture id instead
of `resolve_default_translation_id`, silently making embedding generation
a no-op against any real production (BSB-only) database.

## What was built

- **`core/content`**: `UsagePermissions` (14 fields: `rights_holder`,
  `source_provider`, `source_url`, `attribution_text`, `license_start`,
  `license_expiry`, and 8 `Option<bool>` permissions) + 8 `permits_*`
  helper methods, all reading only `Some(true)` as "yes." New `usage`
  field on `ContentMetadata`.
- **`database/migrations/0016_translation_usage_permissions.sql`**: 14
  new nullable columns on `content_registry`.
- **`integrations/content`**: `SqliteContentRegistry` reads/writes every
  new column, round-tripping `None`/`Some(true)`/`Some(false)` correctly.
- **`integrations/bible`**: `TranslationInput` gains an optional
  (`#[serde(default)]`) `usage: UsagePermissions` field - every dataset
  JSON written before this phase, or any future one that omits it, still
  parses.
- **`apps/desktop/src-tauri/src/content.rs`**: `import_and_register`
  threads `dataset.translation.usage` into the registered
  `ContentMetadata`, so real evidence supplied at import time actually
  reaches the registry.
- **`apps/desktop/src-tauri/src/commands.rs`**: new
  `ensure_ai_processing_permitted` (pure function, fail-closed) wired
  into `generate_verse_embeddings`; both `generate_verse_embeddings` and
  `get_embedding_capabilities` now resolve the real default translation
  instead of the hardcoded dev-fixture id.
- **`database/datasets/bsb/bsb.json`**: BSB's `translation.usage` now
  records real, evidence-backed permissions (every flag `true` except
  `training_allowed`, left `null` - CC0 was never evaluated against that
  specific use case). `docs/data/bible/BSB/manifest.json` documents the
  same.
- **Frontend**: `domain/content.ts` gains `UsagePermissions` and
  `ContentMetadata.usage`/`BibleDatasetTranslationInput.usage?`; contract
  test fixtures updated.
- **`docs/bible-translation-licensing-roadmap.md`** (new): the supplied
  research reframed as a named, actionable roadmap (translation priority
  list, per-translation route, Phase A/B/C staging) rather than
  fabricated progress - cross-linked from
  `docs/bible-translation-registry.md`.

## Testing boundary

New Rust tests: 3 in `core/content` (default/explicit-false/explicit-true
`permits_*` semantics), 2 in `integrations/content` (SQLite round-trip
including unset fields), 2 in `integrations/bible` (JSON
backward-compatibility with `usage` omitted, and explicit deserialization
with camelCase field names), 1 in `apps/desktop/src-tauri/content.rs`
(usage flows through `import_and_register`, not dropped), 3 in
`apps/desktop/src-tauri/commands.rs` (`ensure_ai_processing_permitted`:
unregistered → refused, registered-without-permission → refused,
explicitly-granted → allowed), 1 in `bible_production_dataset.rs` (the
real BSB asset declares real permissions, not a synthetic fixture).
Frontend: 2 existing `ContentMetadata` contract tests extended with
`usage` assertions.

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs (default and `--features whisper`).
- `cargo check --workspace` / `cargo check --features whisper`: clean.
- `cargo test --workspace`: 977 passed, 0 failed (default config).
- `cargo test --features whisper` (desktop crate): 320 passed, 0 failed.
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings,
  unchanged) / `npm run test -- --run` (266 passed, 0 failed) / `npm run
  build`: all clean.

## Architectural safety

- Zero new Tauri commands, zero new events. `generate_verse_embeddings`
  and `get_embedding_capabilities` are the only two command bodies
  changed, both already-existing commands.
- `core/bible`/`core/service`/Bible detection are entirely untouched -
  per the audit's design choice, licensing lives only in
  `ContentMetadata`/`content_registry`, never in the detection or
  provider-serving path.
- Every prior `ContentMetadata` construction site (8 files, 11 literals)
  updated to populate the new field explicitly - none silently defaulted
  by the compiler, all reviewed individually.
- The migration is purely additive (14 nullable `ALTER TABLE ADD COLUMN`
  statements) - no existing row's data changes shape or is rewritten.

## Windows rebuild

Required: this phase changes Rust code compiled into the desktop binary
(new migration, `ensure_ai_processing_permitted`, the
`generate_verse_embeddings`/`get_embedding_capabilities` fix). See
`pilot-evidence/9/windows/installer-contents-verification.json` and the
updated `release/windows/release-manifest.json` for direct binary proof.

## Known limitations (honest, not deferred silently)

- Only `ai_processing_allowed` is actually enforced. The other seven
  permissions (`distribution_allowed`, `offline_storage_allowed`,
  `projection_allowed`, `api_allowed`, `commercial_allowed`,
  `llm_prompt_allowed`, `training_allowed`) are recorded and queryable
  via `UsagePermissions::permits_*` but gate nothing yet - no other call
  path in this codebase currently does anything they would need to
  restrict.
- No operator-facing UI to view or edit a translation's `UsagePermissions`
  after import - today it is set only via `TranslationInput.usage` at
  import time, or left fully unset.
- No YouVersion/API.Bible network client code, no new Bible text of any
  kind, no Bible Society of Nigeria contact - see
  `docs/bible-translation-licensing-roadmap.md`'s Phase B/C, both
  explicitly out of this session's reach (no real API keys, no authority
  to register accounts or sign license terms).
- BSB's `training_allowed` is left `null` (unknown) rather than `true`,
  even though every other permission is `true` - a deliberate, narrower
  read of "public domain" than "permits literally everything," since the
  CC0 dedication's evidence chain was never specifically evaluated
  against model-training use.

## Final gate

Environment A (build-time verification, full regression, direct binary
symbol inspection): PASS. Environment C (a real operator exercising
`generate_verse_embeddings` against BSB on real Windows hardware, and
confirming it still succeeds now that the AI-processing gate is wired
in): not yet performed - carried forward into `physicalHardwareStatement`
per this project's standing discipline.
