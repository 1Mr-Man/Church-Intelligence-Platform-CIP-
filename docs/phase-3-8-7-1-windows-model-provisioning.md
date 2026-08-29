# Phase 3.8.7.1 — Whisper Model Provisioning

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `61c80a4` (Phase 3.8.7, diagnostic-coverage audit)

## Why this phase exists

The operator's real Windows diagnostics (Phase 3.8.7) isolated `SPEECH
ERROR`'s exact root cause: `Whisper model: Not found`, `Model loaded:
NO`, `Speech engine ready: NO`. The whisper feature and the audio
pipeline are both genuinely working - there is simply no model file
installed. The operator's own spec asked for a real provisioning
solution, audit-first, per the project's discipline.

## Audit (Part A) — see `docs/phase-3-8-7-1-audit.md`

Full trace written before implementation. Key findings:

- The expected filename/path resolution logic is already correct and
  well-tested (`config.rs`); the gap is purely "no file has been placed
  there", not a path-resolution bug.
- Bundling a model into the installer (Option A) and in-app automatic
  download (Option B) both require this container to reach the standard
  model host (`huggingface.co`). Re-tested fresh this session: still
  deliberately blocked by organization network policy (`connect_rejected`,
  HTTP 403 on the CONNECT, not a transient failure). No unofficial
  mirror was used as a workaround - that would both work against a
  deliberate policy and provide no way to verify the file's authenticity
  against a canonical checksum.
- Option C (operator selects an already-downloaded file) requires no
  network access at all and is fully implementable and testable here.

**Decision**: implement Option C fully this phase. Options A/B remain
real future work, gated on either a network policy change or the
operator supplying a canonical checksum from their own machine.

## What was implemented

### 1. Model selection + validated, atomic install

- **`apps/desktop/src-tauri/src/commands.rs`**: new `install_whisper_model`
  command (feature-gated). Validates the candidate file by actually
  attempting to load it as a real Whisper model
  (`cip_ai_speech::WhisperSpeechEngine::load`) - the exact same code
  path this application uses at its own startup - before copying
  anything. Never trusts a filename or extension. Copies atomically
  (temp file in the destination directory, then rename) so a crash or
  full disk mid-copy can never leave a half-written file where CIP
  expects a real model. The non-`whisper`-feature build honestly refuses
  (no engine exists to validate against).
- **`apps/desktop/src-tauri/Cargo.toml`** / **`capabilities/default.json`**:
  added `tauri-plugin-dialog` (native file picker), scoped to
  `dialog:allow-open` only - no save dialog, no message boxes, no
  fs/shell/http plugin. `capabilities/display.json` (the presentation
  window) is unchanged and still has zero plugin access.
- **Frontend** (`PilotDiagnosticsPanel.tsx`, `lib/commands.ts`,
  `config/appConfig.ts`): a "Select Existing Model File…" button next to
  the "Whisper model" diagnostic row, shown only when the whisper
  feature is compiled in. Opens the native picker, calls
  `installWhisperModel`, shows the result (or the real error text), and
  refreshes diagnostics. Explicitly tells the operator installing takes
  effect on CIP's **next launch** - `AppState.speech_engine` is
  constructed once at startup and this command does not attempt to
  hot-swap it.
- **`apps/desktop/src-tauri/src/lib.rs`**: also now auto-creates
  `config.model_dir` at startup (it was never created before - only the
  database's own parent directory was, as a side effect of
  `cip_database::open`), so the manual "place a file at this path"
  instructions from Phase 3.8.6 never fail on a missing folder either.

### 2. Two real defects found and fixed while investigating

**Stale build-commit metadata.** `build.rs` embedded `git rev-parse
HEAD` with no `cargo:rerun-if-changed` directive at all. Cargo's default
rerun heuristic only tracks files inside this crate's own package
directory, never `.git/` at the workspace root - so a build with no
other `apps/desktop/src-tauri` file changed (exactly Phase 3.8.6.1's
shape) never reran the build script, silently keeping the previous
build's commit hash. This is the confirmed, direct explanation for the
operator's real diagnostics showing `487994b` (Phase 3.8.5's commit)
on a binary that was actually much newer. Fixed by watching
`.git/HEAD` and the ref it resolves to; also added a `build_dirty` flag
(from `git status --porcelain`) since this project's own workflow always
builds before committing, so the commit hash alone is routinely one
phase behind. Directly verified: this rebuild embeds `61c80a4` (the real
current commit at build time), not a stale value.

**Misleading inference-attempted counter.** `handle_audio_chunk`
incremented `inferences_attempted` for every chunk handed to
`feed_audio`, regardless of whether the engine reported itself ready -
so a real Windows session with no model loaded showed "60,684 attempted
/ 0 succeeded", and wrote a fresh "speech engine not initialized"
timeline row on every single one of those chunks (a real, separate,
previously-undiscovered inefficiency: unbounded timeline growth from a
static condition). Fixed by checking `speech.is_ready()` first (a
reliable, always-available per-engine signal, confirmed by reading both
`NullSpeechEngine` and `WhisperSpeechEngine`'s implementations directly)
and short-circuiting before ever calling `feed_audio` or writing to the
timeline when not ready - a new `chunks_skipped_engine_not_ready`
counter tracks these instead.

## Full regression result

Rust workspace (default features): `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings` clean. `cargo test --workspace`: every
crate green, `cip-desktop`'s own suite 227 passed / 0 failed (unchanged
count). `cargo check --target x86_64-pc-windows-gnu`: clean, including
the new `tauri-plugin-dialog`/`rfd` dependency cross-compiling
successfully. Whisper feature: `cargo clippy -p cip-desktop --features
whisper --all-targets`, `cargo test -p cip-ai-speech --features whisper`
(7 passed) and `-p cip-desktop --features whisper` (227 passed) - both
unchanged pass counts. Frontend: `npm run typecheck`, `npm run lint` (0
errors, 4 pre-existing warnings, unchanged), `npm test` (210 passed,
unchanged), `npm run build` - all clean.

## Windows artifact

- SHA-256: `ab35b87e2d48760dffed2cb243a84cccc420412755f5ff50a2d5e070103036f7`
- Size: 8,572,190 bytes (up from 8,458,075 - consistent with the new
  file-picker dependency and command)
- Runtime DLLs (Phase 3.8.6.1's fix): still present, verified by direct
  7z extraction - see `pilot-evidence/3.8.7.1/windows/installer-contents-verification.json`
- Whisper feature: re-verified via `cargo tree` and real symbol strings
  (`whisper_full_with_state`, `ggml_backend_init`) in this exact binary
- New file-picker plugin: verified via real symbol strings
  (`tauri_plugin_dialog::init`, `tauri_plugin_dialog::desktop::pick_file`)
  in this exact binary
- Build-commit fix: verified - the string `61c80a40` (this session's
  real current-HEAD hash at build time) is embedded, not the stale
  `487994b6` from before

A genuine, real finding surfaced while gathering this evidence:
`x86_64-w64-mingw32-strip --strip-debug` rewrites the PE COFF header's
Time/Date field (and its dependent CheckSum field) to the wall-clock
time of the strip operation - confirmed by stripping the identical
source DLL twice, one second apart, and diffing the `objdump -p` output
(only those two header fields differed; import/export tables and all
code sections were byte-identical). This means the three runtime DLLs'
SHA-256 is **not** stable across rebuilds by construction - not a
security-relevant discrepancy, just a property of this specific PE
tooling. This phase's evidence records the fresh values from this exact
build rather than assuming they'd match Phase 3.8.6.1's.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/build.rs,
  apps/desktop/src-tauri/Cargo.toml,
  apps/desktop/src-tauri/capabilities/default.json,
  apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src-tauri/src/lib.rs,
  apps/desktop/src-tauri/src/state.rs,
  apps/desktop/src/components/workspace/PilotDiagnosticsPanel.tsx,
  apps/desktop/src/config/appConfig.ts,
  apps/desktop/src/lib/commands.ts,
  apps/desktop/package.json,
  release/windows/*
FILES CREATED: apps/desktop/package-lock.json (new - locks the newly
  added npm dependency; no lockfile existed before),
  docs/phase-3-8-7-audit.md, docs/phase-3-8-7-1-audit.md,
  docs/phase-3-8-7-1-windows-model-provisioning.md,
  pilot-evidence/3.8.6.1/windows/real-windows-relaunch-confirmation.json,
  pilot-evidence/3.8.7.1/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
TAURI COMMANDS ADDED: install_whisper_model (new, feature-gated,
  additive only - no existing command's name/signature/return type changed)
TAURI COMMANDS RENAMED/REMOVED: NONE
NEW PLUGIN: tauri-plugin-dialog, scoped to dialog:allow-open on the main
  window only - never granted to the presentation display window
EVENT CONTRACTS CHANGED: NONE
SPEECHENGINE / AUDIOENGINE TRAITS: UNCHANGED - no resampling, inference,
  or audio-capture logic touched this phase
BIBLE/INTELLIGENCE/PRESENTATION: UNCHANGED
NETWORK CAPABILITIES: NONE ADDED to the shipped application - the new
  command never initiates a network request; it only reads a
  operator-selected local file
OFFLINE ARCHITECTURE: preserved
```

## Environment A / B / C

- **Environment A (automated)**: full pass - regression green, installer
  directly extracted and inspected, all proof claims verified against
  the actual compiled binary (not assumed).
- **Environment B (Xvfb)**: still unavailable in this container
  (pre-existing, unrelated to this phase).
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for this
  exact artifact.** The decisive pending gate is the operator's own test:
  install this rebuild, use "Select Existing Model File" to install a
  model they've downloaded themselves, restart CIP, and confirm the
  Diagnostics panel shows `Model loaded: YES` / `Speech engine ready:
  YES`.

## Known limitations

- In-app automatic model download (Option B) was deliberately not
  implemented - see the audit's reasoning. Remains real future work.
- The real-Windows test of the model-picker flow has not yet occurred -
  pending the operator.
- The DLL-checksum non-reproducibility finding (see "Windows artifact"
  above) means future phases should not assert DLL SHA-256 equality
  across rebuilds as a correctness check - byte size and `objdump -p`
  import-table comparison are the meaningful, stable checks.

## Deferred work

The operator's real-Windows re-test of the model-picker flow. If that
succeeds (model loads, engine ready), Phase 3.8.7's original
investigation (real Whisper inference on real captured audio) can
finally proceed with a genuinely working model. Automatic model
download (Option B), gated on network policy or a supplied checksum.

## Final gate

| Item | Status |
|---|---|
| Model-provisioning audit (Part A) | DONE |
| Network re-test (fresh evidence) | DONE - still blocked, documented honestly |
| Model-selection command (validated, atomic) | DONE, regression-tested |
| Native file picker wired + capability-scoped | DONE |
| Build-commit metadata fix | DONE, directly verified against compiled binary |
| Inference-counter semantics fix | DONE, regression-tested |
| Windows artifact rebuilt + verified (Environment A) | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.8.7.1: Environment A verification PASS. Real Windows re-test
(Environment C) is the pending, decisive gate.** Whisper
model/inference testing (the rest of Phase 3.8.7's original scope)
resumes once the operator confirms the model loads on real hardware.
