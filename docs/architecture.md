# Architecture

## Purpose

CIP assists a live church service in real time: it listens for spoken
scripture references, resolves them against a Bible translation, and
proposes them for on-screen presentation. Every proposal is a suggestion, a
human operator approves, edits, or rejects it - the system assists, it does
not replace the operator's judgment.

## Principles

1. **Local-first.** The app must fully function with no network at all. A
   single SQLite file on local disk is the source of truth; there is no
   required cloud database, no Supabase, no external database server.
2. **Internet-enhanced, never internet-required.** `integrations/web` may
   add optional network-backed features. Nothing in `core` may assume one
   is reachable, and no feature may degrade to non-functional offline.
3. **AI-native, human-controlled.** AI (speech recognition, scripture
   detection, classifiers) produces `Suggestion`s. A suggestion starts
   `pending` and only moves to `approved` / `edited` / `rejected` through
   an explicit human action - see `core/ai::Suggestion`. Nothing auto-applies
   based on confidence alone, regardless of how high that confidence is.
4. **Domain-oriented, not layer-oriented.** Code is organized by what it's
   *about* (bible, service, presentation, ...), not by technical role
   (models, controllers, ...). Each domain in `core/` is a separate crate
   with its own `Cargo.toml`, so a dependency from one domain to another is
   a real, visible, compiler-enforced dependency edge - not an accident of
   folder layout.
5. **Provider/adaptor for anything external.** `core` never imports a
   specific database driver, AI SDK, or third-party API client directly.
   It defines a trait (`BibleProvider`, `AudioEngine`, `SpeechEngine`,
   `SearchEngine`); `integrations/*` and `ai/*` provide implementations.
   Swapping a local Bible database for an online translation API, or a
   local speech model for a cloud one, should never require a change to
   `core` or the UI.

## Domains (`core/`)

| Crate                  | Owns                                                             |
| ------------------------ | ------------------------------------------------------------------ |
| `core/bible`            | `BibleProvider`, `ScriptureReference`, text normalization, reference detection, verse-range retrieval, local search, the dataset integrity checker, the Scripture Context Manager (see below) |
| `core/content`          | `ContentRegistry` - what local content exists, and its provenance/licensing (Phase 1.5) |
| `core/intelligence`     | The shared intelligence architecture (Phase 2.0) - `IntelligenceContext`, `IntelligenceEngine`, `IntelligenceFinding`, the engine registry, the Bible compatibility adapter, the Music adapter (Phase 2.1, extended with acoustic fusion in Phase 2.2), the Sermon adapter (built under an earlier internal "Phase 2.3" label, extended in place as this repository's authoritative Phase 2.6 - see [`docs/sermon-intelligence.md`](sermon-intelligence.md)), the cross-domain correlation rule engine (built under an earlier internal "Phase 2.4" label - see the roadmap note below) - see [`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md), the Service Intelligence adapter, this repository's authoritative Phase 2.4 - see [`docs/service-intelligence.md`](service-intelligence.md), and `ContentCandidate`/`ContentIntelligenceEngine`, this repository's authoritative Phase 2.7 - see [`docs/content-intelligence.md`](content-intelligence.md) |
| `core/music`            | Song/lyric domain model (`Song`, `SongSection`, `LyricLine`), `MusicProvider`, deterministic title/alias/number/lyric matching (Phase 2.1); `AcousticMusicRecognizer` trait, audio segmentation, signal-quality gate, evidence fusion (Phase 2.2) |
| `core/service`          | `ServiceSession` lifecycle, `AudioEngine` capture contract        |
| `core/ai`               | `SpeechEngine` transcription contract, `Suggestion`                |
| `core/presentation`     | `PresentationItem` - *what* is shown, not how it's rendered       |
| `core/search`           | `SearchEngine` - a single source-agnostic query contract          |
| `core/confidence`       | `ConfidenceResult` - shared by every domain that produces an uncertain, AI-derived result |
| `core/sermon`           | Sermon taxonomy, deterministic phrase-anchored structural/theme detection, sermon-state inference (built under an earlier internal "Phase 2.3" label, extended in place under this repository's authoritative Phase 2.6 with Takeaway/FoodForThought detection, a logistics-question filter, and a state->Phase-2.5-section candidate mapping); its `foundation` submodule: the Sermon entity/lifecycle/section/speaker/segment model, this repository's authoritative Phase 2.5 - see [`docs/sermon-foundation.md`](sermon-foundation.md) and [`docs/sermon-intelligence.md`](sermon-intelligence.md) |

> **Roadmap note.** This repository's authoritative Phase 2 roadmap is:
> 2.0 Intelligence Architecture -> 2.1 Unified Intelligence Event/Context
> Layer -> 2.2 Music Content Foundation -> 2.3 Music Intelligence -> **2.4
> Service Intelligence** -> **2.5 Sermon Intelligence Foundation** ->
> **2.6 Sermon Intelligence** -> **2.7 Content Intelligence** -> **2.8
> Cross-Domain Intelligence** -> 2.9 Unified Operator Intelligence
> Workspace -> 2.10 Full Phase 2 Validation. The cross-domain correlation
> rule engine referenced above, and `core/sermon`'s own semantic detection
> modules (`detection`/`state`/`structure`/`taxonomy`/`theme`), were both
> built under earlier, internal phase labels ("Phase 2.4" and "Phase 2.3"
> respectively) before this roadmap was adopted; those labels are
> historical artifacts and are not rewritten. The cross-domain engine is
> reserved for formal validation under the roadmap's actual Phase 2.8; the
> semantic sermon detection modules were subsequently extended in place as
> the roadmap's actual Phase 2.6 (see [`docs/sermon-intelligence.md`](sermon-intelligence.md)),
> with the roadmap's actual Phase 2.5 being the separate `foundation`
> submodule referenced above.

`core/confidence` is the one crate every other domain may depend on; the
rest do not depend on each other except through two documented
composition points: `core/service` (composing `core/bible` + `core/ai`
into the Bible Intelligence Core pipeline) and, one level up the same
stack, `core/intelligence` (Phase 2.0, extended in Phase 2.1 and 2.3),
which composes `core/bible` + `core/ai` + `core/service` + `core/content`
+ `core/music` + `core/sermon` into the shared
`IntelligenceContext`/`IntelligenceEngine` contracts - reusing
`ScriptureContext`/`TranscriptSegment`/`ServiceStatus`/`ContentMetadata`
exactly rather than duplicating them. `core/music` and `core/sermon` each
depend on nothing beyond `core/confidence`/`serde`/`regex` (see
[`docs/music-intelligence.md`](music-intelligence.md#offline-guarantee)
and
[`docs/sermon-intelligence.md`](sermon-intelligence.md#offline-guarantee)),
matching every other domain crate's rule below. See
[`docs/intelligence-architecture.md`](intelligence-architecture.md) for
why this dependency shape was chosen.

## Provider/adaptor implementations

- `integrations/bible` - Phase 1 ships one `BibleProvider`: a local
  SQLite-backed implementation (`SqliteBibleProvider`), proving the
  contract end to end. Phase 1.5 adds the reusable local Bible dataset
  importer alongside it (`import_bible_dataset`) - see
  [`docs/bible-datasets.md`](bible-datasets.md). `integrations/web`,
  `integrations/obs`, `integrations/vmix` are placeholders for later
  phases.
- `integrations/music` (Phase 2.1) - the one `MusicProvider`
  implementation, `SqliteMusicProvider`, plus the reusable local music
  dataset importer (`import_music_dataset`) - see
  [`docs/music-datasets.md`](music-datasets.md).
- `integrations/music-acoustic` (Phase 2.2) - `AcousticMusicRecognizer`
  implementations: `NullAcousticMusicRecognizer` (the safe default),
  `ScriptedAcousticMusicRecognizer` (deterministic test/demo adapter),
  and `LocalAcousticMusicRecognizer` (the real local-model integration
  boundary - honestly reports `Unavailable` when no model is
  configured, since no acoustic inference backend is implemented in
  this build) - see [`docs/acoustic-music.md`](acoustic-music.md).
- `integrations/content` - Phase 1.5's one `ContentRegistry`
  implementation, `SqliteContentRegistry`, mirroring
  `integrations/bible`'s shape. See
  [`docs/content-registry.md`](content-registry.md).
- `integrations/audio` - Phase 1.2's `AudioEngine` implementation
  (`CpalAudioEngine`, over the cross-platform `cpal` crate).
- `ai/speech`, `ai/embeddings`, `ai/classifiers` - AI backend
  implementations. Phase 1.2 ships three `SpeechEngine`s:
  `NullSpeechEngine` (the safe default), `ScriptedSpeechEngine`
  (deterministic test/demo adapter), and `WhisperSpeechEngine` (a real
  local backend over whisper-rs/whisper.cpp, behind a `whisper` Cargo
  feature) - see [`docs/live-speech.md`](live-speech.md).
  `ai/models` is not a crate - it's where local model artifacts are placed
  at runtime (never committed to the repository).
- `presentation/renderer` - turns a `PresentationItem` into on-screen
  output. Kept separate from `core/presentation` so the AI/suggestion
  pipeline never couples directly to the renderer. Phase 1 ships only
  `NullRenderer`; Phase 1.4 adds a real, deterministic
  `render_content()` (no AI generation, no randomness) producing a
  structured `RenderedSlide` via the one `SCRIPTURE_DEFAULT` template -
  see [`docs/presentation.md`](presentation.md). `presentation/templates`
  and `presentation/outputs` remain reserved directories for the future
  presentation designer (visual/typographic design beyond that one
  template, plus real display output).

## Bible Intelligence Core & Scripture Context Manager

Implemented in Phase 1.1: `core/bible::ScriptureContextManager`'s interface
boundary (established in Phase 1.0) now has a real implementation,
`DefaultScriptureContextManager`, and `core/service`'s
`process_transcript_segment` composes it with detection, `BibleProvider`
validation, and `Suggestion` creation into the full transcript-to-suggestion
pipeline:

```
Pastor: "Turn with me to Romans chapter 8." -> ACTIVE SCRIPTURE CONTEXT = Romans 8
Pastor: "verse 28"             -> resolves to Romans 8:28
Pastor: "verse 31"              -> resolves to Romans 8:31
Pastor: "go back to verse 18"   -> resolves to Romans 8:18
```

See [`docs/bible-intelligence.md`](bible-intelligence.md) for the full
pipeline, the context model, ambiguity handling, and the transcript test
harness. Phase 1.2 connects this to a real live-service input path - real
audio capture, a replaceable speech-to-text boundary, persistence, IPC/
event wiring, and an operator UI - without changing any of the above; see
[`docs/live-speech.md`](live-speech.md). Phase 1.3 adds the operational
layer around that pipeline - service lifecycle, a timeline, operator
ambiguity resolution and context correction, suggestion deduplication,
and failure recovery - again without changing the Bible Intelligence Core
itself; see [`docs/live-service.md`](live-service.md). Phase 1.4 connects
an approved suggestion to a real, prepared presentation output - real
local Bible text, a deterministic renderer, and separate preview/prepare
actions - again without changing the Bible Intelligence Core, the
detection pipeline, or the operator approval boundary; see
[`docs/presentation.md`](presentation.md). Phase 1.5 builds the content/
dataset foundation underneath all of the above - the Content Registry,
the Bible dataset importer/integrity checker, verse-range retrieval, and
local search - again without changing the Bible Intelligence Core's own
detection/context/resolution behavior; see
[`docs/bible-datasets.md`](bible-datasets.md) and
[`docs/content-registry.md`](content-registry.md). Phase 2.0 wraps this
same, unchanged pipeline in a thin `IntelligenceEngine` compatibility
adapter (`core/intelligence::bible_adapter::BibleIntelligenceEngine`) so
it can sit behind the new shared intelligence architecture alongside
future Music/Sermon/Content engines - `core/bible` and `core/service`
were not modified, and every existing Bible Intelligence Core test still
passes unmodified; see
[`docs/intelligence-architecture.md`](intelligence-architecture.md).
Phase 2.1 registers the first of those future engines for real:
`core/intelligence::music_adapter::MusicIntelligenceEngine`, backed by
the new `core/music`/`integrations/music` crates, proving two
independent engines can share one `IntelligenceContext` without ever
calling each other - again without touching `core/bible`, `core/service`,
or `bible_adapter.rs`; see
[`docs/music-intelligence.md`](music-intelligence.md). Phase 2.2 adds a
second, real recognition path to that same Music engine - acoustic
(audio-fingerprint) recognition, fused with the existing lyric/title
path via `core/music::fusion` - again without touching `pipeline.rs`,
`core/bible`, or `bible_adapter.rs`; see
[`docs/acoustic-music.md`](acoustic-music.md). Phase 2.3 registers a third
engine, `core/intelligence::sermon_adapter::SermonIntelligenceEngine`,
backed by a new pure-domain `core/sermon` crate (deterministic sermon
taxonomy/structure/theme detection, no dependency on `core/intelligence`
or any other domain crate) - proving a third independent engine shares the
same `IntelligenceContext`/registry/failure-isolation architecture without
touching `pipeline.rs`, `core/bible`, `bible_adapter.rs`, `core/music`, or
`music_adapter.rs`; see
[`docs/sermon-intelligence.md`](sermon-intelligence.md). Phase 2.4 adds
no fourth engine - `CrossDomainCorrelationEngine` deliberately does not
implement `IntelligenceEngine` and is never registered into the registry,
since it reads findings *across* domains rather than producing them for
one; it reads the same shared `IntelligenceContext` every registered
engine does, and calls none of Bible/Music/Sermon directly. See
[`docs/cross-domain-intelligence.md`](cross-domain-intelligence.md).

## Event architecture

CIP uses Tauri's built-in event system (`AppHandle::emit` /
`@tauri-apps/api/event`'s `listen`) as its event bus - no bespoke pub/sub
was introduced. Event *names* have one typed source of truth on each side:
`apps/desktop/src-tauri/src/events.rs` (Rust `AppEvent` enum) and
`apps/desktop/src/events/eventNames.ts` (TypeScript `AppEvents` object),
kept in sync by hand since both are small. The Phase 1 event set:

```
AUDIO_STARTED, AUDIO_STOPPED, TRANSCRIPT_UPDATED
SCRIPTURE_DETECTED, SCRIPTURE_UPDATED, SCRIPTURE_CONFIRMED, SCRIPTURE_REJECTED
SUGGESTION_CREATED, SUGGESTION_APPROVED, SUGGESTION_EDITED, SUGGESTION_REJECTED
PRESENTATION_PREPARED, PRESENTATION_STARTED, PRESENTATION_STOPPED
SERVICE_STARTED, SERVICE_PAUSED, SERVICE_ENDED
```

Phase 1.3 added six more variants to the same enum - `ServiceResumed`,
`SpeechStarted`/`SpeechStopped`, `ErrorOccurred`,
`ScriptureContextCorrected`, `ScriptureAmbiguousResolved` - still no
second event bus. It also reuses Phase 1.0's previously-unused
`audit_events` table as the service timeline's storage
(`apps/desktop/src-tauri/src/timeline.rs`): every meaningful event is
both emitted live (the mechanism above) and persisted as one
`audit_events` row, so the timeline is reconstructable after a restart
without a redundant table. See [`docs/live-service.md`](live-service.md#service-timeline).

Phase 1.4 added two more - `PresentationPreviewed` and
`PresentationCancelled` - alongside the already-existing
`PresentationPrepared`/`PresentationStarted`/`PresentationStopped`.
`PresentationStarted`/`PresentationStopped` remain declared but unused,
reserved for a future real display integration: nothing in this codebase
emits them, since nothing can actually display prepared content yet. See
[`docs/presentation.md`](presentation.md).

## Configuration

`apps/desktop/src-tauri/src/config.rs`'s `AppConfig` resolves, from Tauri's
app data directory, everything the app needs to run: `data_dir`,
`database_path`, `model_dir`, `log_dir`, and an `environment`
(`development` / `test` / `production`, resolved from the `CIP_ENV`
environment variable, falling back to the build profile). No secrets are
read or stored here - a networked integration is responsible for its own
credential storage, out of scope for Phase 1.

## Logging & error handling

Every log call site logs against one of eleven categories
(`apps/desktop/src-tauri/src/logging.rs::LogCategory`): `App`, `Database`,
`Audio`, `Speech`, `Bible`, `Ai`, `Presentation`, `Content`, `Network`,
`Security`, `Error`. `apps/desktop/src-tauri/src/errors.rs::AppError` is the single
error type every Tauri command returns; it wraps each domain's own error
type and knows which category it belongs to, so command dispatch logs
consistently before the error crosses the IPC boundary.

## Boundaries

- `core/*` crates depend on `cip-core-confidence` and (for `core/service`,
  `core/ai`, `core/presentation`) each other's public types by id
  reference, never on `integrations/*`, `ai/*`, `presentation/renderer`,
  `database`, or `tauri`. `core/content` follows the same rule - it
  defines `ContentRegistry` and `ContentMetadata` with no dependency on
  `core/bible` or any other domain; the `"bible:<id>"` naming convention
  that links a Bible translation to its registry entry lives in the
  composition layer (`apps/desktop/src-tauri/src/content.rs`), not in
  either domain crate.
- `core/intelligence` (Phase 2.0, extended in Phase 2.1) is the one
  exception alongside `core/service`: it depends on `core/bible`,
  `core/ai`, `core/service`, `core/content`, and (as of Phase 2.1)
  `core/music` directly, reusing their types
  (`ScriptureContext`/`TranscriptSegment`/`ServiceStatus`/`ContentMetadata`)
  rather than duplicating them - the same "one documented composition
  point per layer" pattern `core/service` already established, one level
  up the same stack. It still depends on nothing outside `core/*` and
  `cip-core-confidence` - no `tauri`, no SQLite implementation, no
  network client. See
  [`docs/intelligence-architecture.md`](intelligence-architecture.md#23-offline-operation).
  `core/music` itself follows the ordinary domain-crate rule above - it
  depends only on `cip-core-confidence`, nothing else in `core/*`.
- `integrations/*` and `ai/*` depend on `core/*` (to implement its
  traits) and on `database` where they need storage - never the reverse.
- `apps/desktop/src-tauri` is the only crate allowed to depend on `tauri`
  directly. It composes `core`, `database`, and `integrations/*` behind
  Tauri commands; no domain crate knows Tauri exists.
- Nothing depends on Supabase, Firebase, or any other cloud SDK. Nothing
  in `core` may hard-code Bible content (it comes from a `BibleProvider`)
  or a specific AI provider (it comes from a `SpeechEngine`).
