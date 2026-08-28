# Phase 3.8.5 — Audit

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `bd8b135` (Phase 3.8.4)
- Working tree at start: clean

## B. What changed since the last report

The operator physically tested CIP on the real Windows laptop. The
presentation-display pipeline continues to work. New finding: Windows
correctly exposes multiple real input devices (Iriun Webcam microphone,
Realtek Stereo Mix, Intel Smart Sound microphone array, Line In), CIP can
display them, but attempting to connect CIP to any of them via "Start
Listening" produces no live transcript/intelligence, and the UI reports
"SPEECH UNAVAILABLE — manual operation remains available." The operator
asked whether audio capture itself is actually blocked by a speech
(Whisper) readiness gate, rather than a genuine audio-capture failure.

## C. Primary question, answered directly

**Yes.** `commands::start_listening` (`apps/desktop/src-tauri/src/commands.rs`,
lines 770-779) contains exactly the gate the operator described:

```rust
if !state
    .speech_engine
    .lock()
    .expect("speech_engine mutex poisoned")
    .is_ready()
{
    return Err(log_and_return(AppError::SpeechEngine(
        cip_core_ai::SpeechEngineError::NotInitialized,
    )));
}
```

This runs **before** `state.audio_engine....start(&resolved_device_id, sink)`
(line 798-802). If `speech_engine.is_ready()` is `false`, the function
returns `Err` immediately - `AudioEngine::start()` is never called, no
CPAL stream is ever opened, no `AudioChunk` is ever produced, `input_level`
never becomes non-zero, and the acoustic/music worker (spawned right
before the audio-engine call) never receives a single chunk either, since
it is spawned in the same command body but the flow never reaches that
point.

**A second, independent gate exists at the frontend layer.**
`LiveChurchBrain.tsx` line 760:

```tsx
disabled={!status?.service || status.speechStatus === "unavailable" || isBusy("start-listening")}
```

The "Start Listening" button itself is disabled whenever
`speechStatus === "unavailable"` - so even a hypothetical backend fix
would not, by itself, let the operator click the button on the real
Windows build.

## D. Is this the current shipped Windows build's actual state?

Yes, unconditionally. `apps/desktop/src-tauri/src/lib.rs`'s
`create_speech_engine` returns `NullSpeechEngine` whenever the `whisper`
Cargo feature isn't compiled in (`#[cfg(not(feature = "whisper"))]`), and
every Windows artifact built in this project through Phase 3.8.4 was
built via `npm run tauri build -- --target x86_64-pc-windows-gnu` with no
`--features whisper` flag - confirmed by every prior phase's build log
line `"built without the whisper feature; live transcription is
unavailable (manual operation still works)"`. `NullSpeechEngine::is_ready()`
returns `false` unconditionally (`ai/speech/src/lib.rs`). So on the
artifact the operator has been testing, `start_listening`'s gate has
**always** evaluated to "blocked," on every device, every time - this is
not a configuration edge case, it's the default and only state this
artifact has ever been in.

## E. Is this an intentional design or an accidental coupling?

**Accidental coupling, contradicted by this project's own documentation.**
`docs/live-speech.md`'s "Online/offline and AI availability" section
states, verbatim:

> `get_live_status` reports four independent signals - deliberately never
> collapsed into one "is everything OK" boolean, since each answers a
> different operator question: **`audioStatus`** ... from the real
> `AudioEngine::status()` and device enumeration. **`speechStatus`** ...
> from `SpeechEngine::is_ready()`.

`get_live_status` itself (`commands.rs` lines 4702-4736) already computes
`audio_status` purely from `AudioEngineStatus`/device enumeration, with
zero reference to `speech_engine` anywhere in that computation - the
*reporting* layer is already fully independent, exactly as documented.
The gate inside `start_listening` is the one place in the whole codebase
where this documented independence is violated: it makes it structurally
impossible for `audioStatus` to ever report `Listening` (and for
`input_level` to ever become observable) whenever `speechStatus` is
`unavailable`, directly contradicting the architecture the project's own
docs describe as deliberate. This is Option B territory, not Option A -
the existing documentation already describes the intended design; the
code has a bug relative to its own stated architecture.

## F. Downstream evidence that Option B is already the intended consumer model

`handle_audio_chunk` (`commands.rs` lines 1021-1052), the sink every
`AudioChunk` is delivered to once capture starts, already treats
`speech.feed_audio()` returning `Err` as a normal, recoverable,
non-fatal outcome:

```rust
match speech.feed_audio(&chunk.samples) {
    Ok(segments) => segments,
    Err(e) => {
        log::error!(...);
        *state.speech_error.lock()... = Some(e.to_string());
        record_timeline(..., AppEvent::ErrorOccurred, ...);
        return;   // this one chunk is dropped; capture continues
    }
};
```

This is exactly the Phase 1.3 "speech failure recovery" pattern (the
service stays LIVE, only the one chunk is dropped) already applied to
*every* `feed_audio` error, including `NotInitialized` - the graceful,
per-chunk handling the operator's proposed architecture calls for already
exists and is already exercised by this exact code path. The pre-flight
`is_ready()` gate in `start_listening` is strictly redundant with this:
removing it does not require handling a new failure mode, since
`handle_audio_chunk` already handles "speech is not ready" gracefully on
every single chunk it would receive.

The `SpeechEngine` trait itself (`core/ai/src/speech_engine.rs`) is built
the same way: its own doc-tested example (`NullSpeechEngine`) demonstrates
`is_ready() == false` paired with `feed_audio()` returning
`Err(NotInitialized)` as the trait's own normal, expected "not ready"
contract - not a state the trait expects callers to pre-empt by refusing
to call it at all.

## G. Does this also block acoustic/music recognition?

**Yes - a real, previously-unnoticed consequence.** The acoustic worker
is spawned, and the sink closure that feeds it a clone of every
`AudioChunk` is constructed, entirely inside `start_listening`'s body
*after* the `is_ready()` gate but *before* `audio_engine.start()`. Since
the gate returns early, the acoustic/music pipeline the operator's
proposed diagram places as a peer of the speech pipeline (both fed by the
same `AudioChunk`) is *also* completely unreachable whenever speech is
unavailable - on the current shipped Windows build, that means always.

## H. Is `input_level` (RMS) already computed independently of speech?

**Yes, entirely.** `integrations/audio/src/lib.rs`'s `rms_level()` (a
small, pure function) runs inside the CPAL stream callback
(`build_stream`'s `stream_for!` macro), computed from the raw captured
samples before the sink (and therefore before `handle_audio_chunk`/
`speech.feed_audio`) ever sees them:

```rust
input_level_bits.store(rms_level(&samples).to_bits(), Ordering::Relaxed);
has_level_reading.store(true, Ordering::Relaxed);
sink(AudioChunk { samples, sample_rate_hz });
```

`AudioEngineStatus` already exposes `input_level: Option<f32>`,
`selected_device: Option<String>`, `channels: Option<u16>`, and
`is_capturing: bool` (`CpalAudioEngine::status()`), and `get_live_status`
already forwards the whole `AudioEngineStatus` struct verbatim as
`LiveStatus.audio`. The TypeScript domain mirror
(`apps/desktop/src/domain/service.ts`) already declares
`inputLevel: number | null`. **No second audio meter needs to be built -
the existing mechanism is already complete end-to-end at the data layer.**

## I. Is `input_level` actually surfaced to the operator today?

**No.** `grep` across every `.tsx`/`.ts` file under `apps/desktop/src`
for `inputLevel` finds it declared in exactly two type-mirror files
(`domain/service.ts`, `config/appConfig.ts`) and used nowhere else -
no component reads or renders `status.audio.inputLevel`. Today the
operator has no way to see NO SIGNAL vs. SIGNAL CAPTURED at all, even
once capture itself is unblocked - only the coarse `audioStatus` badge
(`Unavailable`/`Ready`/`Listening`/`Error`) and, once transcription
works, transcript text. This is a real, evidence-supported gap the
operator's spec explicitly asks to close, using the existing mechanism.

## J. Whisper configuration/model/feature audit

- **Configured model path**: `AppConfig.whisper_model_path`, defaulting
  to `<data_dir>/models/ggml-tiny.en.bin`, overridable via
  `CIP_WHISPER_MODEL_PATH` - unchanged, not investigated further since
  it is out of this phase's scope (no Whisper/model changes requested or
  needed).
- **Model existence/readability**: `WhisperSpeechEngine::load` checks
  `model_path.is_file()` and reports `SpeechEngineError::ModelNotFound`
  if absent - real, already correct, already tested
  (`missing_model_file_is_reported_as_model_not_found`).
- **`whisper` feature configuration**: not compiled into any Windows
  artifact built in this project to date (see section D) - this is a
  build/packaging decision from earlier phases, not something this phase
  is asked to change ("keep the architecture fully offline... do not add
  network dependency" - the `whisper` feature itself has no network
  dependency to build or run; it was simply never enabled for the
  Windows release artifact).
- **Errors returned by `speech_engine.is_ready()`**: `is_ready()` itself
  never errors (it's `-> bool`, not `Result`) - the *error* an operator
  would see is `SpeechEngineError::NotInitialized`, currently surfaced
  only as the reason `start_listening` refuses to run at all, not as a
  per-chunk, capture-independent signal.
- **Does the installed Windows artifact expect a model separately?**
  Yes, always has - `docs/live-speech.md`'s "Manual fallback" section and
  the in-UI copy under "SPEECH UNAVAILABLE" already state this
  (`live-brain__notice`, `LiveChurchBrain.tsx` lines 714-725: "place a
  local Whisper model at ... or set `CIP_WHISPER_MODEL_PATH`"). No change
  needed here - this is already honest, already documented, already
  visible to the operator. This phase does not download, bundle, or
  change anything about how a model is obtained.

## K. Test matrix findings (traced from code, not yet run on real hardware)

| # | Scenario | Current code behavior |
|---|---|---|
| 1 | Stereo Mix + system audio playing | `start_listening` never reaches `audio_engine.start()` on this build - **blocked**, not a capture failure |
| 2 | Physical microphone + speech | Same - **blocked** before any device is opened |
| 3 | Whisper unavailable + valid device | This *is* TEST 1/2 on the current shipped build - proves audio capture cannot currently operate independently, though nothing about `CpalAudioEngine`/`AudioChunk`/`rms_level` itself prevents it (see sections F, H) |
| 4 | Whisper available + valid audio | Not blocked by this gate (whisper feature not built into this artifact, so not exercisable this phase; downstream `handle_audio_chunk`→`feed_audio`→Bible pipeline wiring is unchanged and was proven in Phase 1.2/1.3's own test suite) |
| 5 | Invalid/disconnected device | Already correctly handled *below* this gate - `find_device` returns `AudioEngineError::DeviceNotFound` (proven by `starting_an_unknown_device_id_is_reported_not_fabricated`), never a silent fallback - but only reachable once TEST 1-3's gate is fixed |
| 6 | Stop listening | Already correct and already tested (`stop_without_ever_starting_is_a_safe_no_op`; `CpalAudioEngine::stop()` unconditionally clears `is_capturing`) - unaffected by this gate |
| 7 | Restart listening | `AudioEngineError::AlreadyCapturing` guard already exists in `CpalAudioEngine::start()`; restart-after-stop already works at the engine level - unaffected by this gate |

## L. Exact boundary failing

**`start_listening`'s pre-flight `speech_engine.is_ready()` check**
(`commands.rs` line 770), and its frontend mirror, the "Start Listening"
button's `disabled` condition including `speechStatus === "unavailable"`
(`LiveChurchBrain.tsx` line 760). Everything upstream (device
enumeration, `AudioEngineStatus`, `input_level`/RMS) and everything
downstream (`handle_audio_chunk`'s per-chunk graceful `feed_audio` error
handling, the acoustic worker, the Bible/Sermon pipeline once a real
`TranscriptSegment` exists) is already built for, and already tested
against, independent audio-capture operation. Nothing else in the chain
the operator listed (UI device selection → `startListening(deviceId)` →
Tauri `start_listening` → `audio_engine.start()` → CPAL device resolution
→ CPAL input stream → `AudioChunk` → `handle_audio_chunk` →
`speech.feed_audio()` → `TranscriptSegment` → Bible/Sermon/Content
intelligence) needs to change.

## M. Decision: Option A or Option B

**Option B**, per section E/F above - directly supported by
`docs/live-speech.md`'s own explicit statement that `audioStatus` and
`speechStatus` are deliberately independent signals, and by
`handle_audio_chunk` already implementing graceful per-chunk speech
failure handling. This phase does not invent Option B; it removes the
one place the code contradicts an architecture the project already
documents and already implements everywhere else.

## N. Smallest evidence-supported fix

1. **`commands.rs`**: remove the pre-flight `is_ready()` early-return
   from `start_listening`. Audio capture starts unconditionally (subject
   to the existing, unchanged device-resolution/`AudioEngineError`
   handling). Speech readiness is checked *after* a successful
   `audio_engine.start()` only to decide whether to emit/record
   `AppEvent::SpeechStarted` - so that event keeps meaning "speech
   actually started," never fabricated when it did not (matching this
   codebase's existing "never show a fabricated signal" discipline, e.g.
   `selected_device`/`stream_error`'s own doc comments). `AppEvent::AudioStarted`
   is recorded/emitted unconditionally, since audio genuinely did start.
2. **`LiveChurchBrain.tsx`**: remove `status.speechStatus === "unavailable"`
   from the "Start Listening" button's `disabled` condition. The button
   remains disabled when there is no active service or a request is
   already in flight - unchanged.
3. **Surface `input_level`** (the existing, already-computed RMS value)
   in the Audio & Speech panel, next to the device selector - no new
   audio-analysis mechanism, purely rendering a number/bar from
   `status.audio.inputLevel` that already exists in `LiveStatus`.

No change to: `CpalAudioEngine`, `WhisperSpeechEngine`, `NullSpeechEngine`,
the `SpeechEngine`/`AudioEngine` trait contracts, `handle_audio_chunk`'s
per-chunk error handling, the acoustic worker, the Bible/Sermon/Content
intelligence pipeline, any Tauri command's name/parameters, any event
contract, any database schema, the `whisper` Cargo feature, model
loading/download behavior, or any capability/permission grant. No second
speech engine, no second audio engine, no cloud dependency introduced.

## O. What this phase cannot verify

No physical Windows machine, no real audio hardware, and no real Whisper
model file exist in this container (confirmed again: `list_devices`
returns an honest empty list here, matching every prior phase). This
audit's conclusions are proven by direct code tracing and by this
project's own existing, passing test suite (cited throughout), not by
executing real audio capture. The real Windows re-test - selecting
Stereo Mix, observing `input_level` become non-zero while audio plays,
and (separately) confirming the manual-pipeline Bible detection still
works exactly as Phase 3.8.4 verified it - remains the decisive gate and
has not occurred in this session.
