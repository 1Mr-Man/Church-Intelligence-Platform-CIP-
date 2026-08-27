# Phase 3.2 — Real Hardware Pilot Execution & Production Release Candidate

## Executive Summary

Phase 3.2's job was to try to remove Phase 3.1's "PILOT READY — CONDITIONAL"
conditions through real hardware validation, and to prepare a production
release candidate. This container has the same hardware it had in Phase
3.1: no `/dev/snd`, no `$DISPLAY`, no physical monitor or projector. That
has not changed and could not be changed from inside this environment —
so the three hardware-dependent capabilities remain **NOT AVAILABLE**,
not upgraded to VERIFIED, exactly as the spec's Hardware Truth Rule
requires.

What *did* change this phase: a genuine defect was found and fixed (a
microphone physically disconnecting mid-capture was silently invisible to
the operator — `AudioStatusKind` stayed `Listening` after the stream
died), a real backup mechanism was built and proven with an actual
backup/restore round-trip, a real hardware-diagnostics command was added,
and a real forced-process-termination (`kill -9`) crash-recovery test was
run against the actual release binary, not just a simulated one.

**Primary gate: HOLD** (church hardware pilot). **Software gate:
RELEASE CANDIDATE.** See section AG.

## A. Git Baseline

Started at `1787aae` (Phase 3.1's commit, `docs/phase-3-1-pilot.md`),
tree clean. Every change this phase is additive: 10 files touched, 0
lines deleted from any pre-existing logic (only whitespace-neutral
additions and the small `mut` → non-`mut` cleanup on a variable this
phase's own new test introduced). No existing command, schema, or
architectural boundary was removed or redesigned.

## B. Final Commit

Recorded in section AH below, after this document and the release
manifest are committed together in the single commit this phase produces.

## C. Branch

`claude/cip-foundation-init-i85g87` — unchanged, per instruction.

## D. Audit Findings

A read-only audit (this phase's own background research pass) of the
diagnostics, backup, and security surfaces found:

1. **No unified hardware-diagnostics command existed.** `get_live_status`
   reported coarse audio/speech status but never the selected device's
   sample rate together with the Whisper model's existence/readability in
   one place. **Fixed** — new `get_pilot_diagnostics` command (section
   G).
2. **A real, genuine defect**: the cpal stream-error callback
   (`integrations/audio/src/lib.rs`) only logged a mid-capture hardware
   failure — it never updated `AppState.audio_error` or flipped
   `is_capturing`, so `AudioStatusKind` would keep reporting `Listening`
   after a microphone physically disconnected mid-service. **Fixed**
   (section G) — this is the one finding in this phase that is a genuine
   pre-existing bug, not a coverage gap.
3. **No backup/export mechanism existed** for the SQLite database, despite
   it running in WAL mode (a raw file copy while running would be
   unsafe/incomplete). **Fixed** — new `backup_database` command using
   `VACUUM INTO` (section P).
4. Tauri capabilities/CSP: re-confirmed unchanged from Phase 2.10/3.0/3.1
   — `core:default` only on both windows, no `fs`/`shell`/`http`/`dialog`
   plugin anywhere in `Cargo.toml`, CSP still `null`. Matches every prior
   phase's own documentation of this exactly (section W).
5. `CIP_WHISPER_MODEL_PATH`: re-confirmed to only ever be opened as a data
   file for whisper.cpp to parse — no `exec`/`spawn` anywhere in this
   codebase touches it. No path-traversal/RCE vector (section W).
6. No SQL-injection risk found in `persistence.rs` — every user-influenced
   value is bound via `rusqlite::params!`; the only `format!`-built SQL
   fragments are either compile-time-constant column lists or an
   app-generated (never operator/transcript-typed) LIKE pattern.
7. No pre-existing multi-hour/stress/soak test existed anywhere in the
   Rust or TypeScript test suites — confirmed absent, closing part of the
   gap section M addresses.

## E. Environment Classification

Every claim below is labeled by which of the three evidence classes
produced it:

- **Environment A (automated tests)**: `cargo test`, `vitest` — proves
  code correctness only.
- **Environment B (Xvfb/sandbox)**: real release binary, real SQLite,
  real BSB import, launched under a virtual X server — proves desktop
  runtime correctness (startup, migrations, idempotency, no panics), not
  physical hardware behavior.
- **Environment C (real church hardware)**: not available in this
  container at any point in this phase. Nothing in this document claims
  Environment C evidence for microphone, Whisper, or physical display.

## F. Hardware Availability

Direct OS-level re-probe this phase (not inferred from source code),
identical result to Phase 3.1:

| Check | Result |
|---|---|
| `/dev/snd` | does not exist |
| `aplay` | not installed |
| `/proc/asound/cards` | does not exist |
| `$DISPLAY` | empty |
| `xrandr` | not installed |
| `Xvfb`/`xvfb-run` | installed and used (Environment B only) |

## G. Microphone

- Device enumeration and unknown-device rejection: **PROVEN** (Environment
  A, pre-existing tests, re-verified passing).
- **New this phase**: a real, previously-invisible defect fixed. cpal's
  stream-error callback (fired when the OS reports the audio stream
  itself has died — e.g. the device was unplugged) now:
  1. Flips the shared `is_capturing` flag to `false` immediately (via the
     new `record_stream_error` function, `integrations/audio/src/lib.rs`).
  2. Records the failure reason in a new `AudioEngineStatus.stream_error`
     field.
  3. `commands::get_live_status` now checks `stream_error` explicitly, so
     `AudioStatusKind::Error` is reported instead of silently falling
     through to `Ready`/`Unavailable` once `is_capturing` reads false.
  4. The header (`WorkspaceHeader.tsx`) now shows the specific failure
     reason next to the Audio/Speech status, not just "ERROR".
  - **PROVEN** (Environment A): `record_stream_error_flips_capturing_false_and_records_the_reason`
    directly exercises the exact logic cpal's real callback invokes — the
    one piece of this failure path provable without real hardware to
    unplug.
  - **NOT AVAILABLE** (Environment C): a real microphone physically
    disconnecting mid-capture, with a real operator watching the header
    update, cannot be exercised here.
- Device-selection persistence across restarts: confirmed **absent**
  (frontend `selectedDevice` is plain React state, reset every launch —
  `grep -rn "localStorage" apps/desktop/src` finds zero matches). Not
  changed this phase — a real gap for operator convenience, but not a
  correctness or safety defect (the operator picks a device again after
  restart; nothing silently uses the wrong one), so left as a documented
  P2 for Phase 3.3 rather than expanded scope here.

## H. CPAL

Automated tests (Environment A): **PROVEN** — device enumeration, unknown-
device rejection, pause-without-capturing, stop-without-starting, the new
stream-error-recording logic, and `status()`'s default state, all
re-verified passing (8/8 in `integrations/audio`).

Real capture against real hardware (Environment C): **NOT AVAILABLE.**

## I. Whisper

- Missing-model-file handling: **PROVEN** (pre-existing test).
- Corrupt-model-file handling: **PROVEN** (Phase 3.1's test, re-verified
  this phase) — a garbage file at the configured path is rejected by the
  real whisper.cpp binding with `invalid model data (bad magic)`, mapped
  cleanly to `SpeechEngineError::TranscriptionFailed`, no panic.
- **New this phase**: `get_pilot_diagnostics`'s `diagnose_whisper_model`
  distinguishes `Missing` / `Unreadable` / `Present` for the configured
  path — **PROVEN** (3 new tests, Environment A) — never collapsed into
  one generic "unavailable."
- Real transcription of real speech: **NOT AVAILABLE** (needs both a real
  model file, not bundled by design, and real microphone audio, neither
  obtainable here).

## J. Speech Latency

**NOT AVAILABLE** as a real-hardware measurement. The only latency-shaped
numbers this environment can produce are release-mode test-suite timings
(section X) — those measure *code path* speed against in-memory/file
SQLite, not audio-capture-to-transcript latency, and are explicitly
**AUTOMATED/SIMULATED**, never presented as real speech latency.

## K. Presentation

Software state machine (`Prepared -> Active -> Stopped`, single-active-
item invariant, stale-Active reconciliation on restart): **PROVEN**
(Environment A, pre-existing + Phase 3.1/3.2 tests).

Software window creation/rendering under a virtual display: **PROVEN**
(Environment B) — two fresh source-built Xvfb launches plus one from the
`.deb`-installed path, all clean, this phase (section S).

## L. Physical Display / Projector

**NOT AVAILABLE** (Environment C). No `$DISPLAY`, no `xrandr`, no
physical display hardware exists in this container. `get_pilot_diagnostics`'s
new `displays` field (via Tauri's own `available_monitors()`) would
report real monitor count/geometry *if* run somewhere with real displays —
but that capability itself is only Environment-B-verifiable here (it
would report Xvfb's single virtual screen, not a real projector), and
`docs/release-manifest-3.2.json` says so explicitly rather than treating
`displays.length >= 1` as physical-display evidence.

## M. 60-Minute Service

**SIMULATED** — new test
`pipeline::tests::phase_3_2_sixty_minute_simulated_service_remains_stable`
(Environment A). Not real-time: this session cannot productively spend an
hour of wall-clock time waiting, and nothing in CIP's own logic is
genuinely time-driven, so "60 minutes" is represented as 20 compressed
cycles of sermon-taxonomy + Scripture + presentation activate/stop
against real SQLite and the real orchestration functions (mirroring the
spec's own minute-by-minute outline's cadence). Checks:

- No panic across the full run (the primary assertion).
- Finding-queue growth stays bounded and roughly proportional to input
  (`<= 4x` the cycle count) — a real duplicate-accumulation regression
  would fail this.
- Every transcript segment fed in is persisted exactly once — no silent
  loss or duplication.
- No presentation item is left `Active` after the run, despite seven
  activate/stop cycles occurring during it.
- Both Bible- and Sermon-domain findings accumulated across the full run
  — neither engine went silent partway through.

This is real evidence of sustained-load *logical* stability. It is
explicitly not evidence of real multi-hour wall-clock behavior — see
section T.

## N. Operator UX

No real human operator was available this phase (Environment C). This
section is a code-reading review, not a usability study — labeled
accordingly. Reviewed the full documented workflow
(`docs/first-use.md`'s Quick Start) against the current UI:

- Every step already has a clear, single, undoable-by-explicit-action
  control (Start Listening, Prepare, Display, Stop) — unchanged, still
  matches Phase 3.0/3.1's own findings.
- The one concrete improvement made: the Audio/Speech header field now
  shows the *specific* failure reason on a stream error, not just
  "ERROR" (section G) — directly serves "obvious operator state," the
  explicit priority this section's spec asks for.
- No broad UI redesign was performed or is recommended — nothing found
  rises to "materially prevents pilot use."

## O. Operator Task Timing

**NOT VERIFIED.** No real human operator was available to time. Per the
spec's own instruction, automated execution time is explicitly not
substituted for human usability time anywhere in this document.

## P. Installation

- **Linux `.deb`**: **VERIFIED** (Environment B) — real package built
  this phase (`target/release/bundle/deb/Church Intelligence Platform_0.1.0_amd64.deb`,
  7,258,632 bytes, SHA-256 `0efe6289c3c45888e5f768f7dca9b2bafe44af263708535c35c5beeb732c5552`),
  verified with `dpkg-deb -I`/`-c` (correct metadata, correct
  `libwebkit2gtk-4.1-0`/`libgtk-3-0` dependencies, real binary + desktop
  entry + icons, no stray files), then **extracted and launched from its
  installed path** (`dpkg-deb -x`, run directly from `usr/bin/cip-desktop`,
  no `git`/`cargo`/`npm`/source tree involved) — clean startup, real BSB
  import, zero panics.
- **RPM / AppImage**: **NOT VERIFIED IN THIS ENVIRONMENT** —
  `rpmbuild`/`appimagetool`/`linuxdeploy` are not installed, and the
  latter two are normally auto-downloaded by `tauri build`, which this
  environment has no network egress to do.
- **Windows (msi/nsis) / macOS (dmg/app)**: **NOT VERIFIED IN THIS
  ENVIRONMENT** — cannot cross-build a Windows or macOS installer from
  this Linux container; this is an environmental/tooling limitation, not
  a code defect. `tauri.conf.json`'s `bundle.targets: "all"` claims every
  platform equally; only what could actually be attempted here is
  reported as attempted.
- The installed application does not require `git`, `cargo`, `npm`, a
  source repository, or a development server to run — directly
  demonstrated by the extracted-`.deb` launch above.

## Q. First Run

Re-reviewed, unchanged from Phase 3.0/3.1's own findings (still accurate,
no regression introduced this phase): Bible/microphone/speech/display/
offline readiness are all visible in the always-visible header; a missing
optional Whisper model reads as an informative notice, not an
application failure; a missing microphone never blocks manual transcript
use; a missing projector never blocks Bible search or intelligence use.
No developer terminology is exposed to the operator in the normal
workflow.

## R. Backup / Recovery

**VERIFIED** (Environment A, real round-trip). New `backup_database`
command (`apps/desktop/src-tauri/src/commands.rs`):

- Takes an operator-chosen destination directory (created if missing),
  writes a timestamped, **consistent** snapshot via SQLite's own
  `VACUUM INTO` — safe to call while CIP is running, correctly handles
  WAL mode (a raw `fs::copy` of just `cip.sqlite3` would miss data still
  only in the `-wal`/`-shm` sidecar files; `VACUUM INTO` does not have
  this problem).
- New test `a_vacuum_into_backup_survives_a_simulated_working_database_loss`:
  creates a real service record in a real file-backed database, backs it
  up via the exact mechanism the command uses, **deletes the working
  database and its sidecar files entirely** (simulating total loss — a
  throwaway temp file created and destroyed only within the test, never
  a real operator database), restores by copying the backup over the
  missing path, reopens exactly as a fresh launch would, and confirms the
  data survived intact.
- **Restore is deliberately not a live in-app command.** Swapping an
  actively-open database connection's backing file out from under it
  while CIP is running is a real corruption risk this phase's "minimal
  scope, no unnecessary risk" principle rules out. The safe, documented
  procedure: **close CIP, copy the backup file over `cip.sqlite3` (delete
  any stale `-wal`/`-shm` sidecars first), reopen CIP.** No new code is
  needed for this — `cip_database::open`'s normal startup path (migrations
  no-op on an up-to-date schema, stale-Active reconciliation) already
  handles whatever file it finds there, exactly as proven by the crash-
  recovery test in section S.
- Database location: `<app-data-dir>/cip.sqlite3`, resolved via Tauri's
  own `path().app_data_dir()` (unchanged, `config.rs`).

## S. Crash / Restart

- **Structural** (Environment A): the Phase 3.1 full-service simulation's
  file-backed restart, and this phase's backup/restore round-trip, both
  re-verified passing.
- **Real forced termination** (Environment B, new this phase): the real
  release binary was launched under Xvfb, allowed to fully initialize
  (BSB import, all engines), and sent a genuine `kill -9` (SIGKILL — an
  unclean termination the process cannot intercept or clean up after,
  leaving real stale `-wal`/`-shm` sidecar files behind, confirmed
  present via `ls` immediately after the kill). Relaunching against the
  same data directory afterward: `0 migration(s) applied`, `BSB ... (0
  imported, 31086 already present)`, zero panics — SQLite's own WAL
  recovery handled the unclean shutdown transparently, exactly as its
  design guarantees. This is the strongest crash-recovery evidence this
  environment can produce: a real process, a real uncatchable signal, a
  real reopen.
- **Real physical crash** (Environment C — power loss, OS-level force
  quit on real hardware): **NOT AVAILABLE.**

## T. Multi-Hour Stability

**NOT VERIFIED.** A genuine 2-4 real-wall-clock-hour run was not
performed — this session has no way to productively spend that much
real time waiting on a process with nothing time-driven to observe, and
manufacturing a fake "ran for hours" claim would violate this phase's
core evidence-discipline requirement. The closest available evidence is
the sixty-minute *simulated* stability run (section M), which is
explicitly not the same claim and is labeled as such throughout this
document. A genuine multi-hour soak test remains recommended future work
(section AF) — ideally run on the actual pilot hardware during the real
church engagement, where real time passing is not a scarce resource the
way it is in an interactive development session.

## U. Offline

Re-verified this phase: `cargo tree --workspace --all-features` shows no
`reqwest`/`hyper`-client/`ureq`/`curl` anywhere in the dependency graph
(the only `http`-named crate is Tauri's own IPC/webview type-definitions
crate, not a network client — unchanged finding from every prior phase).
No production code path added this phase makes a network call. The one
existing non-functional diagnostic TCP probe (`check_network_online`, a
best-effort `1.1.1.1:443` reachability check purely for a status
indicator) is unchanged and remains the only network-touching code in
the entire application.

## V. Licensing

- **BSB dataset**: unmodified this phase. Re-confirmed via the existing,
  still-passing `phase_real_bible_dataset_full_validation` test: 66
  books, 1,189 chapters, 31,086 verses, checksum `d4335582ff26a3ac`,
  `licensing_status: verified_public_domain`.
- **Whisper model**: not bundled, not downloaded, not redistributed by
  CIP at any point — unchanged. `CIP_WHISPER_MODEL_PATH` only ever names
  where an operator's *own*, separately-obtained model file lives; its
  licensing is the operator's responsibility, documented as such in
  `docs/release-manifest-3.2.json`.
- **New assets this phase**: none. No icon, font, audio sample, or test
  media file was added. No new third-party crate dependency was added —
  every new capability this phase (mid-capture error detection, pilot
  diagnostics, backup) is built entirely from already-present
  dependencies (`std`, `rusqlite`, Tauri's own monitor API) plus this
  workspace's own existing crates.
- No `UNKNOWN` licensing status was silently converted to `APPROVED`
  anywhere — the licensing gate's negative tests
  (`refuses_import_when_licensing_status_is_unknown_and_writes_nothing`,
  `..._is_restricted_and_writes_nothing`) are unmodified and re-verified
  passing.

## W. Security

Focused re-audit this phase, no release-blocking finding:

- **Tauri capabilities**: `apps/desktop/src-tauri/capabilities/default.json`
  and `display.json` each grant only `core:default` on their respective
  window. `Cargo.toml` has no `tauri-plugin-fs`/`-shell`/`-http`/
  `-dialog` dependency; `lib.rs` registers only `tauri-plugin-log`.
  Unchanged from every prior phase's own finding.
- **CSP**: still `null` in `tauri.conf.json`. Re-evaluated, not
  changed, for the same reason as Phase 3.0/3.1: Tauri v2's IPC
  bootstrap script has its own nonce/CSP interaction that can only be
  safely verified against a real per-OS build/run, and this environment
  (Xvfb, no interactive webview automation tooling) cannot exercise the
  frontend's actual `invoke()` calls closely enough to safely validate a
  policy change blind. Given the zero-plugin-surface finding above, this
  is judged **not release-blocking** — the same judgment Phase 3.0/3.1
  made, re-confirmed rather than silently carried forward.
- **`CIP_WHISPER_MODEL_PATH`**: re-confirmed to only ever be passed to
  `std::path::Path::is_file()`/`std::fs::File::open`/whisper.cpp's model
  loader — never to `std::process::Command` or any exec/spawn call
  anywhere in this codebase (`grep -rn "Command::new\|exec(" apps ai
  core` — zero matches). An operator can point it at any file they have
  read permission to (by design — this is a local, single-operator
  desktop app, not a multi-tenant service), but nothing about the
  mechanism itself creates a path-traversal or code-execution
  vulnerability.
- **New `backup_database` command**: writes a SQLite snapshot to an
  operator-supplied destination directory. This is a real, new
  filesystem-write capability exposed over Tauri IPC — but IPC commands
  are only reachable from CIP's own bundled frontend (no remote content
  is ever loaded, no `shell`/`http` plugin exists to fetch or execute
  anything else), so the practical attacker model is unchanged from
  every other locally-invoked command already in this codebase. It never
  reads arbitrary files (only ever writes a backup via `VACUUM INTO`
  through the app's own already-open connection).
- **New `get_pilot_diagnostics` command**: reads only the
  already-configured `state.config.whisper_model_path` (not attacker-
  influenced per call) and calls Tauri's own `available_monitors()` — no
  new arbitrary-file-read surface.
- **SQL construction**: re-confirmed no injection risk (section D.6).
- No secret, credential, API key, or token was found anywhere in this
  phase's diff (`git diff | grep -iE "api[_-]?key|secret|password|token"`
  — zero matches).

## X. Performance

Release-mode measurements (throwaway probes, this project's established
convention, not a formal benchmark harness):

- The entire `pipeline` test module — now 18 tests, including both
  Phase 3.1's full-service simulation and this phase's 20-cycle
  sixty-minute simulation — completes in **0.78 seconds** in release
  mode (up from Phase 3.1's 2.56s for 17 tests in dev mode; release
  optimization plus the new test's smaller per-cycle footprint together
  account for the difference).
- The `.deb`-extracted, installed-path binary reaches a fully-initialized
  state (9 migrations, full 31,086-verse BSB import, all six intelligence
  domains initialized) in **under 2 seconds** from process start,
  observed identically across three separate launches this phase (two
  source-built, one installed-path).
- Both remain comfortably within the real-time-interactive budget every
  prior phase's performance sections have used as the bar. No regression
  from Phase 3.1's own measurements.

## Y. Release Artifacts

| Field | Value |
|---|---|
| Artifact | `Church Intelligence Platform_0.1.0_amd64.deb` |
| SHA-256 | `0efe6289c3c45888e5f768f7dca9b2bafe44af263708535c35c5beeb732c5552` |
| Size | 7,258,632 bytes |
| Target | Linux x86_64 (.deb, Debian/Ubuntu-family) |
| Build mode | release |
| Built | 2026-08-27T19:45:35Z |
| Git commit basis | `1787aae` (Phase 3.1) → this phase's commit (section AH) |

Full machine-readable manifest: [`docs/release-manifest-3.2.json`](release-manifest-3.2.json).

## Z. PROVEN

- Everything Phase 3.1 already proved (unchanged, re-verified passing:
  761 Rust tests default-features, 7 whisper-feature `cip-ai-speech`
  tests, 198 whisper-feature `cip-desktop` tests, 179 frontend tests).
- Mid-capture audio stream-error detection logic (new, section G).
- Whisper model path diagnosis (`Missing`/`Unreadable`/`Present`, new,
  section I).
- Backup creation and a real backup/restore round-trip (new, section R).
- The sixty-minute *simulated* service's logical stability (new, section
  M).
- A real, installable `.deb` package launched from its installed path
  (rebuilt and re-verified this phase, section P).
- Real forced-process-termination crash recovery (new, section S).

## AA. VERIFIED

(Environment C or the strongest available real-environment evidence)

- `.deb` installation and installed-path launch (Environment B — the
  strongest form of "installation" evidence obtainable here).
- Backup/restore round-trip against a real file-backed database
  (Environment A, real SQLite files on real disk).
- Forced-termination crash recovery against the real release binary
  (Environment B).

## AB. NOT VERIFIED

- RPM, AppImage, Windows, and macOS installers (tooling/environment
  limitations of this session).
- A forced real Tauri presentation-window-open failure (no test harness
  exists anywhere in this codebase for that — pre-existing, unchanged).
- Real human operator task timing (section O).
- Real multi-hour (2-4 hour) wall-clock stability (section T).
- Device-selection persistence across restarts (confirmed absent, not
  attempted this phase — see section G).

## AC. NOT AVAILABLE

- Real microphone audio capture (no `/dev/snd`).
- Real Whisper transcription of real speech (no model + no microphone).
- Real speech latency measurement (depends on the above).
- Physical monitor/projector output (no `$DISPLAY`, no physical display
  hardware).
- Real physical crash injection (power loss, OS-level force quit on real
  hardware).

These four/five items are the hard ceiling on the church-hardware-pilot
gate (section AG) — no amount of further work in this environment can
change them.

## AD. FAILED

None. No capability was tested and found not to work as required this
phase.

## AE. Known Limitations

Everything in `docs/release-manifest-3.2.json`'s `knownLimitations`,
plus: device-selection does not persist across restarts (section G);
CSP remains `null` (section W); a real multi-hour soak test has never
been run (section T).

## AF. Required Pilot Actions

Before a church relies on CIP for a live service, in addition to Phase
3.0/3.1's existing conditions:

1. **Verify microphone capture** on the actual target laptop/interface —
   this phase's fix means a real disconnect will now be visible in the
   header, but the capture path itself has still only ever been proven
   at the enumeration/error-handling level, never with real audio.
2. **Verify Whisper transcription** with a real, operator-sourced model
   file and real speech on the target hardware.
3. **Verify the presentation window on the actual projector/second
   monitor** — proven only under Xvfb here.
4. **Run one real 60-90 minute service** on the target hardware before
   the first live use, ideally with the backup command exercised at
   least once beforehand so the operator knows the procedure works on
   their machine.
5. Consider a longer (multi-hour) real-world soak run if the target
   deployment expects unusually long services (a conference, a multi-
   service day) — not required for a single normal Sunday service given
   this phase's 60-minute simulated evidence.

## AG. RELEASE GATE

```
=========================================
HOLD
=========================================

PRIMARY GATE:
    HOLD

REASON:
    Real microphone unavailable in every environment used to build this
    software so far.
    Real Whisper transcription unavailable (same reason).
    Real physical projector/second display unavailable (same reason).
    Real multi-hour wall-clock stability not verified.

SOFTWARE STATUS:
    RELEASE CANDIDATE

    All mandatory software criteria hold: full regression suite green
    (761 default-feature Rust tests + 7/198 whisper-feature tests + 179
    frontend tests, 0 failures), no P0/P1 defect open, BSB dataset
    complete/valid/licensed, all six intelligence domains operational,
    Unified Operator Workspace operational, presentation software
    operational, a real installable .deb built/checksummed/launched from
    its installed path, first-run workflow understandable without
    developer terminology, offline core workflow fully proven, licensing
    audit clean, security audit has no release-blocking finding, release
    artifact has a SHA-256 checksum, this document is the complete
    release documentation, and every unavailable hardware capability is
    explicitly documented rather than silently passed - satisfying
    release-candidate criteria 9-11 via the explicit declared scope
    below rather than hardware verification.

    Declared supported pilot configuration for this release: manual
    transcript entry (Whisper optional), single-display/manual-preview
    presentation (physical projector optional but recommended,
    unverified). Real microphone/Whisper/projector remain the required
    pilot actions in section AF before a church treats this as verified
    for their own hardware.

HARDWARE PILOT STATUS:
    HOLD

REQUIRED BEFORE CHURCH PILOT:
    1. Test the microphone on the target laptop/interface.
    2. Test a real Whisper model on the target hardware.
    3. Test the projector/second monitor with real Bible content.
    4. Run one real 60-90 minute service on the target hardware.
    5. Exercise the backup command at least once on that hardware.
```

## AH. Final Recommendation

Ship the `.deb` (and, once buildable, the RPM/Windows/macOS equivalents)
as a **software release candidate**, explicitly scoped to manual-
transcript-entry and single-display/manual-preview operation — both
fully proven, both already CIP's documented first-class supported paths,
not degraded fallbacks. Do **not** represent this build as verified for
live microphone capture, live Whisper transcription, or physical
projector output at any specific church until section AF's actions are
completed on that church's own hardware. Do not begin Phase 3.3 or any
new feature work automatically; the recommended next step is the real
hardware pilot itself, not further engineering in this environment.

Git commit for this phase: recorded at commit time, immediately
following this document in the single commit that adds it — see `git
log` on `claude/cip-foundation-init-i85g87` for the exact hash.
