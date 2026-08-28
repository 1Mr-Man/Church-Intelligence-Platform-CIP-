# Phase 3.8.6 — Windows Whisper Build & Packaging Audit

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `487994b` (Phase 3.8.5, "decouple audio capture from
  speech-engine readiness")
- Working tree at start: clean

Full audit in `docs/phase-3-8-6-audit.md`, written before any code
changed, tracing the Whisper feature graph, model discovery/packaging,
and error granularity per the operator's own spec, before implementing
anything.

## Why this phase exists

The operator's Phase 3.8.5 real Windows re-test showed a decisively
narrowed problem: real device enumeration works, audio capture genuinely
starts (`SIGNAL CAPTURED — input level 10%`), the listening lifecycle,
Service Replay, and presentation all work. The one remaining blocker is
transcription itself: `SPEECH ERROR — recorded, will clear on the next
successful chunk.` The operator's spec asked, in order: does the build
even contain Whisper, and if it did, would the rest of the pipeline
actually work?

## Root causes (three, all confirmed by direct evidence)

**1. No Windows build in this project's history has ever compiled with
the `whisper` Cargo feature.** Confirmed by re-reading every prior
phase's own build logs (`"built without the whisper feature; live
transcription is unavailable"`, verbatim in every Xvfb/build log from
Phase 3.2 through 3.8.5) and by directly re-reading `apps/desktop/src-tauri/Cargo.toml`'s
own `[features]` doc comment: *"Off by default... Enable with `cargo
tauri build --features whisper` once a local model is actually
configured."* That command had never been run for a packaged release.

**2. Cross-compiling with `--features whisper` for `x86_64-pc-windows-gnu`
does not work out of the box - two independent, real, verified toolchain
defects block it**, discovered by actually attempting the build rather
than trusting `cargo check` (which doesn't perform the final link step
and had misleadingly succeeded during the audit):

   - **ggml static-library naming.** whisper.cpp's vendored `ggml`
     CMake build (`ggml/CMakeLists.txt`) does
     `set(CMAKE_STATIC_LIBRARY_PREFIX "")` for any Windows target,
     assuming MSVC-style unprefixed `.lib` naming. The actual linker in
     this toolchain is MinGW's GNU `ld` (via `x86_64-w64-mingw32-gcc`),
     which requires the Unix `libX.a` convention for a plain `-lX` flag.
     So `ggml.a`, `ggml-base.a`, `ggml-cpu.a` were built but never found
     (`error: could not find native static library 'ggml'`) - confirmed
     by directly inspecting the CMake install output and the exact
     `rustc`/linker invocations (`cargo build -v`). The sibling top-level
     `whisper` library is built by a *different* CMakeLists.txt and is
     unaffected (`libwhisper.a`, correctly prefixed).
   - **MinGW threading-model mismatch.** This container's default
     `x86_64-w64-mingw32-gcc`/`g++` `update-alternatives` selection is
     the **win32**-threading-model variant, but whisper.cpp/ggml's build
     links `-lpthread` and expects POSIX threading, producing
     `undefined reference to '__mingwthr_key_dtor'` at final link.
     Confirmed the **posix**-threading variant is already installed
     alongside it (`gcc-mingw-w64-x86-64-posix` /
     `g++-mingw-w64-x86-64-posix`) and switching to it via
     `update-alternatives` resolves the symbol.

   Neither fix touches the vendored `whisper-rs-sys`/`ggml`/`whisper.cpp`
   source. Both are captured, with full explanation, in
   `scripts/build-windows-whisper.sh` - a real, repeatable two-pass build
   script (attempt, apply the ggml-naming fix to the already-built CMake
   output, retry) rather than a one-off manual hack. A downstream
   `build.rs`-based fix was attempted first and proven **not** to work:
   `whisper-rs-sys` itself fails to compile (not just the final binary
   link) before `cip-desktop`'s own `build.rs` ever runs, so a
   downstream crate's build script directives arrive too late - this is
   why the fix lives in an external build script instead.

**3. A previously-undocumented sample-rate mismatch that would have
blocked correct transcription even with the feature and a real model
present.** `integrations/audio` delivers `AudioChunk`s at the capture
device's own native rate and *never* resamples (a deliberate, documented
design choice - "a consumer that needs a specific rate is responsible for
converting"). `WhisperSpeechEngine` hardcodes a 16kHz assumption with no
way to know the real rate (the `SpeechEngine::feed_audio` trait signature
never carried one), and the one call site with both pieces of
information (`handle_audio_chunk`) performed zero conversion. Real
Windows consumer devices (`Stereo Mix - Realtek(R) Audio`, `Microphone
Array - Intel Smart Sound`) overwhelmingly negotiate 44.1kHz/48kHz, not
16kHz - so audio would have reached Whisper roughly 3x too fast,
producing empty or garbage transcription even on a fully-configured
build. `docs/live-speech.md` never mentioned sample rate at all - this
was not a known, accepted gap the way the model-download blocker is.

## Fixes applied

1. **`core/ai/src/speech_engine.rs`** - added
   `SpeechEngine::required_sample_rate_hz() -> Option<u32>` as a default
   trait method (`None`) - fully backward compatible, `NullSpeechEngine`/
   `ScriptedSpeechEngine` need no changes.
2. **`ai/speech/src/whisper.rs`** - `WhisperSpeechEngine` overrides it to
   `Some(SAMPLE_RATE_HZ)` (16,000).
3. **`apps/desktop/src-tauri/src/commands.rs`** - added a pure,
   unit-tested `resample_pcm16(samples, from_hz, to_hz)` linear-
   interpolation resampler, and wired it into `handle_audio_chunk`:
   before calling `feed_audio`, if the engine reports a required rate
   different from the chunk's real rate, resample first. `NullSpeechEngine`/
   `ScriptedSpeechEngine` are unaffected (their `required_sample_rate_hz()`
   stays `None`, so no conversion happens for them). Also added per-chunk/
   per-inference diagnostics counters (see below).
4. **Diagnostics** (`state.rs`, `lib.rs`, `commands.rs`,
   `PilotDiagnosticsPanel.tsx`) - `create_speech_engine`'s real
   `SpeechEngineError` from a failed model load, previously logged with
   `log::warn!` and then discarded, is now retained in a new
   `AppState.speech_diagnostics: Mutex<SpeechDiagnostics>` and surfaced
   through the *existing* `get_pilot_diagnostics` command/System
   Diagnostics panel (Phase 3.4's own "operator should be able to
   understand whether Whisper is ready" mechanism, extended rather than
   duplicated): feature-compiled flag, whether the model actually loaded
   into a working engine (distinct from "a file exists at the path"),
   the real load/inference error text, chunks-received/last-chunk-rate/
   last-resampled-count counters, and inferences-attempted/succeeded.
5. **Windows build** - rebuilt with `--features whisper` explicitly, for
   the first time in this project's history, using the two toolchain
   fixes above (documented and automated in
   `scripts/build-windows-whisper.sh`).

No new Tauri command, no new event, no second audio engine, no second
speech engine, no second audio meter, no hardcoded device, no network
dependency added to the shipped app, no automatic model download - none
of these were touched, matching every preservation requirement in the
operator's spec.

## Model packaging - honestly not solved this phase

**No Whisper model file is bundled with this installer, and none could
be obtained to bundle.** This container's egress policy blocks the
standard model host (`huggingface.co`) - confirmed with a live `curl`
test returning a proxy-level `403`/`connect_rejected`. No `.bin`/ggml/gguf
model file exists anywhere in this repository to embed even if bundling
logic were added (confirmed via a repo-wide search, unchanged from Phase
3.8.5's finding). This is Option D from the operator's own spec: the
model must already exist at
`%APPDATA%\org.churchintelligence.cip\models\ggml-tiny.en.bin` (or
wherever `CIP_WHISPER_MODEL_PATH` points), placed by the operator - not a
defect in the resolver (which correctly uses Tauri's own
`app_data_dir()` API, confirmed to never depend on cwd/dev paths), but a
real, honestly-reported gap that this phase does not close. No automatic
download and no network dependency were added to the shipped application
to work around this, per the operator's explicit instruction.

## Full regression result

Rust workspace (default features): all suites green, unchanged behavior
from Phase 3.8.5's baseline plus 6 new `resample_pcm16` unit tests
(no-op-on-matching-rates, no-op-on-empty, downsample-length,
upsample-length, constant-signal-preserved, never-panics-on-a-single-
sample). `cargo fmt --check`, `clippy --all-targets -- -D warnings`:
clean on both default and `--features whisper`. Whisper feature test
suite (`cargo test -p cip-ai-speech --features whisper`): 7 passed, 0
failed, unchanged. `cip-desktop` test suite with `--features whisper`:
227 passed, 0 failed. Windows-target cross-compile check (`cargo check
--target x86_64-pc-windows-gnu`): clean on both feature configurations.
Frontend: 210 passed, 0 failed (unchanged pass count - the new
diagnostics fields are additive to an existing command's return type,
not a new contract requiring new tests beyond type-checking).
`typecheck`, `lint` (0 errors, 4 pre-existing warnings, unchanged),
`build`: clean.

## Windows artifact - genuinely Whisper-enabled, first time in this project

Rebuilt this phase with `--features whisper` for the Rust build **and**
the NSIS packaging step - see `pilot-evidence/3.8.6/` for the checksum
and `release/windows/release-manifest.json` for full provenance,
including concrete proof the feature is really compiled in:

- `cargo tree -p cip-desktop --target x86_64-pc-windows-gnu --features whisper -i whisper-rs`
  shows `whisper-rs -> cip-ai-speech -> cip-desktop` (absent without the
  flag).
- The embedded application binary (`cip-desktop.exe`, before NSIS
  compression) is 33,776,623 bytes.
- `strings cip-desktop.exe | grep -i ggml` finds real whisper.cpp/ggml
  symbols compiled into the binary: `whisper_full_with_state`,
  `whisper_full_parallel`, `ggml_backend_init`,
  `ggml_backend_set_n_threads`, and the literal build-time source path
  `.../whisper-rs-sys-.../out/whisper.cpp/src/whisper.cpp` - direct
  evidence of real object code, not just a `Cargo.lock` entry.
- SHA-256: `26739280b7cc4ef750d73a7d04248cf00533b7ff5623720f8ad1621f5e3f9441`.

## Architectural safety diff

```
FILES MODIFIED: core/ai/src/speech_engine.rs,
  ai/speech/src/whisper.rs,
  apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src-tauri/src/state.rs,
  apps/desktop/src-tauri/src/lib.rs,
  apps/desktop/src/config/appConfig.ts,
  apps/desktop/src/components/workspace/PilotDiagnosticsPanel.tsx
FILES CREATED: docs/phase-3-8-6-audit.md,
  docs/phase-3-8-6-windows-whisper-build.md,
  scripts/build-windows-whisper.sh,
  pilot-evidence/3.8.6/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
BIBLE DATABASE CHANGED: NO
INTELLIGENCE ENGINES CHANGED: NO
SERVICE REPLAY CONTRACT CHANGED: NO
TRANSCRIPT CONTRACT CHANGED: NO
TAURI COMMANDS RENAMED/REMOVED: NONE
TAURI COMMANDS ADDED: NONE
EXISTING COMMAND SIGNATURES CHANGED: get_pilot_diagnostics keeps its
  exact name/params; its return type (PilotDiagnostics) gained one
  additive `speech: SpeechRuntimeDiagnostics` field - existing consumers
  of the other fields are unaffected
EVENT CONTRACTS CHANGED: NONE
PRESENTATION LIFECYCLE: unchanged
PERSISTENCE: unchanged
OFFLINE ARCHITECTURE: preserved - no network dependency added to the
  shipped application; the only network access this phase attempted was
  this build session's own (failed, confirmed-blocked) attempt to reach
  a model host, never anything the installed app does
NETWORK CAPABILITIES: NONE ADDED
SPEECHENGINE TRAIT: extended additively (one new default method); NullSpeechEngine/
  ScriptedSpeechEngine/WhisperSpeechEngine's existing methods unchanged
AUDIOENGINE TRAIT / CPALAUDIOENGINE: UNCHANGED
SECOND AUDIO ENGINE / SECOND SPEECH ENGINE / SECOND AUDIO METER: NONE ADDED
DEVICE CONTRACT: UNCHANGED - fully dynamic enumeration, no hardcoded device
```

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above - including,
  for the first time, a green build with the `whisper` feature actually
  enabled for the Windows target.
- **Environment B (Xvfb)**: **NOT AVAILABLE THIS SESSION**, unchanged
  from Phase 3.8.5's finding (a session/container limitation isolated
  and proven unrelated to code in that phase; not re-attempted this
  phase since nothing about the GUI/webview layer changed).
- **Environment C (real Windows/audio/model hardware)**: **NOT
  VERIFIED.** No physical Windows machine, no real audio hardware, and
  (new to this phase) no real Whisper model file are available in this
  container. See `pilot-evidence/3.8.6/hardware/hardware-status.json`
  and the final gate below.

## Known limitations

- The two Windows/whisper build fixes (ggml lib-naming, mingw threading-
  model alternative) are container/toolchain-level workarounds performed
  directly against this container's installed compiler alternatives and
  the CMake build output, not source patches to any dependency - see
  `scripts/build-windows-whisper.sh`. A future container rebuild, a
  different base image, or a `whisper-rs`/`whisper.cpp` version upgrade
  could change whether either fix is still needed; the script re-detects
  and re-applies the ggml fix on every run rather than assuming it.
- No Whisper model file is bundled or obtainable in this container (see
  "Model packaging" above) - the shipped installer's transcription
  capability remains inert until an operator supplies one.
- The resampling fix is proven by direct unit tests of the pure function
  and by full regression, but has not been exercised against a real
  Windows audio device at a real non-16kHz rate - that requires the real
  Windows/real-audio test this phase's evidence explicitly says has not
  occurred.

## Deferred work

Real Windows test with real audio hardware AND a real local Whisper
model placed by the operator (the hard blocker for PASS, per the
operator's own instruction); investigating whether whisper.cpp/ggml has
since fixed the MinGW static-library-naming defect upstream (would let
`scripts/build-windows-whisper.sh` drop that half of its workaround);
the full aspirational UX redesign (still deliberately out of scope).

## Final gate

Per the operator's own instruction, this phase reports the 12 final-gate
items separately, and does **not** mark the live-audio pipeline PASS
without real Windows audio evidence:

| Gate | Status |
|---|---|
| WINDOWS_DEVICE_ENUMERATION | NOT_VERIFIED (real Environment C test not re-run this phase; Phase 3.8.5 evidence stands for this item unchanged) |
| AUDIO_DEVICE_SELECTION | NOT_VERIFIED (as above) |
| AUDIOENGINE_START | NOT_VERIFIED (as above) |
| CPAL_STREAM | NOT_VERIFIED (as above) |
| INPUT_LEVEL | NOT_VERIFIED (as above) |
| AUDIO_CHUNK_FLOW | NOT_VERIFIED - resampling fix is unit-tested only |
| WHISPER_INITIALIZATION | NOT_VERIFIED - feature is now compiled in and buildable, but no model file exists to actually load |
| SPEECH_FEED | NOT_VERIFIED |
| TRANSCRIPT | NOT_VERIFIED |
| BIBLE_DETECTION | NOT_VERIFIED |
| SERMON_INTELLIGENCE | NOT_VERIFIED |
| OFFLINE_OPERATION | NOT_VERIFIED against real hardware (offline architecture itself is preserved and verified by code/dependency inspection) |

**Phase 3.8.6: NOT PASS - Environment C not verified; no model file
available to test with even if it were.** Phase 3.9 is not started
automatically.
