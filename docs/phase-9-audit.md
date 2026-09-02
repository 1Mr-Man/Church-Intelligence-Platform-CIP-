# Phase 9 Audit — Bible Translation Registry v2: Licensing Metadata & Enforcement

## Trigger

The user supplied external research (a licensing-strategy comparison of
YouVersion Platform vs. API.Bible vs. direct publisher licensing, a
translation priority list including Yoruba/Igbo/Hausa via the Bible
Society of Nigeria, and a recommended `bible_translations` schema and
`BibleProvider` capability surface) and said "consider this and 'keep
going'" — instructing this to be folded into the project's next
autonomous phase, continuing the "don't stop until you finish all the
phases" directive from the Phase 8 session.

## What the advice actually recommends doing *now*

The advice's own final recommendation is explicit: **"Do not write CIP
code to a specific provider yet. First build the Translation Registry and
a provider-neutral interface."** Its own phased roadmap is:

- **Phase A — start now**: Translation Registry, provider abstraction,
  local/open/public-domain Bible support, license metadata enforcement.
- **Phase B — apply for platforms**: register with YouVersion Platform
  and API.Bible, evaluate their exact catalogs/terms for a registered
  CIP application.
- **Phase C — direct publisher licensing**: NIV (Biblica), NKJV
  (HarperCollins/Thomas Nelson), NLT (Tyndale), NASB (Lockman), ESV
  (Crossway), plus a Bible Society of Nigeria partnership for
  Yoruba/Igbo/Hausa.

Phase B and Phase C both require actions this session cannot perform:
registering a real legal entity/application with YouVersion or
API.Bible, obtaining real API keys, and corresponding with real rights
holders (Biblica, Thomas Nelson, Tyndale, Lockman, Crossway, the Bible
Society of Nigeria) to obtain actual signed licenses. Building network
client code against those APIs with no real credentials, or drafting
"license request" text and presenting it as sent, would be a simulation
dressed as a feature — this project's own established discipline
(`docs/bible-translation-registry.md`'s Path B: "this status can only be
entered by a human with an actual signed agreement in hand, never
inferred") forbids exactly that. **This phase therefore implements Phase
A only**, and documents B/C as a concrete, actionable roadmap rather than
attempting to fake them.

## What already exists (do not duplicate)

`docs/bible-translation-registry.md` (written in the prior session)
already names and documents: `BibleTranslation`, `content_registry`,
`LicensingStatus` (`VerifiedPublicDomain`/`VerifiedRedistributable`/
`LicensedForCip`/`Unknown`/`Restricted`), the hard import gate in
`import_bible_dataset`, and BSB's per-translation evidence folder. This
is the coarse **admission gate**: can a dataset be written to the
database at all. It is correct and untouched by this phase.

## The real gap the advice identifies

The advice's proposed schema goes further than admission — it wants
**per-use-case permission tracking**: a translation might be
distributable but not offline-cacheable, or displayable but not
sendable to an AI/LLM. CIP's `LicensingStatus` enum is coarse-grained
(one status governs "can this be bulk-imported," full stop) — it cannot
express "BSB may be projected and embedded, but a future
`LicensedForCip` NIV entry may be projected but never sent to an AI
model." Nothing in this codebase enforces any such distinction today —
and, concretely, `commands::generate_verse_embeddings` (Phase 4.4's
embedding pipeline) sends every verse of a translation's text into a
local embedding model with **zero licensing check at all**. That is a
real, live gap: it happens to be safe today only because the sole
translation ever embedded is BSB (public domain), not because anything
stops a future `LicensedForCip` translation whose agreement forbids
AI/ML use from being embedded anyway.

## Design choices (no genuine architectural fork; proceeding directly)

**1. Extend `ContentMetadata`, not `LicensingStatus`.** The advice's
granular flags (`offline_storage_allowed`, `ai_processing_allowed`,
`llm_prompt_allowed`, `commercial_allowed`, etc.) are a different axis
than the coarse admission gate, not a replacement for it — a
translation still needs `LicensingStatus::permits_bulk_import()` to
enter the database at all, and *then*, once admitted, its
`UsagePermissions` govern what CIP is allowed to do with it. Both live
on `ContentMetadata`/`content_registry` (already the one
licensing-metadata home for content, per the existing registry design;
`core/bible`'s `BibleProvider` stays purely about serving text, exactly
as the advice's own architecture diagram separates Bible detection from
the Translation Provider layer).

**2. New fields default to `None` ("not yet determined"), never to a
permissive value.** Every boolean permission is `Option<bool>`: `None`
means unknown (identical honesty discipline to `publisher`/`copyright`/
`license` already being `Option<String>`), `Some(false)` means
explicitly denied, `Some(true)` means explicitly granted. A permission
check must never treat `None` as `true`.

**3. Enforce it somewhere real, not just store it.** Storing 8 new
unenforced boolean columns would be paperwork, not a safety mechanism.
This phase wires `ai_processing_allowed` into the one real call path
that sends Bible text into an AI model today —
`generate_verse_embeddings` — as a hard, fail-closed gate: missing
registration or `ai_processing_allowed != Some(true)` refuses to
generate embeddings. This is deliberately the *opposite* default from
`ensure_translation_selectable` (which fails open for an unregistered
translation, so bookkeeping gaps never block browsing/searching/
displaying content already in the database) — AI processing is exactly
the class of action `LicensingStatus`'s own "never assume permissive"
doctrine was built for, so it must fail closed. The other seven
permission flags (distribution/offline/projection/api/commercial/
llm_prompt/training) are recorded and queryable via
`UsagePermissions::permits_*` but not wired into a second enforcement
point this phase — no other call path currently does anything they'd
need to gate (CIP has no LLM-prompt feature, no commercial billing path,
no external API surface today). Documented explicitly as recorded-but-
not-yet-enforced, not silently implied complete.

**4. Fix, not paper over, the `DEFAULT_TRANSLATION_ID` hardcoding in the
two functions this phase touches anyway.** `generate_verse_embeddings`
and `get_embedding_capabilities` both hardcode the literal `"KJV"` dev-
fixture id instead of using `resolve_default_translation_id` (the fix
`commands.rs` already applies at twelve other call sites per its own
documented Phase 3.7 root-cause finding). In a real production Windows
build, `"KJV"` is never registered — only `"BSB"` is — so today's
`generate_verse_embeddings` command is silently a no-op against any real
production database, and `get_embedding_capabilities`'s coverage count
always reports on a translation that was never installed. This is the
exact bug class Phase 3.7 already fixed at twelve other sites; fixing it
here (the same two lines already being edited for the licensing gate) is
a small, directly-justified correction, not scope creep.

**5. Populate BSB's own `UsagePermissions` with real evidence**, not a
synthetic test fixture only. BSB is public domain (CC0-equivalent,
per `docs/data/bible/BSB/BSB-LICENSE.md`) — every permission is
genuinely `true`. This is threaded through `TranslationInput` (the
existing importer input type, gaining one new `#[serde(default)]`
field so no existing dataset JSON breaks) into the compiled-in
`database/datasets/bsb/bsb.json` asset, so the one real production
translation in this codebase actually satisfies the new AI-processing
gate — proving the enforcement is real, not merely passing a synthetic
test.

## Testing boundary

New Rust unit tests: `UsagePermissions` default/round-trip/`permits_*`
behavior (`core/content`), SQLite column round-trip for every new field
including `None`/`Some(true)`/`Some(false)` (`integrations/content`),
the `ensure_ai_processing_permitted` gate (pure function, real in-memory
SQLite registry, no `tauri::test` harness — mirrors
`resolve_default_translation_id`'s existing split of pure/testable core
behind a thin `State`-based command wrapper), and the BSB dataset asset
itself declaring `ai_processing_allowed = true`
(`bible_production_dataset.rs`, extending its existing
`the_embedded_asset_parses_and_declares_verified_public_domain_licensing`
test). Frontend: `ContentMetadata`/`BibleDatasetTranslationInput`
contract tests updated for the new field.

## What this phase explicitly does NOT do (deferred, honestly)

- No YouVersion Platform or API.Bible network client code — no real API
  key exists for either, and writing a client with no credentials to
  test against would be unverifiable scaffolding, not a feature.
- No NIV/NKJV/NLT/NASB/ESV text of any kind — all remain `Unknown`/not
  imported; this phase changes zero bytes of actual Bible text.
- No Yoruba/Igbo/Hausa dataset or Bible Society of Nigeria contact — a
  real partnership conversation this session cannot initiate.
- No new UI for editing `UsagePermissions` from the operator's own
  screen — today it is set only at import time (`TranslationInput.usage`)
  or left `None`; an operator-facing editor is a reasonable future
  addition, not attempted here to keep this phase's diff focused on the
  enforcement mechanism itself.
- The translation-priority roadmap (Tier 1 English six, Tier 2 Nigerian
  three) and the YouVersion-vs-API.Bible comparison are captured as a
  planning document (`docs/bible-translation-licensing-roadmap.md`), not
  as code — there is nothing to implement yet for a translation whose
  rights nobody has secured.
