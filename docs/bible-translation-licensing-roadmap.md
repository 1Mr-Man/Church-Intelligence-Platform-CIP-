# Bible Translation Licensing Roadmap

This document captures the licensing strategy research supplied for CIP
(a comparison of YouVersion Platform, API.Bible, and direct publisher
licensing) as a concrete, phased roadmap against the [Bible Translation
Registry](bible-translation-registry.md) this codebase already enforces.
It names real routes and real contacts; it does not claim any of them
have been pursued yet. See [`docs/phase-9-audit.md`](phase-9-audit.md)
for why this phase implemented only the registry/enforcement half (Phase
A below) and not the platform/publisher outreach.

## The three routes, compared

| Requirement | YouVersion Platform | API.Bible | Direct publisher license |
|---|---|---|---|
| Non-commercial CIP | Strong candidate | Yes | Yes |
| Commercial CIP | Restricted - Platform apps must be non-commercial (no ads/paywalls/subscriptions) | Pro/Enterprise tiers available | Case-by-case, negotiated |
| Offline caching | Governed by that app's specific license terms | Governed by that app's specific license terms | Negotiated directly |
| NIV | Available under YouVersion's own terms | Not available commercially through API.Bible Express Licensing - Biblica must be contacted directly | Biblica |
| ESV | Available under YouVersion's own terms | Not available - Crossway's own ESV API is the official route | Crossway |
| Nigerian languages (Yoruba/Igbo/Hausa) | Reported available (Bible Society of Nigeria versions) - must be verified for CIP's specific registered app | Must be verified against API.Bible's actual catalog | Bible Society of Nigeria, direct |
| AI/ML use of the text | Must be checked per-license; several major publishers (e.g. Biblica) require this be explicitly granted | Restricted on copyrighted text without a license that expressly permits it | Negotiated - must ask explicitly |

Nothing in this table has been verified against a live account, an
actual API key, or a real conversation with any of these organizations
in this codebase's development process. It is planning input, not a
license record - the Bible Translation Registry's `LicensingStatus`
never treats research like this as evidence a translation may be
imported (see that document's own admission-gate discipline).

## Translation priority list

**Tier 1 - essential English**: KJV, NKJV, NIV, ESV, NLT, NASB 2020.

**Tier 2 - Nigeria priority**: Yoruba, Igbo, Hausa - named as a
deliberate differentiator, since CIP is being developed with Nigerian
churches as an important target audience, and the Bible Society of
Nigeria is reported to already be involved in translating, publishing,
and distributing Scripture in these languages.

## Per-translation licensing route

| Translation | Rights holder | Route | Registry status today |
|---|---|---|---|
| KJV | Public domain in the US; UK Crown rights may differ | A carefully sourced, independently verified public-domain dataset, same evidence-chain standard as BSB (Path A, `bible-translation-registry.md`) | Not imported - the current `KJV` id is a small Phase 1.2 dev/test fixture, not a real complete dataset |
| NKJV | Thomas Nelson / HarperCollins Christian Publishing | Direct software/digital permission request - explicitly naming Windows desktop use, church projection, offline caching, commercial distribution, and Nigeria + international territories | `Unknown` - not requested |
| NIV | Biblica | Express Licensing (YouVersion/API.Bible) only covers non-commercial, non-AI use; CIP's AI/speech-intelligence features and commercial ambitions require Biblica's own full permission process, which requires a registered legal entity | `Unknown` - not requested |
| ESV | Crossway | Crossway's digital permissions process (crossway.org/permissions/digital) for full-text/offline use; the official ESV API covers only a non-commercial prototype | `Unknown` - not requested |
| NLT | Tyndale | Direct permissions request (tyndale.com/permissions) - ordinary quotation limits do not cover a bundled full-text database | `Unknown` - not requested |
| NASB 2020 | The Lockman Foundation | Direct permission request (lockman.org) - their own form explicitly asks about the target platform (Windows/website/mobile), which fits CIP's exact use case cleanly | `Unknown` - not requested |
| Yoruba / Igbo / Hausa | Bible Society of Nigeria | A proposed digital-integration partnership, contacted directly rather than assumed available via YouVersion's catalog | `Unknown` - not requested |

## The roadmap

### Phase A - build the registry and enforcement (this phase, done)

Per `docs/phase-9-audit.md`: `UsagePermissions` on `ContentMetadata`/
`content_registry`, a real enforcement point
(`commands::ensure_ai_processing_permitted`, wired into
`generate_verse_embeddings`), and BSB's own real usage permissions
recorded as evidence. Bible detection itself
(`core/bible`/`core/service`) remains entirely independent of which
translations are licensed - it always operates on whatever's actually
in the local database, exactly as this roadmap's own architecture
requires (a licensed translation is a *provider* concern, never a
*detection* concern).

### Phase B - apply for platforms (not started; requires real accounts)

1. Register a real CIP application with YouVersion Platform
   (developers.youversion.com) and read the exact license terms that
   attach to the resulting App Key - in particular, whether CIP's
   AI/speech-intelligence features are compatible with YouVersion's
   non-commercial requirement, or whether a Ministry (free, no ads, no
   subscription) edition of CIP is the only compatible shape.
2. Register a real CIP application with API.Bible (docs.api.bible) and
   evaluate the Starter vs. Pro vs. Enterprise tiers against CIP's
   actual commercial plans, and check its live catalog for the
   Nigerian-language translations this roadmap prioritizes.
3. Contact the Bible Society of Nigeria (biblesociety-nigeria.org)
   directly about Yoruba/Igbo/Hausa digital integration - do not assume
   YouVersion catalog availability implies permission to extract and
   bundle that text into CIP.

Nothing in Phase B can be performed by an autonomous coding session with
no real API key and no authority to register a legal entity or sign
terms of service on the user's behalf.

### Phase C - direct publisher licensing (not started; requires real correspondence)

In priority order: NIV (Biblica), NKJV (Thomas Nelson/HarperCollins),
NLT (Tyndale), NASB (Lockman), ESV (Crossway). Each request should
explicitly state: Windows desktop application, live church projection,
Scripture search, offline cache/storage, full-Bible access, whether
commercial distribution is intended, and target territories (including
Nigeria). Every one of these publishers' own permission pages requires a
human decision-maker and cannot be initiated by this codebase itself.

## What "done" looks like for a new translation

A translation only leaves `Unknown` in the registry once real evidence
exists - either Path A (an independently verified public-domain/
permissive source, `docs/bible-translation-registry.md`) or Path B (an
actual signed license/permission on file, naming grantor, scope, and
explicit AI/ML boundaries). This roadmap names the routes; it is not
itself evidence for any of them.
