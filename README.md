# Church Intelligence Platform (CIP)

CIP is a desktop application that assists a live church service: as
scripture is spoken, it detects the reference, brings up the verse text,
and queues it for presentation - reviewed and approved by a human operator,
never auto-applied. It is being built in phases; this repository currently
contains **Phase 1 - Foundation**, **Phase 1.1 - Bible Intelligence Core**,
and **Phase 1.2 - Live Speech Foundation**.

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
Brain UI, [`docs/development.md`](docs/development.md) to get running
locally, and [`docs/database.md`](docs/database.md) for the SQLite/migration
story.

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

Still deliberately **not** implemented: song recognition, sermon
intelligence, semantic/paraphrase Bible search, cloud sync, OBS/vMix
integration, and the full presentation designer. Those are later phases.

## Repository layout

```
apps/desktop/          Tauri + React + TypeScript desktop application
  src/                 React/TypeScript frontend
  src-tauri/            Rust backend (Tauri commands, app shell)

core/                  Domain logic and contracts, one crate per domain
  bible/               BibleProvider, text normalization, reference detection, Scripture Context Manager
  music/                (placeholder - Phase 2+)
  sermon/               (placeholder - Phase 2+)
  service/              ServiceSession + AudioEngine
  presentation/         PresentationItem
  search/               SearchEngine
  ai/                    SpeechEngine + Suggestion
  confidence/            ConfidenceResult (shared by every domain above)

database/              Local-first SQLite: migrations, schema docs, seeds
integrations/          Provider/adaptor implementations (bible, audio, music, web, obs, vmix)
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
