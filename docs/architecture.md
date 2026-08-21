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
| `core/bible`            | `BibleProvider`, `ScriptureReference`, text normalization, reference detection, the Scripture Context Manager (see below) |
| `core/service`          | `ServiceSession` lifecycle, `AudioEngine` capture contract        |
| `core/ai`               | `SpeechEngine` transcription contract, `Suggestion`                |
| `core/presentation`     | `PresentationItem` - *what* is shown, not how it's rendered       |
| `core/search`           | `SearchEngine` - a single source-agnostic query contract          |
| `core/confidence`       | `ConfidenceResult` - shared by every domain that produces an uncertain, AI-derived result |
| `core/music`, `core/sermon` | Placeholders reserving the architectural boundary for Phase 2+ |

`core/confidence` is the one crate every other domain may depend on; the
rest do not depend on each other except through `core/service`, which
composes `ServiceSession` state that other domains reference by id
(`service_id`) rather than by direct type dependency.

## Provider/adaptor implementations

- `integrations/bible` - Phase 1 ships one `BibleProvider`: a local
  SQLite-backed implementation (`SqliteBibleProvider`), proving the
  contract end to end. `integrations/music`, `integrations/web`,
  `integrations/obs`, `integrations/vmix` are placeholders for later
  phases.
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
  `NullRenderer`. `presentation/templates` and `presentation/outputs` are
  reserved directories for the future presentation designer.

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
itself; see [`docs/live-service.md`](live-service.md).

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

## Configuration

`apps/desktop/src-tauri/src/config.rs`'s `AppConfig` resolves, from Tauri's
app data directory, everything the app needs to run: `data_dir`,
`database_path`, `model_dir`, `log_dir`, and an `environment`
(`development` / `test` / `production`, resolved from the `CIP_ENV`
environment variable, falling back to the build profile). No secrets are
read or stored here - a networked integration is responsible for its own
credential storage, out of scope for Phase 1.

## Logging & error handling

Every log call site logs against one of ten categories
(`apps/desktop/src-tauri/src/logging.rs::LogCategory`): `App`, `Database`,
`Audio`, `Speech`, `Bible`, `Ai`, `Presentation`, `Network`, `Security`,
`Error`. `apps/desktop/src-tauri/src/errors.rs::AppError` is the single
error type every Tauri command returns; it wraps each domain's own error
type and knows which category it belongs to, so command dispatch logs
consistently before the error crosses the IPC boundary.

## Boundaries

- `core/*` crates depend on `cip-core-confidence` and (for `core/service`,
  `core/ai`, `core/presentation`) each other's public types by id
  reference, never on `integrations/*`, `ai/*`, `presentation/renderer`,
  `database`, or `tauri`.
- `integrations/*` and `ai/*` depend on `core/*` (to implement its
  traits) and on `database` where they need storage - never the reverse.
- `apps/desktop/src-tauri` is the only crate allowed to depend on `tauri`
  directly. It composes `core`, `database`, and `integrations/*` behind
  Tauri commands; no domain crate knows Tauri exists.
- Nothing depends on Supabase, Firebase, or any other cloud SDK. Nothing
  in `core` may hard-code Bible content (it comes from a `BibleProvider`)
  or a specific AI provider (it comes from a `SpeechEngine`).
