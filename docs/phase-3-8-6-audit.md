# Phase 3.8.6 audit — Windows Whisper Build & Packaging

Written before any code change, per this project's standing discipline
and the operator's explicit instruction to audit the build/packaging
path before touching code.

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `487994b` (Phase 3.8.5, "decouple audio capture from
  speech-engine readiness")
- Working tree at start: clean

## B. Trigger

The operator's real Windows test (post-3.8.5) now shows genuinely working
audio capture: real device enumeration, `SIGNAL CAPTURED — input level
10%`, correct listening lifecycle, Service Replay, and presentation all
working. The remaining, now-isolated symptom is:

> SPEECH ERROR — recorded, will clear on the next successful chunk.
> transcription unavailable, audio capture only

The operator's spec asks two questions in order: (1) does the *build*
even contain Whisper, and (2) if it did, would the rest of the pipeline
actually work end-to-end. Both are answered below with evidence, not
assumption.

## C. The Whisper feature graph (Cargo.toml → binary)

```
Cargo.toml (workspace)                 — no `whisper` feature reference at all
  └── ai/speech/Cargo.toml
        whisper-rs = { version = "0.14", optional = true }
        [features]
        whisper = ["dep:whisper-rs"]
  └── apps/desktop/src-tauri/Cargo.toml
        cip-ai-speech.workspace = true      (no feature forwarded by default)
        [features]
        whisper = ["cip-ai-speech/whisper"]   ← OFF unless explicitly requested
```

- **What Cargo feature enables Whisper?** `whisper`, defined in both
  `ai/speech/Cargo.toml` (gates the `whisper-rs` dependency) and
  `apps/desktop/src-tauri/Cargo.toml` (forwards it). Confirmed by reading
  both manifests directly.
- **Is it enabled by default?** No. Neither `[features] default = [...]`
  entry exists in either crate; `optional = true` on `whisper-rs` means
  it is excluded from the dependency graph unless requested.
- **Was it enabled in the previous Windows installer?** No. Every prior
  Windows rebuild in this project's history (3.2, 3.4, 3.7, 3.8, 3.8.1
  through 3.8.5) ran `cargo build --release --target x86_64-pc-windows-gnu`
  or `npx tauri build --target x86_64-pc-windows-gnu` **without**
  `--features whisper`. Direct evidence: every prior phase's own Xvfb/
  build logs record `"built without the whisper feature; live
  transcription is unavailable (manual operation still works)"` -
  verbatim, e.g. `pilot-evidence/3.8.4/xvfb/cip-xvfb-3-8-4-run1-fresh.log`
  line 10, and this session's own Phase 3.8.5 Xvfb attempt logged the
  identical line. The desktop crate's own `[features]` doc comment says
  so explicitly: *"Off by default... Enable with `cargo tauri build
  --features whisper` once a local model is actually configured."* That
  command has never been run for a packaged release in this project.
- **Does the Windows cross-build command explicitly enable it?** No -
  confirmed by re-reading every prior phase's own "Windows artifact"
  section; all say `cargo build --release --target x86_64-pc-windows-gnu`
  with no `--features` flag.
- **Does `cargo tree` show the Whisper dependency when the release build
  is produced?** Only when `--features whisper` is passed. Verified this
  phase:
  ```
  $ cargo tree -p cip-desktop --features whisper -i whisper-rs
  whisper-rs v0.14.4
  └── cip-ai-speech v0.1.0
      └── cip-desktop v0.1.0
  ```
  Without `--features whisper`, `whisper-rs` does not appear in the tree
  at all (it is an optional dependency gated by the feature).
- **Does the resulting Windows binary contain the Whisper-enabled code
  path?** Not in any artifact produced by this project to date. Verified
  this phase that it *can*:
  ```
  $ cargo check --target x86_64-pc-windows-gnu -p cip-desktop --features whisper
  ...
  Compiling whisper-rs v0.14.4
  Compiling whisper-rs-sys v0.13.1
  ...
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 01s
  ```
  whisper-rs-sys compiles whisper.cpp (C++/cmake) for the
  `x86_64-pc-windows-gnu` target using this container's
  `x86_64-w64-mingw32-g++`/`cmake` cross-toolchain (both confirmed
  present: `/usr/bin/cmake` 3.28.3, `/usr/bin/x86_64-w64-mingw32-g++`).
  This is real evidence the Windows target *can* be built with Whisper
  compiled in - it has just never been done for a shipped artifact.

**Conclusion**: every "Whisper tests passing" claim in this project's
history refers to `cargo test -p cip-ai-speech --features whisper` run on
the **Linux host target**, as part of the regression suite - never the
Windows cross-compiled release binary. No prior Windows installer has
ever contained Whisper. This is the confirmed root cause the operator's
spec asked to distinguish, and it fully explains "SPEECH UNAVAILABLE"/
"SPEECH ERROR" on every real Windows test performed in this project so
far, independent of anything about device selection or audio capture
(which Phase 3.8.5 separately fixed).

## D. Model discovery and installed-runtime path resolution

```
AppConfig::resolve(app: &AppHandle)
  data_dir = app.path().app_data_dir()   ← Tauri's own per-OS API,
                                            not cwd, not a dev path
  model_dir = data_dir.join("models")
  whisper_model_path = env "CIP_WHISPER_MODEL_PATH"
                        .unwrap_or(model_dir.join("ggml-tiny.en.bin"))
```

On Windows, `app_data_dir()` resolves to `%APPDATA%\org.churchintelligence.cip`
(the operator's own screenshots confirmed this exact path:
`C:\Users\HP\AppData\Roaming\org.churchintelligence.cip\models\ggml-tiny.en.bin`).
This is a real Tauri platform API, not a hand-rolled path - it does not
depend on the current working directory, the repository directory,
`target/debug`, `target/release`, or any Linux development path.
**The resolver itself has no defect** - confirmed by direct reading of
`apps/desktop/src-tauri/src/config.rs` and its existing test
`whisper_model_path_defaults_under_model_dir_when_unset`.

**Which of the operator's options (A/B/C/D) is the current behavior?**
Option **D** - the model is expected to already exist in `AppData`, and
nothing in this codebase creates it:

- No `resources` array exists in `apps/desktop/src-tauri/tauri.conf.json`
  (grepped directly - `"bundle"` has only `icon`, no `resources` key).
- No `include_bytes!`, no resource-copy-on-first-run logic exists
  anywhere in `apps/desktop/src-tauri/src/` (grepped directly).
- **No `ggml*.bin` model file exists anywhere in this repository** -
  confirmed via `find . -iname "*.bin" -o -iname "ggml*"` returning zero
  matches outside `node_modules`. There is nothing to embed even if
  bundling were wired up.
- This is documented as a known, deliberate limitation already, in
  `ai/speech/src/whisper.rs`'s own module doc comment: *"Running it
  requires a local ggml/gguf Whisper model file, which is not bundled
  with CIP... and, in this development environment, could not be
  downloaded to verify end-to-end transcription: the standard model host
  (huggingface.co) is blocked by this environment's egress policy."*

**Verified this phase, directly**: this container's egress policy still
blocks the standard model host.
```
$ curl -sS -m 10 https://huggingface.co
curl: (56) CONNECT tunnel failed, response 403
[agent-proxy] ... connect_rejected (the egress proxy denied the CONNECT
  (organization policy) or could not reach the destination)
```
This is a hard environmental constraint, not a code defect and not
something this session can route around. **Option A (embed a model in
the installer) and Option C (download on first run) are both
infeasible in this container** - there is no reachable, legitimate model
source to obtain a real ggml file from. Option D (operator supplies the
file into `AppData\...\models\ggml-tiny.en.bin` themselves) remains the
only viable path for this session, and is honestly the documented
architecture already - the real gap is that this is a silent expectation
rather than a first-class, clearly-surfaced one (see section H).

## E. Model loading error granularity

```
create_speech_engine(config)
  #[cfg(feature = "whisper")]
    WhisperSpeechEngine::load(&config.whisper_model_path)
      model_path.is_file()? no  → Err(ModelNotFound(path))
      WhisperContext::new_with_params(...)  fails → Err(TranscriptionFailed(whisper-rs's own error text))
      success                              → Ok(engine)
  #[cfg(not(feature = "whisper"))]
    → NullSpeechEngine (always)
```

`SpeechEngineError` (`core/ai/src/speech_engine.rs`) has three variants:
`NotInitialized`, `ModelNotFound(String)`, `TranscriptionFailed(String)`.
`WhisperSpeechEngine::load` (`ai/speech/src/whisper.rs`) already
distinguishes a missing file (`ModelNotFound`) from every other failure
during `WhisperContext::new_with_params` - a corrupt/truncated/wrong-
format file, or a genuine whisper.cpp/library init failure - which all
collapse into `TranscriptionFailed(e.to_string())`, where `e` is
whisper-rs's own real error. **The underlying text is not lost** - it is
carried inside the `String`. A dedicated unit test
(`corrupt_model_file_is_reported_as_transcription_failed_not_a_panic`)
already proves a corrupt-but-present file fails cleanly with this
variant, never a panic.

**The real gap is visibility, not generation.** Tracing where this error
goes:

1. `create_speech_engine`'s `Err(e)` branch (`lib.rs`) only
   `log::warn!`s the error text and silently falls back to
   `NullSpeechEngine` - **the error itself is discarded after logging**;
   nothing in `AppState` retains it, so no command can ever report it.
2. `handle_audio_chunk`'s `Err(e)` branch (per-chunk `feed_audio`
   failures) *does* store the text in `AppState.speech_error` and logs it
   via `log::error!` - but `speech_error` is read by exactly one place,
   `get_live_status`, and only to produce the bare enum
   `SpeechStatusKind::Error` (no message field exists on `LiveStatus` at
   all - confirmed by reading the full struct).
3. The frontend (`LiveChurchBrain.tsx` line 732) hardcodes: `"SPEECH
   ERROR — recorded, will clear on the next successful chunk."` - a
   static string, never interpolating the real error.

So today, a real Windows operator who successfully gets `SPEECH ERROR` in
the UI has **no way to see the underlying reason** - not the file path
that failed to load, not whisper.cpp's own error text, nothing. This
matches the operator's step 5 requirement exactly and is a confirmed,
fixable gap: the data already exists at the point of failure, it is just
thrown away one layer up.

## F. Audio chunk → Whisper inference path — sample-rate mismatch (new finding)

```
CpalAudioEngine (integrations/audio/src/lib.rs)
  own module doc: "Chunks are delivered at the device's own native
    sample rate (reported on every AudioChunk), never silently resampled
    to a fixed rate. A consumer that needs a specific rate (e.g. a
    SpeechEngine) is responsible for converting."
  → AudioChunk { samples: Vec<i16> (already mono via downmix_to_i16),
                 sample_rate_hz: u32 (the device's real negotiated rate) }
       ↓
handle_audio_chunk (commands.rs)
  speech.feed_audio(&chunk.samples)   ← passes samples AS-IS, no conversion
       ↓
WhisperSpeechEngine::feed_audio (ai/speech/src/whisper.rs)
  const SAMPLE_RATE_HZ: u32 = 16_000;  ← hardcoded, assumed unconditionally
  buffers samples, runs inference treating every sample as 16kHz-spaced
```

**This is a real, previously undocumented defect**, distinct from
anything the operator's report described so far, but squarely inside
what their spec asked this audit to rule in or out ("wrong sample rate"
is explicitly named in their step 6 checklist). Evidence:

- `SpeechEngine::feed_audio`'s trait signature
  (`core/ai/src/speech_engine.rs`) takes only `&[i16]` - no sample-rate
  parameter. Its doc comment says "raw mono PCM16 audio," an implicit
  fixed-rate contract with no rate actually passed.
- `WhisperSpeechEngine` hardcodes `SAMPLE_RATE_HZ = 16_000` and never
  reads `chunk.sample_rate_hz` (it has no way to - the trait never gives
  it that information).
- `handle_audio_chunk` (the one place that actually has both the real
  device rate, via `chunk.sample_rate_hz`, and the call into
  `feed_audio`) performs **zero resampling**.
- `docs/live-speech.md` never mentions sample rate at all (grepped
  directly, zero matches) - this was never flagged as a known, accepted
  gap the way the model-download blocker was.
- No existing test exercises `feed_audio` with a non-16kHz chunk, or
  exercises `handle_audio_chunk`'s call into it at all - this path is
  currently completely untested.

Real Windows input devices overwhelmingly negotiate 44100 Hz or 48000 Hz
as their native rate (`Stereo Mix - Realtek(R) Audio`, `Microphone Array
- Intel Smart Sound` are both consumer WASAPI devices; 16 kHz native
capture is unusual). Feeding 44.1/48 kHz-paced samples into an inference
path that assumes 16 kHz means: the `CHUNK_SAMPLES` (48,000-sample,
"~3 seconds") threshold fires roughly 3x faster than 3 real seconds, and
whisper.cpp is asked to transcribe audio that is effectively compressed/
pitch-shifted relative to what its 16kHz-trained model expects. This
would very plausibly produce empty or garbage transcriptions **even with
the `whisper` feature enabled and a real, valid model file present** -
i.e. even a "successful" Phase 3.8.6 that only flips the Cargo feature on
and ships a model would likely still fail the real-hardware test, for a
completely different reason than the one currently visible. This must be
fixed as part of this phase, not deferred, because it is the difference
between "Whisper is compiled in" and "Whisper can actually transcribe
real Windows audio."

The acoustic/music pipeline is unaffected: `spawn_acoustic_worker`
already receives and uses `chunk.sample_rate_hz` explicitly
(`worker.ingest(&chunk.samples, chunk.sample_rate_hz)`) - only the direct
`SpeechEngine::feed_audio` call site lacks this.

## G. Answering the operator's 14 questions (condensed)

1. Cargo feature: `whisper` (both crates). Not default.
2. Not enabled in any prior Windows build - confirmed via every prior
   phase's own logs.
3. Windows cross-build has never passed `--features whisper`.
4. `cargo tree` only shows `whisper-rs` when the feature flag is passed;
   confirmed both ways this phase.
5. The Windows binary produced so far never contains the Whisper code
   path; confirmed feasible to build one that does (`cargo check
   --target x86_64-pc-windows-gnu -p cip-desktop --features whisper`
   succeeded, compiling whisper.cpp via the mingw/cmake cross-toolchain).
6. Model resolution is Option D (expected pre-placed in `AppData`); not a
   defect in the resolver itself, but a silent expectation, not a
   first-class supported flow.
7. No model file exists anywhere in this repository to bundle even if
   packaging were wired up.
8. This container's egress policy blocks `huggingface.co` (confirmed
   with a live `curl` test this phase) - Options A/C are infeasible here.
9. The resolver never depends on cwd/dev paths - confirmed correct via
   `app.path().app_data_dir()` and its own test coverage.
10. Missing-file and corrupt-file failures already produce distinct,
    non-panicking errors (`ModelNotFound` vs `TranscriptionFailed`) with
    real underlying text - but that text is discarded after logging at
    startup, and never included in any command's JSON response.
11. The per-chunk `feed_audio` error text *is* retained
    (`AppState.speech_error`) but never surfaced past a bare
    `SpeechStatusKind::Error` enum - no message ever reaches the UI.
12. The audio→speech path performs **no sample-rate conversion at all**
    - a real, previously-undocumented defect that would block correct
    transcription independent of the feature/model gap.
13. The acoustic/music path is unaffected (already handles variable
    sample rate correctly).
14. Nothing about the `SpeechEngine`/`AudioEngine` trait boundaries, the
    Bible database, intelligence engines, or event contracts needs to
    change to fix any of this - the fixes are additive (a new default
    trait method, one new resampling call site, and diagnostics state
    that was already being computed but discarded).

## H. Decision: what Phase 3.8.6 will and will not do

**Will do** (all evidence-supported, all "smallest justified fix" in
scope):

1. Build the Windows release with `--features whisper` explicitly, so
   the shipped binary genuinely contains the Whisper code path -
   verifiable via build log + `cargo tree` + the resulting binary size
   change (whisper.cpp statically linked is a substantial, measurable
   size increase).
2. Fix the sample-rate mismatch: add a `SpeechEngine::required_sample_rate_hz()`
   default trait method (`None`), override it in `WhisperSpeechEngine`
   (`Some(16_000)`), and resample `AudioChunk.samples` from
   `chunk.sample_rate_hz` to the engine's required rate in
   `handle_audio_chunk` before calling `feed_audio` - exactly the
   "consumer that needs a specific rate is responsible for converting"
   contract `integrations/audio` already documents. No second audio
   engine, no change to the `AudioEngine` trait or `CpalAudioEngine`.
3. Retain and surface the real underlying error text that already exists
   at both failure points (startup model load, per-chunk inference) -
   extend `AppState` with a small, additive diagnostics record and
   extend the *existing* `PilotDiagnostics`/`get_pilot_diagnostics`
   command (Phase 3.4's own "operator should be able to understand
   whether Whisper is ready" mechanism) rather than inventing a second,
   parallel diagnostics command. Add `featureCompiled`, whether the
   configured model actually loaded into a working engine (distinct from
   "a file exists at the path"), the real load/inference error text, and
   basic chunk/inference counters (chunks received, last chunk sample
   rate/sample count, inferences attempted/succeeded).
4. Rebuild the Windows installer with the feature enabled, record real
   build-log proof of the feature/dependency graph in the release
   manifest, and be completely honest that **no model file is embedded**
   because none could be obtained in this container (network-blocked,
   confirmed live) - not simulated as present, not silently omitted from
   the report either.

**Will not do**:

- Will not fabricate, guess, or synthesize a "model" file of any kind.
- Will not add a runtime network dependency to the shipped app (the only
  network access in this plan is this build session's own attempt - and
  failure - to reach a model host at build time, not anything the
  installed app does).
- Will not add a second speech engine, a second audio engine, or a
  second audio meter.
- Will not change the `SpeechEngine`/`AudioEngine` trait's existing
  methods, only add one new default-method (fully backward compatible -
  `NullSpeechEngine`/`ScriptedSpeechEngine` need no changes at all).
- Will not claim real Windows transcription evidence exists - it does
  not, and cannot, without a real model file on real hardware, which
  remains an operator-side step this session cannot perform.

## I. What this phase cannot verify

No physical Windows machine and no real audio hardware exist in this
container (unchanged from every prior phase). Additionally, this
specific phase cannot obtain a real Whisper model file at all, because
the standard model host is blocked by this container's own egress
policy - confirmed by a live network test, not assumed. This means even
after this phase's fixes, the shipped Windows installer will compile in
real Whisper support but will **not** include a model file; an operator
must still place one at `%APPDATA%\org.churchintelligence.cip\models\ggml-tiny.en.bin`
(or point `CIP_WHISPER_MODEL_PATH` elsewhere) before transcription can
run at all. This is reported honestly in the phase report and release
manifest, not glossed over.
