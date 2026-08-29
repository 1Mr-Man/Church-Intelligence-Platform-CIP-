# Phase 3.8.7.1 — Audit: Whisper Model Provisioning

Written before implementation, per this project's standing discipline and
the operator's own Part A instruction ("do not implement a solution
until this audit is complete").

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `61c80a4` (Phase 3.8.7, diagnostic-coverage audit)

## Trigger

Phase 3.8.7's diagnostics collection (the operator's own real Windows
report) confirmed the exact, single root cause of `SPEECH ERROR`:

```
Whisper model: Not found (expected at
  C:\Users\HP\AppData\Roaming\org.churchintelligence.cip\models\ggml-tiny.en.bin)
Feature compiled: YES
Model loaded: NO (model not found: ...)
Speech engine ready: NO
Audio chunks received: 60,684 (last: 480 samples @ 48,000 Hz)
```

No other defect is indicated. Audio capture, the DLL fix, and the
whisper-feature build are all confirmed working. This phase's sole job is
model provisioning.

## Part A: tracing the existing architecture end to end

### 1. Where the expected model filename is defined

`apps/desktop/src-tauri/src/config.rs:19`:

```rust
pub const WHISPER_MODEL_FILENAME: &str = "ggml-tiny.en.bin";
```

A single constant, referenced nowhere else by a duplicated literal.

### 2. Where the `%APPDATA%`-equivalent model directory is calculated

`AppConfig::from_data_dir` (`config.rs:167-178`):

```rust
let model_dir = data_dir.join("models");
...
let whisper_model_path = std::env::var("CIP_WHISPER_MODEL_PATH")
    .map(PathBuf::from)
    .unwrap_or_else(|_| model_dir.join(WHISPER_MODEL_FILENAME));
```

`data_dir` itself comes from Tauri's own `app_data_dir()` API (resolved
in `AppConfig::resolve`, `config.rs:157-161`) - confirmed in Phase 3.8.6's
audit to never depend on cwd or a dev-relative path, and the operator's
own diagnostics this session (`C:\Users\HP\AppData\Roaming\...`) confirm
this resolves correctly on real Windows. `CIP_WHISPER_MODEL_PATH` is an
env-var override that, when set, is used **verbatim** - never merged with
`model_dir` (see `config.rs`'s own test,
`an operator-supplied path must be used verbatim, never merged with model_dir`).

### 3. Whether bundled resources are already supported

**Yes, mechanically** - Phase 3.8.6.1 proved this for the three MinGW
runtime DLLs via `tauri.windows.conf.json`'s `bundle.resources`. The same
mechanism could bundle a model file into the installer. **But not usable
this phase**: `bundle.resources` requires the file to exist on disk at
build time (confirmed the hard way in 3.8.6.1 - `tauri-build`'s own
`build.rs` validates every resource path even during a plain `cargo
build`), and no model file exists anywhere in this repository or this
build environment (see network findings below). Bundling remains
possible in principle, blocked in practice by the same constraint that
blocked it in Phase 3.8.6.

### 4. Whether the Tauri application has a reliable resource directory

Yes - `tauri::path::PathResolver::resource_dir()` is a well-defined,
platform-correct API this project has not yet used for anything other
than the DLLs bundled via `bundle.resources` (which install to
`$INSTDIR` directly, not a nested `resources/` folder - see Phase
3.8.6.1). Not exercised for the model this phase, for the same reason as
above: nothing to bundle.

### 5. Whether the model is expected to be bundled / manually installed / downloaded / env-var-supplied

**Manually installed today** - Phase 3.8.6's `modelPackagingStatement`
documented this as "Option D" from the operator's original spec: the
operator places a file at the documented path themselves, or points
`CIP_WHISPER_MODEL_PATH` elsewhere. Nothing in the codebase attempts a
download. This audit reconfirms that finding is still accurate, and adds
real, fresh evidence for *why* (see Network findings below) - it is not
merely "not implemented yet", it is currently **not achievable from
inside this build/development environment specifically**, though it may
be entirely achievable from the operator's own Windows machine.

### 6. Whether model discovery supports fallback locations

**No** - `whisper_model_path` resolves to exactly one path: the env-var
override if set, otherwise `model_dir/ggml-tiny.en.bin`. No search chain,
no bundled-resource fallback. This is honestly reflected in
`WhisperModelDiagnostic` (`Missing`/`Unreadable`/`Present` against that
one path) - it never claims to have searched multiple locations because
it doesn't.

### 7. What happens if the model is missing

Fully, granularly handled, not collapsed into a generic error:
`create_speech_engine` (`lib.rs:45-97`) attempts
`WhisperSpeechEngine::load(model_path)`; on failure (file missing, or a
`WhisperContext::new_with_params` error for an unreadable/invalid file)
it falls back to `NullSpeechEngine` (`is_ready() == false` always) and
records the real error text in `SpeechDiagnostics.model_load_error`. The
one gap this phase's own Phase 3.8.7 audit found and fixed already (see
`docs/phase-3-8-7-audit.md` and the commit alongside this doc): every
audio chunk arriving while the engine was not ready was still being
counted as an "inference attempt" and logged as a fresh timeline error -
fixed by checking `speech.is_ready()` before ever calling `feed_audio`.

## A confirmed, previously-undiscovered defect: stale build-commit metadata

Not part of the operator's Part A list, but found while investigating
their side note about the diagnostics panel's build identifier
(`CIP 0.1.0 (487994b63efd)`) looking stale relative to a whisper-enabled
build. Root cause, confirmed by reading `build.rs`: it embeds
`git rev-parse HEAD` via `cargo:rustc-env`, but emitted **no**
`cargo:rerun-if-changed` directive at all. Cargo's default rerun
heuristic only tracks files inside this crate's own package directory,
never `.git/` at the workspace root - so a build run between two commits
with no other `apps/desktop/src-tauri` file changed (exactly Phase
3.8.6.1's shape: DLL packaging fix, no Rust/TS source touched) never
reran the build script, silently keeping the previous build's commit
hash. Fixed this phase (see commit) by explicitly watching `.git/HEAD`
and the ref it resolves to, and by adding a `build_dirty` flag (from
`git status --porcelain`) so a diagnostics reader can tell "exactly this
commit" apart from "this commit plus uncommitted changes" - honest given
this project's own build-then-commit workflow.

## Network findings (fresh evidence, not assumed from Phase 3.8.6)

Re-tested directly this session, not carried over from memory:

```
huggingface.co:443           -> CONNECT rejected, HTTP 403 (policy denial)
cdn-lfs.huggingface.co:443   -> CONNECT rejected, HTTP 403 (policy denial)
ggml.ggerganov.com:443       -> CONNECT rejected, HTTP 403 (policy denial)
github.com                   -> reachable (HTTP 400 on bare root - expected, real connection)
raw.githubusercontent.com    -> reachable, real content served (200, verified against a known file)
api.github.com               -> intercepted by this session's own repo-scoping layer (out of scope
                                 for whisper.cpp's repo specifically), not a raw network block
```

**Conclusion: the standard Whisper model distribution channel
(huggingface.co, including its CDN) is deliberately blocked by this
environment's organization network policy** - not a transient failure,
not a DNS issue, an explicit `connect_rejected` / policy denial recorded
by the egress proxy itself. No attempt was made to route around this via
an unofficial third-party mirror: doing so would both work against a
deliberate security policy and - more importantly for a model file
specifically - provide no way to verify the file's authenticity against
a canonical checksum, which is exactly the kind of "silent substitution"
the operator's own spec said never to do. `github.com`/
`raw.githubusercontent.com` being reachable is not a usable substitute:
no official whisper.cpp model mirror exists there (the project's own
`models/download-ggml-model.sh` script only ever pulled from
huggingface.co), and finding an unofficial one would reintroduce the
same authenticity problem.

## Decision for this phase, given the above

**Option A (bundle a model in the installer) and Option B (in-app
automatic download) cannot be safely completed and verified from inside
this container** - there is no model file to bundle, and no way to
download one, verify its authenticity, or test the download path
end-to-end here. Implementing an untested download feature with a
guessed/unverified checksum would risk shipping code that either rejects
every legitimate download or silently accepts a corrupted one - worse
than not having the feature.

**Option C (operator selects an existing file) is fully implementable
and testable in this environment** - it requires no network access at
all. The operator's own Windows machine almost certainly has ordinary
internet access (this container's block is an environment-specific
policy, not evidence about the operator's network), so "download the
file yourself, then point CIP at it with one click" is the fastest real
path to a working model, and is exactly what this phase implements: a
native file picker (`tauri-plugin-dialog`, the one new plugin this phase
adds, scoped to `dialog:allow-open` only - see
`capabilities/default.json`) feeding a new `install_whisper_model`
command that validates the candidate file by actually attempting to load
it as a real Whisper model (the same `WhisperSpeechEngine::load` call
this application uses at startup - never a magic-byte guess) before
copying it atomically into place.

This is a genuine, working provisioning path today, not a placeholder -
see the phase report for the full implementation and its regression
evidence. Options A/B remain real, valid future work, gated on either a
network policy change for this container or the operator supplying a
canonical checksum from their own machine to pin in code.

## What this audit does NOT change

No resampling, inference, audio-capture, or model-loading *logic*
changes - the operator's own spec said not to touch these without new
evidence, and this phase's diagnostics evidence points entirely at
provisioning, not at any of those code paths.
