# Church Intelligence Platform (CIP)

CIP is a desktop application that assists a live church service: as
scripture is spoken, it detects the reference, brings up the verse text,
and queues it for presentation - reviewed and approved by a human operator,
never auto-applied. It is being built in phases; this repository currently
contains **Phase 1 - Foundation**, **Phase 1.1 - Bible Intelligence Core**,
**Phase 1.2 - Live Speech Foundation**, **Phase 1.2.1 - Runtime
Compatibility**, **Phase 1.3 - Live Service Intelligence & Operator
Workflow**, **Phase 1.4 - Presentation Foundation & Real-Service
Validation**, **Phase 1.5 - Content/Dataset Foundation &
Full-Service Validation**, and **Phase 2.0 - Intelligence Architecture &
Unified Intelligence Context**.

## Approved architecture

- **Local-first & offline-capable.** No required cloud database, no
  Supabase, no external database server. Every install owns a single local
  SQLite file.
- **Internet-enhanced, never internet-required.** Optional integrations
  may use the network to enhance the experience; nothing in `core` may
  assume one is present.
- **AI-native, human-controlled.** AI produces suggestions; only a human
  action approves, edits, or rejects them. Nothing auto-applies based on
  confidence alone.
- **Desktop-first.** Tauri + React + TypeScript frontend, Rust backend.
- **Domain-oriented.** Business logic is organized by domain (`core/*`),
  not by technical layer, and depends on external systems only through
  provider/adaptor traits (`integrations/*`).

See [`docs/architecture.md`](docs/architecture.md) for the full picture,
[`docs/bible-intelligence.md`](docs/bible-intelligence.md) for the
transcript-to-suggestion pipeline, [`docs/live-speech.md`](docs/live-speech.md)
for real audio capture, the speech-to-text boundary, and the Live Church
Brain UI, [`docs/live-service.md`](docs/live-service.md) for the service
lifecycle and operator workflow built around that pipeline,
[`docs/presentation.md`](docs/presentation.md) for the presentation
preparation path from an approved suggestion to persisted, prepared
output, [`docs/content-registry.md`](docs/content-registry.md) for what
local content exists and its provenance/licensing,
[`docs/bible-datasets.md`](docs/bible-datasets.md) for the Bible dataset
importer/integrity checker and the licensing policy governing them,
[`docs/full-service-validation.md`](docs/full-service-validation.md) for
the realistic full-service validation results,
[`docs/intelligence-architecture.md`](docs/intelligence-architecture.md)
for the shared intelligence contracts future Bible/Music/Sermon/Content
engines are built on,
[`docs/development.md`](docs/development.md) to get running locally, and
[`docs/database.md`](docs/database.md) for the SQLite/migration story.

## What's implemented (and what isn't)

**Phase 1 (Foundation)** established the application skeleton: the desktop
shell, the domain-oriented crate layout, the local SQLite schema and
migration system, typed domain contracts (`BibleProvider`, `AudioEngine`,
`SpeechEngine`, `SearchEngine`, `Suggestion`, `PresentationItem`,
`ServiceSession`, `ConfidenceResult`), the event architecture,
configuration, logging, and the Scripture Context Manager's interface
boundary.

**Phase 1.1 (Bible Intelligence Core)** implemented that interface
boundary for real: transcript text normalization, deterministic scripture
reference detection, the `ScriptureContextManager` (so "verse 28" resolves
against whatever chapter the pastor named, even across unrelated
intervening speech), Bible-validated reference resolution, confidence
scoring, and `Suggestion` creation - all driven by a deterministic
transcript-input test harness, with no real audio or speech model
involved. See [`docs/bible-intelligence.md`](docs/bible-intelligence.md).

**Phase 1.2 (Live Speech Foundation)** connects that pipeline to a real
live-service input path: a real cross-platform `AudioEngine`
(`integrations/audio::CpalAudioEngine`, over `cpal`), a replaceable
`SpeechEngine` boundary with three implementations (`NullSpeechEngine`,
`ScriptedSpeechEngine` for deterministic testing, and a real local
`WhisperSpeechEngine` behind a `whisper` Cargo feature), transcript/
detection/suggestion persistence, Tauri IPC and event wiring, a manual
text-entry fallback, online/offline and AI-availability status reporting,
and a v0.1 "Live Church Brain" operator UI. See
[`docs/live-speech.md`](docs/live-speech.md), including the documented
model-download blocker in this development environment and how to verify
real transcription with network access to a model host.

**Phase 1.2.1 (Runtime Compatibility & Web Fallback)** made the frontend
runtime-aware: this same build can be deployed as a static web app (e.g.
Vercel) with no Tauri backend behind it. Every Tauri IPC call and event
subscription now checks `isTauriRuntime()` first, so opening the web
deployment in an ordinary browser shows a clear "Web Runtime" notice
instead of the raw `TypeError` a bare `invoke()` call previously threw
outside Tauri. See [`docs/live-speech.md`](docs/live-speech.md#cip-web-vs-cip-desktop-phase-121).

**Phase 1.3 (Live Service Intelligence & Operator Workflow)** turned that
pipeline into a reliable, operator-controlled live-service tool: a full
service lifecycle (start/pause/resume/end, with duplicate-start and
invalid-transition protection), a service timeline reusing the existing
`audit_events` table, session-scoped suggestion deduplication, operator
ambiguity resolution and manual context correction (both validated and
audited), edit validation against the `BibleProvider`, audio/speech/
database failure recovery that keeps the service live, a service history
archive, and a refit "Live Church Brain" operator workspace (confidence-
grouped suggestion queue, current/recent/history views that never
interfere with each other, guarded keyboard shortcuts). See
[`docs/live-service.md`](docs/live-service.md), including its documented
scope decisions and the reasoning behind the deduplication/ambiguity/
failure-recovery policies.

**Phase 1.4 (Presentation Foundation & Real-Service Validation)** connected
that operator-approved pipeline to a real presentation preparation path:
`PresentationItem` now traces back to the suggestion (if any) it came
from and the template that rendered it, a deterministic
`SCRIPTURE_DEFAULT` renderer turns real, local-Bible-sourced content into
a structured slide, and separate Preview (non-mutating, available before
approval) and Prepare (approval-gated, persists) actions replace a
pre-1.4 UI bug where "Preview" silently called the approval-gated prepare
command. A manual creation path keeps presentation preparation working
with no suggestion, no speech engine, and no network. Preparation is
still never projection - nothing in this codebase can display prepared
content yet, and the phase proves as much: a detected Scripture cannot
automatically become a prepared item, and no code path ever sets a
presentation item to "active". See
[`docs/presentation.md`](docs/presentation.md).

**Phase 1.5 (Content/Dataset Foundation & Full-Service Validation)** built
the scalable local content layer underneath that pipeline and validated
the whole thing end to end against realistic transcripts. A new Content
Registry (`core/content`/`integrations/content`) answers "what local
content exists, and under what license?" for any content category
(Bible today; music/sermon/media/reference are the reserved shape for
later phases); a reusable, idempotent Bible dataset importer validates
and loads a structured local dataset (never bulk-downloading or scraping
a copyrighted translation); a dataset integrity checker distinguishes a
development fixture from a complete canonical dataset without hard-coding
Bible facts it can't verify; `core/bible` gained verse-range retrieval
and a translation-aware local search dispatcher (reference/chapter/range/
free-text, entirely offline); and a canonical full-service acceptance
test proves context retention, context replacement, false-positive
protection, operator overrides, and dataset-validation authority (an
out-of-range verse never produces a suggestion) all hold together in one
realistic scripted service. See
[`docs/content-registry.md`](docs/content-registry.md),
[`docs/bible-datasets.md`](docs/bible-datasets.md), and
[`docs/full-service-validation.md`](docs/full-service-validation.md).

**Phase 2.0 (Intelligence Architecture & Unified Intelligence Context)**
built the shared architecture future intelligence engines will sit
behind, without implementing any of them. A new `core/intelligence` crate
defines `IntelligenceDomain`/`FindingKind`/`FindingStatus`, the mandatory
observed/inferred/suggested/generated distinction, an evidence/provenance
model tied to the Phase 1.5 Content Registry, a deterministic (confidence
is not urgency) priority model, a bounded `IntelligenceContext` (proven to
stay bounded even fed 10,000 synthetic transcript segments), the
`IntelligenceEngine` contract, an in-process engine registry with
panic-safe failure isolation, a correlation-model foundation, and an
in-memory finding queue. The one real engine is a thin compatibility
adapter over the unchanged Bible Intelligence Core - `core/bible` and
`core/service` were not modified by this phase, and every existing Bible
regression test still passes unmodified. Music, Sermon, and Content
intelligence remain **not implemented** - `IntelligenceDomain` only
reserves the shape they will occupy. See
[`docs/intelligence-architecture.md`](docs/intelligence-architecture.md).

Still deliberately **not** implemented: song recognition, sermon
intelligence, semantic/paraphrase Bible search, automatic bullet
extraction, a web research engine, online Bible fallback, content
generation, cloud sync, OBS/vMix integration, remote operator accounts, a
mobile app, real display/projection output, and the full presentation
designer (visual/typographic design beyond one deterministic template).
Those are later phases.

## Repository layout

```
apps/desktop/          Tauri + React + TypeScript desktop application
  src/                 React/TypeScript frontend
  src-tauri/            Rust backend (Tauri commands, app shell)

core/                  Domain logic and contracts, one crate per domain
  bible/               BibleProvider, text normalization, reference detection, verse-range/search, integrity checker, Scripture Context Manager
  content/              ContentRegistry - what local content exists, and its provenance/licensing
  intelligence/          Shared intelligence architecture (Phase 2.0) - IntelligenceContext/Engine/Finding, the Bible compatibility adapter
  music/                (placeholder - Phase 2+ engine implementation)
  sermon/               (placeholder - Phase 2+ engine implementation)
  service/              ServiceSession + AudioEngine
  presentation/         PresentationItem
  search/               SearchEngine
  ai/                    SpeechEngine + Suggestion
  confidence/            ConfidenceResult (shared by every domain above)

database/              Local-first SQLite: migrations, schema docs, seeds
integrations/          Provider/adaptor implementations (bible - incl. dataset importer, content, audio, music, web, obs, vmix)
ai/                    AI backend implementations (speech, embeddings, classifiers, models)
presentation/          Rendering subsystem (renderer, templates, outputs)
tests/                 Cross-crate integration tests
docs/                  Architecture, setup, and reference documentation
```

Every `core/*` crate defines contracts, not implementations that reach out
to the OS, network, or a specific AI backend - those live in
`integrations/*` and `ai/*` and depend on `core`, never the other way
around. See [`docs/architecture.md`](docs/architecture.md#boundaries) for
the enforced boundaries.

## Quick start

```sh
pnpm install
pnpm --filter @cip/desktop tauri dev
```

See [`docs/development.md`](docs/development.md) for the full command
reference (typecheck, lint, Rust tests, database validation) and for what
each requires to be installed locally.
