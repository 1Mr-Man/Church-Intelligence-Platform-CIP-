# Phase 3.4 — Windows Release Candidate & Physical Hardware Qualification

## Objective

Prepare CIP for its first genuine physical-hardware pilot on a Windows 11
laptop (the described target: an HP EliteBook 830 G6, Intel Core i5-8365U,
8 GB RAM, Intel UHD Graphics 620, Windows 11 Pro 25H2, x64), produce a
proper Windows release artifact, build the diagnostics/evidence tooling
the pilot needs, and hold the physical-hardware qualification gate exactly
where the evidence puts it.

This container has never had that machine, a real microphone, or a real
second display attached, at any point in this project's history. That is
unchanged this phase. What changed: this phase actually attempted, and
succeeded at, cross-compiling a real Windows installer from this Linux
container - something Phase 3.2's own release manifest had explicitly
recorded as not possible in this environment. It was possible after all,
once the required toolchain (`rustup target add x86_64-pc-windows-gnu`,
`mingw-w64`, `nsis`) was installed. Everything that toolchain could not
give - a real Windows launch, a real microphone, a real projector, a real
operator - remains exactly as blocked as it has been since Phase 3.1.

## A. Baseline

Started at `7471211` (Phase 3.3's commit, `docs/phase-3-3-pilot-qualification.md`),
tree clean, confirmed via `git status`, `git branch --show-current`, and
`git log -1`. HEAD matched the expected baseline - no divergence from a
prior report was found or assumed.

## B. Final Commit

Recorded in section AH below, after this document and the evidence package
are committed together in the single commit this phase produces.

## C. Branch

`claude/cip-foundation-init-i85g87` - unchanged, per instruction.

## D. Architecture Audit

A read-only audit of the areas this phase's spec named, done by reading
the actual source rather than trusting prior phase reports:

- **Tauri configuration** (`tauri.conf.json`): one static window (`main`,
  800x600), `bundle.targets: "all"`, CSP `null`. A second window
  (`display`) is created dynamically at runtime, never statically
  declared - unchanged since Phase 3.2.
- **Windows build configuration**: none existed before this phase - no
  Windows-specific `tauri.conf.json` overrides, no `.cargo/config.toml`
  linker entry, no CI workflow. Nothing to audit because nothing had ever
  attempted a Windows build in this project's history.
- **Frontend build**: `vite build` + `tsc -b`, platform-agnostic, no
  Linux-specific asset handling found.
- **Application entry point** (`apps/desktop/src-tauri/src/lib.rs`,
  `main.rs`): standard Tauri `run()` bootstrap, no `cfg(target_os =
  "linux")` or other platform-gated code path anywhere in this crate (a
  full grep for `cfg(target_os`, `cfg(unix`, `cfg(windows` across
  `apps/desktop/src-tauri/src` and the whole workspace returned zero
  matches).
- **LiveChurchBrain workspace / WorkspaceHeader**: unchanged component
  tree; already surfaces Bible/audio/speech/output status in plain
  language without developer tooling (Phase 2.9's work). No `get_pilot_diagnostics`
  UI existed before this phase (see section G).
- **PilotDiagnostics**: Phase 3.3's `get_pilot_diagnostics` command
  (machine/whisperModel/audioDevices/audio/displays/bible/database) was
  present and correct, but had **zero frontend consumers** - no operator
  could see any of it without developer tools. Closed this phase (section G).
- **Speech/audio configuration, CPAL implementation**: `integrations/audio`'s
  `CpalAudioEngine` uses `cpal::default_host()` (WASAPI on Windows,
  automatically) - no Linux-specific audio backend assumption anywhere.
  Found and closed one real gap: `AudioEngineStatus` had no way to report
  *which* device was selected or its negotiated channel count (section H).
- **Whisper integration / `CIP_WHISPER_MODEL_PATH`**: unchanged since
  Phase 3.2/3.3 - opened only via `std::fs`/whisper.cpp's own loader,
  never `exec`/`spawn`.
- **Presentation/display implementation, display-window management,
  presentation persistence/reconciliation**: unchanged, already reviewed
  in Phase 3.2/3.3; re-confirmed no Linux-only assumption this phase.
- **BSB production dataset**: unchanged, re-confirmed (section T).
- **Licensing metadata/gate**: unchanged, re-confirmed (section U).
- **SQLite database, backup/restore, crash/restart recovery**: unchanged
  mechanisms; re-exercised this phase against the rebuilt binary (section P/Q).
- **All six intelligence domains, unified operator workspace, event
  system, Tauri command surface**: unchanged, no redesign, verified via
  the full regression suite (section S).
- **Offline behavior, security/capabilities**: re-audited fresh this
  phase (sections V/W).
- **Existing release documentation and artifacts**: `docs/phase-3-2-hardware-pilot.md`,
  `docs/phase-3-3-pilot-qualification.md`, `docs/release-manifest-3.2.json`
  read and left untouched (historical record, not rewritten). Phase 3.2's
  manifest explicitly recorded MSI/NSIS as "cannot cross-build... from
  this Linux container" - this phase re-tested that claim directly rather
  than assuming it (section E).
- **Existing pilot-evidence infrastructure**: Phase 3.3's `pilot-evidence/`
  (flat files) and `pilot_evidence.rs` guardrail module read and left
  untouched; this phase adds a new `pilot-evidence/3.4/` subtree per the
  spec's own requested layout, without altering Phase 3.3's files.

**Gap register produced by this audit** (before any implementation):

1. No Windows build had ever been attempted in this environment - unknown,
   not "impossible," until actually tried.
2. `get_pilot_diagnostics` had no frontend consumer - real diagnostic data
   existed but no operator could see it without dev tools.
3. `AudioEngineStatus` could not report the selected device or channel
   count - a real pilot operator needs "which microphone is selected,"
   not just an idle/capturing flag.
4. `DisplayDiagnostic` had no position field - can't distinguish "extended
   to the right" from "extended below" for a real second monitor.
5. No structured `pilot-evidence/3.4/` evidence package existed yet for
   this phase's specific checklist (audio/whisper/display/presentation/
   service/recovery/operator/stability/security/offline/licensing/release).

All five were addressed this phase; none required new architecture,
new intelligence engines, or a redesign of anything already proven.

## E. Windows Packaging

Direct, empirical re-test rather than trusting Phase 3.2's "cannot
cross-build" note:

```
rustup target add x86_64-pc-windows-gnu
apt-get install -y mingw-w64 nsis
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  npx tauri build --target x86_64-pc-windows-gnu --bundles nsis
```

Result: **it worked.** `cargo check`, then a full `release` build,
compiled cleanly against `x86_64-pc-windows-gnu` for the entire dependency
graph, including `cpal` (WASAPI backend), `rusqlite` (bundled SQLite
compiled via the mingw C toolchain), and `tauri`/`wry`/`webview2-com`.
`file` confirms the result is a genuine `PE32+ executable (GUI) x86-64...
for MS Windows`. Tauri's own bundler then produced a real NSIS installer
(`file`: `PE32 executable (GUI) Intel 80386... Nullsoft Installer
self-extracting archive`) after downloading and hash-verifying its
standard `nsis_tauri_utils.dll` helper.

`--bundles msi` was also attempted and genuinely rejected by the Tauri
CLI on this host (`error: invalid value 'msi'... possible values: deb,
rpm, appimage` even when a Windows `--target` was specified) - MSI
bundling depends on the WiX Toolset, which requires an actual Windows (or
Wine-hosted `candle.exe`/`light.exe`) build, unlike NSIS bundling, which
Tauri implements with the cross-platform `makensis` binary. This is a
real, tested environmental limit, not an assumption: **MSI: BLOCKED (WiX
Toolset requires a native Windows host)**. NSIS was always the preferred
format per this phase's own instructions, and it succeeded.

Tauri's own build output included this exact, unedited warning, carried
forward here verbatim rather than suppressed: *"Cross-platform compilation
is experimental and does not support all features. Please use a matching
host system for full compatibility."* Nothing in this document overrides
that warning - see section AA.

## F. Installation

**Cannot be performed in this container** - no Windows machine, no Wine,
no WebView2 runtime exists here to install into. See
`pilot-evidence/3.4/windows-install/installer-test.json` for the itemized
checklist (WIN-INSTALL-02 through WIN-INSTALL-07: install, launch, exit,
relaunch, uninstall, reinstall), all `BLOCKED`. The one row this container
*could* execute - WIN-BUILD-01, "does the toolchain produce a well-formed
Windows PE binary and NSIS installer" - is `PASS`. Building a binary and
running an installer on a live Windows system are different claims; this
document never conflates them.

## G. First-Run Configuration

`get_pilot_diagnostics` existed (Phase 3.3) but had no UI. Added
`PilotDiagnosticsPanel.tsx` (`apps/desktop/src/components/workspace/`), a
collapsible "System Diagnostics" section rendered directly below
`WorkspaceHeader`, showing in plain language: machine/build identity,
database health, Bible dataset status, microphone count and selected
device, Whisper model status (present/missing/unreadable, with the exact
expected path), and detected-display count. It performs no diagnosis of
its own - purely a display of facts `get_pilot_diagnostics` already
computes, with a manual refresh button. No environment variable editing,
no developer tooling, no source-code editing required to read it.

## H. Microphone

Existing CPAL architecture reused, not replaced - no new audio backend, no
justification existed to introduce one. See
`pilot-evidence/3.4/audio/microphone-cpal-checklist.json` for the 12-row
checklist (MIC-01 through MIC-12) mirroring the spec's own checklist item
for item. All rows `BLOCKED` (no `/dev/snd` in this container, re-confirmed
this phase). One real, if small, gap was found and closed: `AudioEngineStatus`
had no way to report which device is selected or its negotiated channel
count. Fixed in `core/service/src/audio_engine.rs` (`selected_device`,
`channels` fields) and `integrations/audio/src/lib.rs` (tracked in
`CpalAudioEngine`, set on a successful `start()`, persisted through `stop()`
exactly like `stream_error` already does). New test:
`selected_device_and_channels_are_none_before_any_successful_start` -
proves the honest default, since no successful start is possible here.

## I. CPAL

Unchanged implementation, re-verified this phase against
`x86_64-pc-windows-gnu`: `cargo check --target x86_64-pc-windows-gnu`
compiled cpal's WASAPI backend cleanly. This is compile-time evidence
only - cpal has never opened a real WASAPI device in this container or
anywhere else in this project's history.

## J. Whisper

Existing whisper-rs/whisper.cpp integration reused unmodified - no new
speech engine. `pilot-evidence/3.4/whisper/whisper-checklist.json`
separates the four automated tests that already exist (missing/unreadable/
corrupt/present model diagnosis, all `PASS`, built with `--features
whisper` against the real whisper-rs binding) from the ten real-hardware
tests (WHISPER-01 through WHISPER-10, including the explicitly-named
Nigerian/West African English accent test) - all `BLOCKED`.

## K. Speech Latency

No latency measurement instrumentation existed before this phase; none
was added, because there is nothing to measure without real speech
reaching a real Whisper engine - building instrumentation with no signal
to exercise it would itself be exactly the kind of speculative
functionality this phase's spec prohibits. The latency measurement points
this phase's spec names (capture start, segment start, Whisper start/end,
transcript availability, intelligence processing, operator-visible
result) already exist as discrete, timestamped steps in the pipeline
(`pipeline.rs`'s existing perf logging, added in Phase 1.3) - what is
missing is real audio to drive them, not missing code. **REAL SPEECH
LATENCY: BLOCKED.**

## L. Display/Projector

Existing presentation/display-window architecture reused unmodified - no
OBS, no vMix, no new rendering path. Added `positionX`/`positionY` to
`DisplayDiagnostic` (`commands.rs`), reading Tauri's own existing
`Monitor::position()` - the one concrete gap the spec's DISPLAY diagnostic
list named that wasn't already covered. See
`pilot-evidence/3.4/display/display-projector-checklist.json` (DISP-01
through DISP-08, all `BLOCKED`) and
`pilot-evidence/3.4/presentation/presentation-checklist.json`.

## M. Presentation

Unchanged `Prepared -> Active -> Stopped` lifecycle and stale-Active
reconciliation on window `Destroyed`. Re-verified via a fresh Xvfb
relaunch of the rebuilt binary this phase (section S) - proves window
lifecycle correctness only, never physical-projector readability.

## N. Full Service

`pilot-evidence/3.4/service/service-checklist.json`: the Phase 3.1/3.2
simulated full-service and 60-minute tests remain `PASS` (Environment A,
compressed, not real-time). The real, uncompressed, real-operator,
real-hardware 60-90 minute service chain (SVC-01) is `BLOCKED` - this is
named as the single most important Environment C test in the package,
because it is the literal "can a real church run this" proof.

## O. Operator UX

`WorkspaceHeader` (Phase 2.9) plus this phase's new `PilotDiagnosticsPanel`
now together give an operator every fact the spec's first-run
requirements list, in plain language, without developer tooling.

## P. Backup/Recovery

Unchanged `VACUUM INTO` backup mechanism and manual-restore procedure. The
automated backup/restore round-trip test remains `PASS`. Real crash/restart
Cases B through F on the actual Windows target machine are `BLOCKED` - see
`pilot-evidence/3.4/recovery/recovery-checklist.json`. Case A (normal
exit/restart) and a real forced-termination case were re-exercised this
phase in Environment B against the freshly rebuilt Linux binary (section S);
a real `kill -9` SIGKILL against an earlier build of this same binary,
with confirmed exit code 137, is Phase 3.2/3.3's own evidence and is not
re-fabricated here.

## Q. Crash/Restart

See section P and `pilot-evidence/3.4/recovery/recovery-checklist.json`.

## R. Multi-Hour Stability

The Phase 3.2 compressed 60-minute simulation remains the only automated
evidence; it is explicitly not a substitute for real wall-clock hours. A
genuine 2-4 hour real-machine run is `BLOCKED` - see
`pilot-evidence/3.4/stability/stability-checklist.json`. No container-based
multi-hour run was attempted, because even a successful one would still
only be Environment A/B, not the Environment C evidence this gate
actually requires.

## S. Six Intelligence Domains

Not redesigned. Verified via the full regression suite: 768 default-feature
Rust tests (one transient parallel-execution flake investigated and
confirmed pre-existing, unrelated to this phase's diff - see
`pilot-evidence/3.4/software/automated-regression.json`), 7 + 204
whisper-feature tests, 179 frontend tests, all green. `cargo clippy
--workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check`
both clean. A fresh Xvfb relaunch of the rebuilt Linux binary (RUN1 fresh:
9 migrations, 31,086 verses imported; RUN2 idempotent: 0 migrations, 0
re-imported) confirms desktop/runtime correctness for the final,
diagnostics-panel-including build - see
`pilot-evidence/3.4/software/xvfb-relaunch-3.4-run1-fresh.log` and
`...-run2-idempotent.log`.

## T. BSB Dataset

Re-confirmed unchanged this phase: 66 books, 1,189 chapters, 31,086
verses, checksum `d4335582ff26a3ac`, `verified_public_domain`. Zero new
content assets added. See `pilot-evidence/3.4/licensing/licensing-revalidation.json`.

## U. Licensing

Hard import gate re-confirmed active; no UNKNOWN->APPROVED silent
conversion; no new translation imported; no scraping of any kind
performed. Windows packaging's own build-time toolchain dependencies
(mingw-w64, NSIS, Tauri's own `nsis_tauri_utils.dll`) are build tooling on
this container, not code linked into or shipped inside the CIP
application binary - see `pilot-evidence/3.4/licensing/licensing-revalidation.json`
for the itemized reasoning.

## V. Offline

Re-confirmed: `cargo tree --workspace --all-features`, run for both the
native and `x86_64-pc-windows-gnu` targets, shows no HTTP client crate in
either dependency graph. Every core capability remains fully offline. See
`pilot-evidence/3.4/offline/offline-revalidation.json`.

## W. Security

Fresh audit this phase, not a copy of Phase 3.2/3.3's: Tauri capabilities
unchanged (`core:default` only, two windows, zero fs/shell/http/dialog
plugins), CSP `null` (accepted risk, re-justified), no runtime exec/spawn
surface, no SQL injection, no secrets in this phase's diff, unsigned
installer explicitly disclosed (not hidden). The new `selectedDevice`/
`channels`/`positionX`/`positionY` diagnostic fields read only
already-negotiated state - no new file/network/process access. See
`pilot-evidence/3.4/security/security-revalidation.json`.

## X. Performance

No new performance measurement was possible or attempted beyond what
Phase 3.2/3.3 already recorded (Environment A/B only) - see section K for
why real speech latency specifically remains unmeasurable without real
hardware.

## Y. Evidence Package

`pilot-evidence/3.4/` - 14 subdirectories (software, windows-install,
audio, whisper, display, presentation, service, recovery, operator,
stability, security, offline, licensing, release) matching the spec's own
requested layout, each JSON file carrying `testId`/`environment`/
`procedure`/`expectedResult`/`observedResult`/`status`/`evidenceRef`/
`operatorNotes` per record. Every `BLOCKED` row has `observedResult: null`
- nothing fabricated.

## Z. Automated/Xvfb Results

All green - see section S and `pilot-evidence/3.4/software/automated-regression.json`.

## AA. Real Windows Hardware Results

None exist. Every Environment C row across every category in
`pilot-evidence/3.4/` is `BLOCKED`. The cross-compiled installer and
binary are real, verifiable build artifacts - they are not, and this
document never claims they are, evidence of a real Windows launch. Tauri's
own cross-compilation warning (section E) stands unqualified.

## AB. Known Failures

None. Zero `FAIL` status anywhere in this phase's evidence package.

## AC. Blocked Tests

Every row requiring real Windows hardware, a real microphone, a real
Whisper model exercised against real speech, a real second display, a
real operator, or real wall-clock multi-hour operation. Enumerated in
full across the 14 `pilot-evidence/3.4/` subdirectories; the six required
actions are summarized in section AG below.

## AD. Release Artifact

**Windows**: `Church Intelligence Platform_0.1.0_x64-setup.exe` (NSIS,
UNSIGNED), SHA-256 and full manifest in
`pilot-evidence/3.4/release/release-manifest-3.4.json`.
**Linux**: `.deb` rebuilt this phase to include the new diagnostics panel
and diagnostics fields; SHA-256 in
`pilot-evidence/3.4/software/release-artifact-3.4.sha256`. Neither
artifact is committed to git (binary build output, gitignored, matching
every prior phase's precedent) - only their checksums and manifests are.

## AE. Software RC Gate

**PASS.** See section AG for the full criteria.

## AF. Physical Hardware Gate

**HOLD.** Zero of the fifteen mandatory Environment C requirements in
section AG have real-hardware evidence.

## AG. Final Release Gate

**HOLD.** Requires AE and AF both to pass; AF does not.

## AH. Remaining Work

Six real-machine actions, identical in spirit to Phase 3.2/3.3's own list,
now scoped to the real Windows target:

1. Install the NSIS `.exe` on the HP EliteBook 830 G6 (or equivalent),
   launch, exit, relaunch, and (if practical) uninstall/reinstall - record
   in `pilot-evidence/3.4/windows-install/installer-test.json`.
2. Connect a real microphone; run the section H checklist - record in
   `pilot-evidence/3.4/audio/microphone-cpal-checklist.json`.
3. Place a real Whisper model; run the section J checklist - record in
   `pilot-evidence/3.4/whisper/whisper-checklist.json`.
4. Connect a real second display/projector; run the section L checklist -
   record in `pilot-evidence/3.4/display/display-projector-checklist.json`
   and `.../presentation/presentation-checklist.json`.
5. Run one real, uncompressed 60-90 minute service with a real operator -
   record in `pilot-evidence/3.4/service/service-checklist.json` and
   `.../operator/operator-timing-checklist.json`.
6. Exercise backup/restore and a real crash/restart on that same machine,
   and (time permitting) a 2-4 hour stability run - record in
   `.../recovery/recovery-checklist.json` and `.../stability/stability-checklist.json`.

Final commit hash recorded in section B/AH's git-log reference at commit
time - see `git log` on `claude/cip-foundation-init-i85g87`.

Git commit for this phase: recorded at commit time, immediately following
this document in the single commit that adds it.

============================================================
FINAL PHASE 3.4 GATE
============================================================

```
SOFTWARE RELEASE CANDIDATE: PASS

REAL WINDOWS HARDWARE: BLOCKED

REAL MICROPHONE: BLOCKED

REAL WHISPER: BLOCKED

REAL SPEECH LATENCY: BLOCKED

REAL SECOND DISPLAY/PROJECTOR: BLOCKED

REAL PRESENTATION: BLOCKED

REAL 60-90 MINUTE SERVICE: BLOCKED

REAL CRASH/RECOVERY: BLOCKED

REAL BACKUP/RESTORE: BLOCKED

OPERATOR WORKFLOW: BLOCKED

MULTI-HOUR STABILITY: BLOCKED

SECURITY: PASS

OFFLINE: PASS

LICENSING: PASS

------------------------------------------------------------

PHYSICAL HARDWARE PILOT:
HOLD

FINAL RELEASE:
HOLD

------------------------------------------------------------

MANDATORY STATEMENT:

Physical hardware qualification was not completed. Environment C evidence
is BLOCKED. Automated and virtual evidence does not substitute for
physical hardware evidence. Final release remains HOLD.
```
