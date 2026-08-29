# Phase 3.8.6.1 — Windows Runtime Dependency Packaging Fix

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `52ceeaf` (Phase 3.8.6, "Windows Whisper build & packaging
  audit")
- Working tree at start: clean

## Why this phase exists

The operator installed the Phase 3.8.6 whisper-enabled installer on a
real Windows machine and launched it. Windows reported a definitive,
photographed system error, not a hypothetical one:

```
cip-desktop.exe - System Error
The code execution cannot proceed because libstdc++-6.dll was not found.
Reinstalling the program may fix this problem.
```

This is a real Environment C failure that occurs before any application
code runs at all - before the main window, before device enumeration,
before Whisper model loading. The operator's own instruction was
explicit: stop and fix Windows runtime dependency packaging first, then
rebuild a genuinely deployable installer, and only then resume the
Whisper model/inference investigation (Phase 3.8.7).

## Root cause (confirmed by direct PE-import inspection, not assumption)

`x86_64-w64-mingw32-objdump -p` against the actual built
`cip-desktop.exe` lists its full dynamic import table. Filtering out
standard Windows system DLLs (present on every stock Windows 10/11 x64
install) and `WebView2Loader.dll` (already bundled automatically by
Tauri's own NSIS generation, unchanged and untouched by this phase)
leaves exactly one non-system runtime dependency: **`libstdc++-6.dll`**
- whisper.cpp's C++ code, compiled into `cip-desktop.exe` since Phase
3.8.6 first enabled the `whisper` feature, dynamically links against it.

Running the same `objdump -p` inspection against `libstdc++-6.dll`
itself (not assumed - checked directly) shows it further depends on
`libgcc_s_seh-1.dll` and `libwinpthread-1.dll`. Running it again against
those two shows no further non-system dependencies. The full, closed
transitive set is exactly these three DLLs - no fourth is needed.

None of the three ship with a stock Windows installation, and Tauri's
NSIS bundler has no built-in knowledge to include them (it only knows
about its own WebView2 loader). Prior Windows builds (before Phase
3.8.6) never hit this because they were never compiled with the
`whisper` feature and therefore never linked against `libstdc++-6.dll`
at all - this is a defect introduced by Phase 3.8.6 enabling the feature
for the first time, surfaced by the operator's real hardware the moment
they tried to launch it.

Full PE-import audit, transitive-dependency inspection, and per-DLL
sourcing/verification: `pilot-evidence/3.8.6.1/build/runtime-dll-evidence.json`.

## Fix applied

1. **`apps/desktop/src-tauri/tauri.windows.conf.json`** (new) - a
   Windows-only config override, automatically merged by the Tauri CLI
   for Windows builds (`npx tauri build --help`: *"a platform-specific
   file is looked up and merged with the default file by default"*).
   Declares `bundle.resources` with the three DLLs mapped to an empty-
   string destination, which Tauri places at `$INSTDIR` (the installer
   root, same directory as `cip-desktop.exe`) - the directory Windows'
   default DLL search order checks first, rather than a nested
   `resources` subfolder the loader would not find automatically.

2. **`scripts/build-windows-whisper.sh`** (extended) - stages the three
   DLLs into `apps/desktop/src-tauri/windows-runtime/` (gitignored,
   regenerated every run) by resolving them from the **active** MinGW
   toolchain via `x86_64-w64-mingw32-g++/gcc -print-file-name=<dll>`
   (never a hardcoded, GCC-version-specific path), so the same posix-
   threads variant used to build `cip-desktop.exe` itself is used for
   its runtime DLLs too (avoiding an ABI mismatch against the win32-
   threads variant's copies). Each DLL's architecture is verified x86-64
   before proceeding (`file(1)`, hard failure on anything else), and
   debug symbols are stripped (`x86_64-w64-mingw32-strip --strip-debug`)
   to keep the installer size reasonable - `libstdc++-6.dll` alone drops
   from 26.3MB to 4.6MB, verified byte-identical in import/export table
   content before/after via `objdump -p`.

   **A real ordering defect was found and fixed during this phase's own
   testing**: the DLL-staging block was originally placed just before
   the final `tauri build` packaging step (after the two-pass ggml build
   logic). Running the script this way failed immediately with `resource
   path 'windows-runtime/libgcc_s_seh-1.dll' doesn't exist` - because
   `tauri-build`'s own `build.rs` (invoked as part of compiling
   `cip-desktop`, even for a plain `cargo build`, not just `tauri
   build`) eagerly validates every `bundle.resources` path in the merged
   config at **compile** time. The fix was to move the entire staging
   block to immediately after the MinGW toolchain switch, before the
   very first `cargo build` attempt.

3. **Build-time verification, not a one-off manual check** - the script
   now finishes by extracting the actual packaged installer with `7z x`
   into a temp directory and asserting all three DLLs are present,
   failing loudly (and refusing to report success) if the resources
   config silently failed to apply. This runs on every future invocation
   of `scripts/build-windows-whisper.sh`, not just this session.

## Installer-contents verification (the final artifact, not just staging)

Per the operator's explicit instruction to inspect the FINAL
`installer.exe` itself rather than trust the staging directory or config
alone: the actual packaged NSIS installer was extracted directly with
`7z x` and its generated `installer.nsi` inspected. All three DLLs are
placed via `File` directives immediately after `SetOutPath $INSTDIR`,
alongside `cip-desktop.exe` and `WebView2Loader.dll`, with matching
`Delete` entries in the uninstaller section. Full extraction listing,
per-file `file(1)` output, and per-file SHA-256:
`pilot-evidence/3.8.6.1/windows/installer-contents-verification.json`.

Installer size grew from 7,613,226 to 8,458,075 bytes (+844,849 bytes) -
a real, substantial increase despite NSIS's LZMA compression, consistent
with ~4.86MB of additional real DLL content (post-strip) being bundled.

## Full regression result

Rust workspace: `cargo fmt --check`, `cargo clippy --all-targets -- -D
warnings` clean on both default and `--features whisper` builds (no
source logic changed this phase, so no new warnings were possible, but
re-run in full per this project's standing discipline). `cargo test`
(default features) and `cargo test -p cip-desktop --features whisper -p
cip-ai-speech --features whisper`: all green, unchanged pass counts from
Phase 3.8.6's baseline (227 passed for `cip-desktop` with the whisper
feature; 7 passed for `cip-ai-speech` with the whisper feature). Frontend
`typecheck`, `lint`, `test`: clean/unchanged (210 passed). This phase
touched no Rust or TypeScript application source - only build tooling,
platform config, release artifacts, evidence, and docs - so an unchanged
regression result is expected and confirms nothing was broken.

## Windows artifact

- Filename: `Church Intelligence Platform_0.1.0_x64-setup.exe`
- SHA-256: `b737f72b9f870393065777516a58d0a5c5e6e7e9411ccdd3cd8c40578249e177`
- Size: 8,458,075 bytes (up from 7,613,226 bytes for the Phase 3.8.6
  artifact)
- Whisper feature proof: unchanged from Phase 3.8.6 (same `--features
  whisper` compile; see `release/windows/release-manifest.json`'s
  `whisperFeatureProof` for the `cargo tree`/symbol-strings evidence,
  still accurate for this rebuild)
- Runtime DLL proof: `pilot-evidence/3.8.6.1/build/runtime-dll-evidence.json`,
  `pilot-evidence/3.8.6.1/windows/installer-contents-verification.json`,
  and `release/windows/release-manifest.json`'s new `runtimeDependencyProof`
  section

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/.gitignore,
  scripts/build-windows-whisper.sh,
  release/windows/release-manifest.json,
  release/windows/Church Intelligence Platform_0.1.0_x64-setup.exe,
  release/windows/Church Intelligence Platform_0.1.0_x64-setup.exe.sha256
FILES CREATED: apps/desktop/src-tauri/tauri.windows.conf.json,
  docs/phase-3-8-6-1-windows-runtime-packaging.md,
  pilot-evidence/3.8.6.1/*
FILES DELETED: NONE
RUST SOURCE CHANGED: NONE
TYPESCRIPT SOURCE CHANGED: NONE
DATABASE MIGRATIONS ADDED: NONE
BIBLE DATABASE CHANGED: NO
INTELLIGENCE ENGINES CHANGED: NO
SERVICE REPLAY CONTRACT CHANGED: NO
TRANSCRIPT CONTRACT CHANGED: NO
TAURI COMMANDS RENAMED/REMOVED/ADDED: NONE
EVENT CONTRACTS CHANGED: NONE
SPEECHENGINE / AUDIOENGINE TRAITS: UNCHANGED
SECOND AUDIO ENGINE / SECOND SPEECH ENGINE / SECOND AUDIO METER: NONE ADDED
DEVICE CONTRACT: UNCHANGED
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved - the three bundled DLLs are static
  runtime libraries resolved from the local build toolchain, not
  downloaded at install or run time
```

This phase is pure build-tooling/packaging/config - no application logic
changed, matching the narrow scope the operator's own spec described.

## Environment A / B / C

- **Environment A (automated)**: full pass - PE-import audit performed
  with the real toolchain's own `objdump`, DLL architecture verified
  x86-64, packaging automated (not a manual copy), and the final packaged
  installer's contents directly extracted and confirmed to contain all
  three required DLLs at the correct install path. This is now a
  standing, build-time-enforced gate in `scripts/build-windows-whisper.sh`
  itself, not a one-off check.
- **Environment B (Xvfb)**: **NOT AVAILABLE THIS SESSION**, unchanged
  from Phase 3.8.5/3.8.6's finding (a pre-existing container limitation,
  unrelated to this phase - nothing about the GUI/webview layer changed).
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED.** This
  is the whole point of this phase - the prior artifact's Environment C
  failure is what triggered it, and this phase's fix has not yet been
  re-tested on real Windows hardware. That re-test (the operator's own
  Test A/B/C) is the decisive, pending gate.

## Known limitations

- The real-Windows relaunch test (install, launch, confirm no
  missing-DLL error, confirm the main window opens) has not yet been
  performed for this rebuilt artifact - that is this phase's pending
  gate, to be run by the operator.
- If that re-test reports a *different* missing DLL, that is not a
  failure of this phase's audit - it is the next concrete dependency to
  investigate with the same `objdump -p` method, per the operator's own
  framing.
- The DLL-bundling fix is a build-tooling/packaging fix, not a source
  patch - like Phase 3.8.6's two toolchain fixes, it lives entirely in
  `scripts/build-windows-whisper.sh` and `tauri.windows.conf.json` and
  could need revisiting if the MinGW toolchain, `whisper-rs`, or
  `whisper.cpp` versions change.
- Whisper model/inference testing remains explicitly deferred (per the
  operator's own instruction) until this phase's real-Windows gate
  passes.

## Deferred work

The operator's own real-Windows re-test: Test A (fresh install +
launch succeeds), Test B (no missing-DLL error, or - if a different DLL
is reported missing - that specific new dependency), Test C (main UI
opens, Live Service view opens, audio devices enumerate). Only after
this passes does Phase 3.8.7 (Real Whisper Model and Inference
Verification) begin, per the operator's explicit sequencing.

## Final gate

| Gate | Status |
|---|---|
| PE dependencies inspected | DONE - `x86_64-w64-mingw32-objdump -p` against the actual built exe |
| Non-system dependencies identified | DONE - exactly `libstdc++-6.dll`, `libgcc_s_seh-1.dll`, `libwinpthread-1.dll` (closed transitive set) |
| DLL architecture verified | DONE - all three confirmed PE32+ x86-64; script refuses to package a mismatched-architecture DLL |
| Packaging automated | DONE - `scripts/build-windows-whisper.sh` stages DLLs from the active toolchain before the first build; `tauri.windows.conf.json` wires them into the NSIS bundle automatically |
| Installer contains dependencies | DONE - verified by direct `7z` extraction of the actual final `installer.exe`, not just the staging directory |
| Real Windows launch | **NOT YET PERFORMED** - pending the operator's own re-test |
| No missing DLL error | **NOT YET VERIFIED** on real hardware - pending the operator's own re-test |

**Phase 3.8.6.1: Environment A/build-time verification PASS. Real
Windows relaunch (Environment C) NOT YET VERIFIED - this is the
decisive, pending gate before Phase 3.8.7 may begin.** Phase 3.8.7 is
not started automatically.
