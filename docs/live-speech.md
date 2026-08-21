# Live Speech Foundation (Phase 1.2)

This document explains the live pipeline added in Phase 1.2: real audio
capture, a replaceable speech-to-text boundary, and the wiring that carries
a spoken sentence all the way to a presented-on-approval suggestion.
[`docs/architecture.md`](architecture.md) still describes the overall
system and [`docs/bible-intelligence.md`](bible-intelligence.md) still owns
the Bible Intelligence Core itself - this document only covers what's new:
everything *before* `process_transcript_segment` (audio, speech,
persistence, IPC, the Live Church Brain UI, and - as of Phase 1.2.1 -
Tauri-vs-web runtime detection) plus how the two phases connect.

**Not in this phase:** song/hymn recognition, sermon intelligence,
semantic/paraphrase Bible search, cloud speech, OBS/vMix integration, the
full presentation designer, automatic projection. See
[`README.md`](../README.md) for the full phase boundary.

## The pipeline

```
MICROPHONE
    v
AudioEngine::start()            (integrations/audio::CpalAudioEngine)
    v  AudioChunk { samples, sample_rate_hz }   (via an AudioChunkSink closure)
SpeechEngine::feed_audio()      (ai/speech - Null / Scripted / Whisper)
    v  Vec<TranscriptSegment>  (interim segments displayed only, never processed further)
handle_audio_chunk()            (apps/desktop/src-tauri/src/commands.rs)
    v  final segments only
persist_transcript_segment()    (transcript_segments row)
    v
process_transcript_segment()    (Bible Intelligence Core - see docs/bible-intelligence.md)
    v
persist_scripture_detection()   (scripture_detections rows, validated only)
persist_suggestion()            (ai_suggestions rows, always Pending)
    v
Tauri events (TRANSCRIPT_UPDATED, SCRIPTURE_DETECTED/UPDATED, SUGGESTION_CREATED)
    v
Live Church Brain (React)  ->  human approves / edits / rejects
```

`apps/desktop/src-tauri/src/pipeline.rs::handle_final_transcript` is the
seam between the two phases: it persists the transcript row, calls the
unchanged Phase 1.1 `process_transcript_segment`, then persists whatever it
returns. Both the real audio path (`commands.rs::handle_audio_chunk`) and
the deterministic `process_test_transcript` command call this same
function, so manual testing and real speech are never two different code
paths downstream of "a final transcript segment exists."

## Three separate layers, on purpose

Per the architecture's provider/adaptor principle, this phase keeps three
boundaries strictly separate - nothing in `core/bible` or `core/service`'s
Bible Intelligence Core knows any of the following three types exist:

1. **Audio recognition** - `cip_core_service::AudioEngine` (capture raw PCM
   from a device). Real implementation: `integrations/audio::CpalAudioEngine`.
2. **Speech recognition** - `cip_core_ai::SpeechEngine` (PCM -> text).
   Implementations: `NullSpeechEngine`, `ScriptedSpeechEngine`,
   `WhisperSpeechEngine` (`ai/speech`).
3. **Church intelligence** - `cip_core_service::process_transcript_segment`
   (text -> detection -> suggestion). Takes a `&str`; has never heard of
   `cpal` or `whisper-rs` and never will.

A different speech backend (a cloud API, a different local model) is a new
`ai/speech` module implementing `SpeechEngine` - nothing in `core/bible`,
`core/service`'s orchestrator, `integrations/audio`, or the Tauri commands
that call `process_transcript_segment` changes.

## AudioEngine: `CpalAudioEngine`

`integrations/audio` implements `AudioEngine` over
[cpal](https://github.com/RustAudio/cpal) (cross-platform: ALSA on Linux,
CoreAudio on macOS, WASAPI on Windows). `cpal::Stream` is not `Send`/`Sync`,
so `CpalAudioEngine` never holds one directly: `start()` sends a command
over an `mpsc` channel to a dedicated worker thread, which owns the actual
`cpal::Stream` for its entire lifetime and reports status back through
`Arc<Atomic*>` fields. This is what makes `CpalAudioEngine` itself
`Send + Sync` (required by `Box<dyn AudioEngine>` in `AppState`) without
faking anything about cpal's real threading model - see
`integrations/audio/src/lib.rs` for the worker loop.

`list_devices()` returning an empty `Vec` (as it correctly does in a
container with no `/dev/snd`) is real, correct behavior, not a stub - the
"no audio device" state is exactly what `AudioStatusKind::Unavailable`
reports through `get_live_status`.

`start()` takes an `AudioChunkSink` (`Arc<dyn Fn(AudioChunk) + Send + Sync>`)
rather than returning a stream/iterator, so the caller (a Tauri command)
never has to poll - chunks arrive on the engine's own capture thread and
are pushed straight into the speech engine from there
(`commands.rs::handle_audio_chunk`).

## SpeechEngine: three implementations, one trait

| Engine | Purpose | Ready by default? |
| --- | --- | --- |
| `NullSpeechEngine` | Safe default - reports itself not ready, rejects audio. Whenever no real engine is configured/available, this is what runs. | Yes (it's what "no speech" looks like) |
| `ScriptedSpeechEngine` | Deterministic test/demo adapter - feeds a fixed script of lines back, one `is_final: true` segment per non-empty `feed_audio` call, no model or audio hardware required. | Only in tests |
| `WhisperSpeechEngine` | Real local backend, [whisper-rs](https://github.com/tazz4843/whisper-rs) (MIT) bindings to [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (MIT). Behind the `whisper` Cargo feature. | Only if a model file is present - see below |

### Interim vs. final

`TranscriptSegment.is_final` is the only signal the pipeline acts on:
`false` (interim/partial) segments are forwarded to the frontend as
`TRANSCRIPT_UPDATED` for live display and nothing else; only `is_final:
true` segments reach `handle_final_transcript`. whisper.cpp's `full()` call
is synchronous over a complete buffer, so `WhisperSpeechEngine` does not
natively produce interim results - it buffers ~3 seconds of audio and emits
one final segment per inference pass (also on `flush()`, e.g. when capture
stops). This is an honest reflection of the backend's real behavior, not a
missing feature: a future engine with true streaming support would emit
interim segments as words arrive without any change to `handle_audio_chunk`
or the Bible Intelligence Core, since both already treat `is_final` as the
only distinguishing signal.

### Engine selection and the model-download blocker

`apps/desktop/src-tauri/src/lib.rs::create_speech_engine` chooses at
startup: if built with `--features whisper` *and*
`<model_dir>/ggml-tiny.en.bin` (`config::WHISPER_MODEL_FILENAME`) exists,
it loads `WhisperSpeechEngine`; otherwise it falls back to
`NullSpeechEngine` and logs why. **A missing or absent model is never
fatal** - the application starts, the database opens, Bible search and the
deterministic transcript harness all work; only live microphone
transcription is unavailable, reported via `SpeechStatusKind::Unavailable`
in `get_live_status` and surfaced as a notice in the Live Church Brain UI
rather than a blocked startup.

CIP never bundles or auto-downloads a model. In this development
environment, obtaining one to verify end-to-end transcription was
attempted and blocked: the standard model host, `huggingface.co`, returns
`403` under this environment's egress policy (a deliberate proxy block,
not a transient failure - see the proxy README, which instructs not to
retry or route around it). That is a documented **environmental**
limitation of this sandbox, not a defect in `WhisperSpeechEngine`:

- The engine's code is real, compiles fully offline once whisper-rs's
  vendored whisper.cpp source is fetched from crates.io (no model
  download needed to *build*), and is exercised by a real test
  (`missing_model_file_is_reported_as_model_not_found`) proving the
  model-absence path behaves exactly as a real installation without a
  configured model would.
- What was **not** verified in this environment is an actual decoded
  transcript from real audio through a real model file, because no model
  file could be obtained here. Anyone running CIP with network access to
  a model host (or who copies a model file in by hand) can verify this by
  building with `--features whisper`, placing a `ggml-tiny.en.bin` (or
  another ggml/gguf Whisper model) at `<app-data-dir>/models/`, and
  starting a service.

### Model licensing

Whisper model weights (e.g. `ggml-tiny.en.bin`) are published by the
[whisper.cpp project](https://github.com/ggerganov/whisper.cpp) under
OpenAI's Whisper model license (MIT for the code that produced them;
consult the specific model card for the weights themselves before
distributing). CIP does not vendor or redistribute any model weights - the
operator supplies their own model file. `whisper-rs` and `whisper.cpp`
(the code CIP does depend on and compile) are both MIT-licensed.

### Building with real speech recognition

```sh
cargo build -p cip-desktop --features whisper
```

Off by default (plain `cargo build`/`cargo check`/`cargo test` do not
compile whisper.cpp) because vendoring and compiling it costs real build
time that shouldn't be imposed on every default build or on CI. CI runs
the default (non-`whisper`) build; `ScriptedSpeechEngine` and
`NullSpeechEngine` exercise the `SpeechEngine` boundary in every test run
regardless of the feature flag - see "Testing" below.

## Persistence

Migration `0002_live_speech_detail.sql` adds the columns Phase 1.2 needed
that Phase 1.0's schema didn't yet have: `transcript_segments.sequence_number`,
`.language`, `.speaker_id`, and `scripture_detections.detection_type`,
`.source_text`. See [`docs/database.md`](database.md) for the full schema.

`apps/desktop/src-tauri/src/persistence.rs` holds plain, Tauri-agnostic
functions over `&rusqlite::Connection` - directly unit-testable without a
running app. Two rules worth calling out:

- **Every final transcript segment is persisted**, regardless of whether
  it contains a scripture reference - the transcript is a record of what
  was said, not a filtered detection log.
- **Only validated detections are persisted as `scripture_detections`
  rows.** `persist_scripture_detection` returns `false` (and inserts
  nothing) for `Ambiguous` and `Unresolved` detections - the schema has no
  "unresolved status" column to record them faithfully, and Phase 1.2's
  constraints are explicit that an invalid/unresolved reference must never
  be persisted as if it were a validated one.

## Tauri commands (IPC surface)

| Command | Purpose |
| --- | --- |
| `start_service` / `end_service` | Service lifecycle (unchanged shape from Phase 1.0, now also stops audio capture on end) |
| `list_audio_devices` | Enumerate input devices |
| `start_listening` / `stop_listening` | Start/stop capture, wiring `AudioEngine` -> `SpeechEngine` -> the pipeline |
| `process_test_transcript` | Feed text through the exact same `handle_final_transcript` path as real audio - the Phase 1.1 deterministic harness, now also the operator's manual fallback |
| `list_transcript` | Recent transcript segments for the active service |
| `list_suggestions` / `approve_suggestion` / `edit_suggestion` / `reject_suggestion` | Suggestion review - status only ever changes via one of these three explicit human actions |
| `prepare_presentation` | Turn an approved suggestion into a `PresentationItem` (still never automatic) |
| `search_bible` | Manual verse lookup, independent of any detection |
| `get_live_status` | Polled by the UI for service/audio/speech/network/AI status |

Every command validates its own input (empty strings, malformed UUIDs)
before touching state and returns `Result<T, AppError>` - nothing panics
across the IPC boundary. `start_listening` with `device_id: None` picks
the reported default device; if none exists it reports
`AudioEngineError::NoDevice` rather than guessing one.

### Event emission

No new event bus was introduced - the Phase 1.0 `AppEvent` enum
(`apps/desktop/src-tauri/src/events.rs`) already had every event name this
phase needs. `handle_audio_chunk` and `process_test_transcript` both call
the same `emit_processed_segment_events` helper, so real and manual input
emit identically: `TRANSCRIPT_UPDATED` per segment (interim and final),
`SCRIPTURE_DETECTED`/`SCRIPTURE_UPDATED` per persisted detection
(`Unresolved` detections are intentionally *not* emitted - too frequent to
be a useful event, and never persisted anyway), `SUGGESTION_CREATED` per
suggestion. `SUGGESTION_APPROVED`/`EDITED`/`REJECTED` and
`PRESENTATION_PREPARED` fire only from their respective explicit-human-action
commands - never from the speech pipeline.

## CIP Web vs. CIP Desktop (Phase 1.2.1)

This same frontend can also be deployed as a plain static site (e.g. to
Vercel) and opened in a normal browser, with no Tauri runtime underneath:

```
CIP DESKTOP:  Browser/WebView -> Tauri -> Rust backend -> SQLite/CPAL/Whisper
CIP WEB:      Normal browser -> React/Vite app -> NO Tauri backend
```

`@tauri-apps/api`'s `invoke`/`listen` reach into `window.__TAURI_INTERNALS__`,
which only exists inside a real Tauri WebView; calling them from a plain
browser throws `TypeError: Cannot read properties of undefined (reading
'invoke')`. Nothing in this frontend may let that surface. `lib/runtime.ts`'s
`isTauriRuntime()` (a thin wrapper over `@tauri-apps/api/core`'s own
`isTauri()`, which reads a runtime-injected `globalThis.isTauri` boolean)
is the single source of truth for which environment the page is running
in, and every IPC boundary is gated behind it:

- **`lib/commands.ts`** - every command wrapper goes through an internal
  `invokeCommand` helper that checks `isTauriRuntime()` *before* calling
  the real `invoke`. Outside Tauri, it rejects with a typed
  `TauriUnavailableError` naming the command - `invoke` itself is never
  called, so the raw `TypeError` never happens.
- **`lib/liveEvents.ts`** - every `onXxx` subscription goes through an
  internal `listenSafe` helper the same way. Outside Tauri there is no
  backend to emit events at all, so subscribing resolves to a harmless
  no-op `UnlistenFn` instead of calling the real `listen`.
- **`App.tsx`** - checks `isTauriRuntime()` once on mount and, outside
  Tauri, renders `WebRuntimeNotice` instead of `LiveChurchBrain` or the
  foundation diagnostics. This is the outer guard: it means the web build
  never even *attempts* an IPC call or event subscription, rather than
  attempting one and recovering from the rejection.

Nothing about Phase 1.0-1.2 changed to make this work - CIP Web has no
Rust backend, no local SQLite database, and no audio/speech engine, so it
offers no live-service functionality of its own; it only stops crashing
when someone opens the desktop frontend's build in an ordinary browser,
and says so clearly instead of showing a raw exception. `WebRuntimeNotice`
(`components/WebRuntimeNotice.tsx`) is the only thing rendered in that
case.

```sh
# frontend tests covering both wrappers' guard behavior
pnpm --filter @cip/desktop test -- runtime commands liveEvents
```

## Online/offline and AI availability

`get_live_status` reports four independent signals - deliberately never
collapsed into one "is everything OK" boolean, since each answers a
different operator question:

- **`audioStatus`** (`unavailable` / `ready` / `listening`) - from the real
  `AudioEngine::status()` and device enumeration.
- **`speechStatus`** (`unavailable` / `ready`) - from `SpeechEngine::is_ready()`.
- **`networkStatus`** (`offline` / `online`) - a short, best-effort TCP
  reachability probe (`check_network_online`, 300ms timeout against a
  well-known address). This is a **status indicator only**; nothing in the
  pipeline branches on it, and no feature is gated by it.
- **`aiStatus`** (`available` / `degraded` / `unavailable`) - derived from
  `speechStatus` alone, **never** from `networkStatus`. A fully offline
  machine with a working local model is `available`; a machine with every
  network interface up but no model installed is `degraded`. This mirrors
  the explicit requirement not to infer AI availability from connectivity,
  and not to treat "offline" as "disabled": CIP's entire audio/speech/
  detection/suggestion/persistence/manual-operation pipeline requires no
  network access at all (see "Offline verification" below) - it just also,
  separately, happens to report whether one is reachable, for the
  operator's awareness.

## Manual fallback

Speech recognition unavailable (no model, no feature flag, no working
audio device) never blocks running a service: `process_test_transcript`
(exposed in the Live Church Brain as a manual text-entry field) drives the
identical `handle_final_transcript` pipeline - persistence, detection,
confidence, suggestions, events - that real audio drives. An operator who
knows what was just said can type "Romans 8:28" and get the same
`Suggestion` that transcription would have produced.

## Live Church Brain UI (v0.1)

`apps/desktop/src/components/LiveChurchBrain.tsx` is the primary view
(`App.tsx` now mounts it first; the Phase 1.0 foundation diagnostics moved
into a collapsed `<details>`). It:

- Polls `get_live_status` every 3 seconds and subscribes to all seven
  Phase 1.2-relevant events (`liveEvents.ts`) for immediate updates between
  polls.
- Shows status badges for service/audio/speech/network/AI state.
- Lists recent transcript segments, the active Scripture context, and
  pending suggestions with their confidence.
- Offers Approve / Edit / Reject on every suggestion - no suggestion ever
  changes status without one of these being clicked.
- Offers the manual transcript-entry fallback and a Bible search box at
  all times, regardless of audio/speech status.

No automatic projection exists anywhere in this UI or the commands behind
it - `prepare_presentation` (also human-triggered) is as far as Phase 1.2
goes.

## Offline verification

CIP's live pipeline requires no network access to function. This isn't a
runtime flag - it's structural: `cargo tree -p cip-core-service` and
`cargo tree -p cip-integrations-bible` contain no HTTP client dependency,
and `cargo tree -p cip-desktop -i reqwest` resolves to nothing, confirming
`reqwest` (present elsewhere in `Cargo.lock` from an unrelated dependency)
is not part of `cip-desktop`'s actual build graph. `pipeline.rs`'s test
`the_pipeline_produces_identical_results_with_no_network_access_possible`
documents this as a test, not just a claim.

## Testing

- **Deterministic harness (Phase 1.1, unchanged and still authoritative)**
  - `core/service/src/bible_intelligence.rs` and
  `tests/tests/bible_intelligence_acceptance.rs` - the Romans 8 -> John 3
  sequence must keep passing exactly as before; Phase 1.2 never touches
  `core/bible` or `core/service`'s orchestrator logic.
- **Real speech testing (new, separate from the above - not a replacement
  for it)** - `ai/speech/src/whisper.rs`'s
  `missing_model_file_is_reported_as_model_not_found` is real and CI-safe
  (no model download); full decode-accuracy testing is manual, documented
  above, and gated on a model file the operator supplies.
- **`integrations/audio`** - 6 tests, including real (empty, in this
  container) device enumeration and a `Send + Sync` bound check on
  `CpalAudioEngine`.
- **`persistence.rs`** - 11 tests over a real in-memory SQLite connection.
- **`pipeline.rs`** - 5 tests, including the persisted Romans 8 -> John 3
  sequence and the offline-structure test above.
- **`commands.rs`** - input-validation and serialization tests for the
  Tauri command layer.
- **Frontend** - `apps/desktop/src/domain/contracts.test.ts` extends the
  Phase 1.0/1.1 compile-time contract tests to the new Phase 1.2 shapes
  (`TranscriptSegment`'s new fields, `ScriptureContext`,
  `AmbiguousCandidate`, `ScriptureDetection`, `ProcessedSegment`,
  `LiveStatus`) - there is no React Testing Library in this project, so
  these remain type/shape contract tests, consistent with the existing
  file's established pattern, rather than DOM rendering tests.
- **Runtime detection (Phase 1.2.1)** - `lib/runtime.test.ts` exercises
  the real `@tauri-apps/api/core::isTauri()` against `globalThis.isTauri`;
  `lib/commands.test.ts` and `lib/liveEvents.test.ts` mock
  `@tauri-apps/api` to prove the IPC/event guards reject with
  `TauriUnavailableError`/resolve to a no-op respectively, and never call
  the real `invoke`/`listen`, when outside the Tauri runtime - see "CIP
  Web vs. CIP Desktop" above.

```sh
cargo test --workspace
cargo test -p cip-desktop
cargo test -p cip-integrations-audio
cargo build -p cip-desktop --features whisper   # compiles whisper.cpp; slower
pnpm --filter @cip/desktop typecheck
pnpm --filter @cip/desktop test
```

## Language support

`TranscriptSegment.language` is an `Option<String>` (BCP-47-ish tag, e.g.
`"en"`), not hard-coded to English anywhere in the contract, and
`normalize_text`'s number-word handling is English-specific by
implementation, not by interface. Phase 1.2 does not claim Yoruba, Igbo,
Hausa, or Nigerian Pidgin support - none of that has been implemented or
tested - but nothing in the `TranscriptSegment`/`SpeechEngine` contracts
assumes English, so adding another language's normalization/detection
later does not require a contract change.
