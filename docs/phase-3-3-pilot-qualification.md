# Phase 3.3 — Physical Hardware Pilot Qualification & Release Gate

## Executive Summary

Phase 3.3's job was to determine, with real evidence rather than
inference, whether CIP can be qualified for a real physical church
pilot. This phase re-probed hardware directly at the OS level (not from
memory of prior phases) and found the identical result as Phases 3.1 and
3.2: no `/dev/snd`, no `$DISPLAY`, no `xrandr`, no `aplay`. This
container has never had physical audio or display hardware attached in
any phase of this project's history. That fact is unchanged and could
not be changed from inside this environment.

Per this phase's own governing principle — physical hardware evidence is
the *only* thing that can satisfy the hardware/pilot gates, and
Environment A (automated tests) or Environment B (Xvfb) evidence may
never be relabeled as hardware-verified — the honest and correct result
is:

```
SOFTWARE RELEASE CANDIDATE: PASS
HARDWARE QUALIFICATION:     HOLD
PILOT QUALIFICATION:        HOLD
FINAL RELEASE:               HOLD
```

This is not a shortfall to be worked around. It is the outcome this
phase's own spec explicitly names as a legitimate, successful validation
result when hardware genuinely isn't available (see section U, "MOST
IMPORTANT PRINCIPLE"). The wording `PILOT READY — CONDITIONAL` is not
used anywhere in this document, per instruction.

What this phase *did* produce: a formal, code-level Hardware Pilot
Qualification Model (`pilot_evidence.rs`) that makes "Automated PASS !=
Hardware PASS" a structural guarantee rather than a prose promise; a
substantially expanded `get_pilot_diagnostics` operator tool (machine
identity, build commit, database read/write health, Bible integrity, in
addition to the audio/Whisper/display detail already present); a
deterministic hardware qualification checklist; a portable
`pilot-evidence/` evidence package with 15 files across audio, Whisper,
display, service, backup, recovery, performance, security, licensing,
and operator-timing categories, each one honestly labeled by the
environment that produced it; and one more piece of real (not
simulated) crash evidence — a genuine `SIGKILL` sent to the running
release binary's real PID, confirmed by exit code 137, followed by a
real clean reopen.

## A. Git Baseline

Started at `fc690a0` (Phase 3.2's release commit,
`docs/phase-3-2-hardware-pilot.md`), tree clean. Every change this phase
is additive: no existing command, schema, module, or architectural
boundary was removed, redesigned, or had its behavior changed. No new
intelligence engine, no `IntelligenceContext`/Unified Workspace redesign,
no Bible architecture rewrite, no cloud AI/LLM, no OBS/vMix integration,
no new runtime dependency was added — see section R.

## B. Final Commit

Recorded in section T below, after this document is committed together
with the rest of this phase's changes in the single commit it produces.

## C. Branch

`claude/cip-foundation-init-i85g87` — unchanged, per instruction.

## D. Environment Classification (unchanged definition, restated)

- **Environment A (automated)**: `cargo test`, `vitest` — proves code
  correctness only. Never proof of hardware behavior.
- **Environment B (Xvfb/sandbox)**: the real release binary, real
  SQLite, real BSB import, launched under a virtual X server — proves
  desktop/runtime correctness (startup, migrations, idempotency, window
  lifecycle, forced-termination recovery), never physical hardware
  behavior.
- **Environment C (real church hardware)**: a real target machine, a
  real microphone, a real Whisper model file, a real monitor/projector.
  **Not available in this container at any point in this project's
  history**, this phase included. Nothing in this document claims
  Environment C evidence for microphone, Whisper, or physical display
  where none exists.

## E. Direct Hardware Re-Probe

Repeated independently this phase, at the OS level, not assumed from
prior phases:

| Check | Result |
|---|---|
| `/dev/snd` | does not exist |
| `aplay` | not installed |
| `/proc/asound/cards` | does not exist |
| `$DISPLAY` | empty |
| `xrandr` | not installed |
| `Xvfb`/`xvfb-run` | installed and used (Environment B only) |

See `pilot-evidence/hardware-status.json` for the machine-readable
record of this probe.

## F. Hardware Pilot Qualification Model

New this phase: `apps/desktop/src-tauri/src/pilot_evidence.rs` — a
small, standalone module (deliberately not wired into any live command,
per this phase's "no speculative functionality" instruction; nothing in
the running app calls it yet, so it carries `#![allow(dead_code)]` with
an explanatory comment rather than a fabricated caller).

```rust
pub enum EvidenceEnvironment { Automated, Xvfb, RealHardware }
pub enum QualificationStatus { NotTested, Pass, Fail, BlockedHardware, NotApplicable }
pub struct EvidenceRecord { pub environment: EvidenceEnvironment, pub status: QualificationStatus, ... }

pub fn hardware_qualification_status(records: &[EvidenceRecord]) -> QualificationStatus
```

The canonical rule this function enforces, and that its own test suite
(the acceptance test this phase's spec calls for) proves structurally:

- Any number of `Automated`/`Xvfb` `Pass` records, alone, can never
  produce anything better than `BlockedHardware`.
- A `RealHardware` record's own status is authoritative.
- A `RealHardware` `Fail` always wins over any number of other passing
  records — hardware evidence can only be improved by better hardware
  evidence, never outvoted by software evidence.
- No evidence at all is `BlockedHardware`, not `NotTested` — silence
  about hardware is treated as "still blocked," not "unknown/neutral."

Six unit tests cover: automated-only never satisfies hardware
qualification; Xvfb-only never satisfies hardware qualification; every
non-hardware environment combined still blocks; a real hardware pass
record produces `Pass`; a real hardware failure is never masked by other
passing evidence; no evidence at all is `BlockedHardware`. All six pass
— see `pilot-evidence/automated-test-summary.json`.

## G. Expanded Pilot Diagnostics ("real operator tool")

`get_pilot_diagnostics` (`apps/desktop/src-tauri/src/commands.rs`) now
reports, in one call:

- **`machine`** (new): OS, architecture, CIP version, and a build commit
  identifier embedded at compile time by `build.rs` (`git rev-parse
  --short=12 HEAD`, run only at build time — never a runtime process
  spawn, explicitly avoiding the exec-surface concern Phase 3.2's
  security audit flagged as something to never introduce).
- **`whisperModel`**: unchanged three-state diagnostic
  (Missing/Unreadable/Present with real file size).
- **`audioDevices`** / **`audio`**: unchanged device list and engine
  status, including Phase 3.2's `streamError` field.
- **`displays`** (extended): every monitor Tauri's native API can
  detect, now including `scaleFactor` alongside name/size/position/
  primary-flag.
- **`bible`** (new): the live `ContentMetadata` for the installed Bible
  dataset from the content registry, or `null` if none is installed.
- **`database`** (new): path, and real (non-mutating) `readable`/
  `writable` checks against the actual database file — distinct from
  "did migrations apply," which `app_health_check` already answers.

Two new tests: `pilot_diagnostics_serializes_camel_case` (extended to
cover the new fields) and
`cip_git_commit_is_embedded_and_not_the_literal_placeholder`. A
representative structural example of this command's output shape (not a
live capture — no webview-automation tooling exists in this container to
actually invoke it via IPC) is in `pilot-evidence/diagnostics.json`,
explicitly labeled as such.

One implementation note: the `database.writable` check was initially
written as a mutating `CREATE TABLE IF NOT EXISTS` probe, then corrected
to a non-mutating `OpenOptions::write(true).open(...)` check — a
diagnostics *read* command must never itself mutate the database.

## H. Hardware Qualification Checklist (deterministic)

A capability is `RealHardware: Pass` only when **all** of the following
hold; otherwise it is `RealHardware: BlockedHardware` (or `Fail` if
attempted and it failed) — never inferred from Environment A/B:

| Capability | Pass requires |
|---|---|
| Microphone capture | A real audio device enumerated and started on the target machine; a real waveform/level observed; a documented stop with no crash |
| Whisper transcription | A real `.gguf`/`.ggml` model file on the target machine, loaded successfully, producing a transcript from real spoken audio (not silence, not a scripted engine) |
| Speech latency | Measured wall-clock time from real speech to a real transcript segment appearing in the UI |
| Physical display/projector | A second display physically connected and detected by the OS, with real Bible content rendered on it and confirmed readable at operator distance |
| Full service | One real, timed (60-90 min), non-simulated service run end-to-end on target hardware with a real operator |
| Backup/restore | `backup_database` executed and a restore verified, both on the target machine |
| Crash/restart | A real power-loss or OS-level force-quit on the target machine, followed by a clean reopen |
| Operator timing | Task durations observed from a real, non-developer operator |

None of these eight rows currently have a `RealHardware: Pass` entry —
every one is `BlockedHardware`. See `pilot-evidence/hardware-status.json`
`capabilities` object for the machine-readable form of this table.

## I. Microphone (Environment-C-only procedure, not performed here)

Cannot be performed in this container (no `/dev/snd`). The documented
procedure for a pilot operator: connect the target microphone/interface,
run CIP, open the diagnostics panel, select the device, start capture,
speak at a normal service volume for at least 60 seconds, confirm a
non-zero input level is shown, disconnect the device mid-capture and
confirm the UI reports the Phase 3.2 `streamError` state rather than
silently freezing, reconnect and restart capture. Record the device
name, sample rate, and result in `pilot-evidence/audio-test.json`'s
`realHardware` block.

## J. Whisper (Environment-C-only procedure, not performed here)

Cannot be performed in this container (no model file present, and no
microphone to feed it even if one were). Documented procedure: place a
real `.gguf`/`.ggml` model at the configured path, confirm
`diagnose_whisper_model` reports `Present` with the correct byte size,
speak known phrases and Scripture references, measure model-load time
and first-transcription latency, and record results in
`pilot-evidence/whisper-test.json`'s `realHardware` block.

## K. Display / Projector (Environment-C-only procedure, not performed here)

Cannot be performed in this container (no `$DISPLAY`, no `xrandr`).
Documented procedure: connect the actual projector/second monitor,
confirm `get_pilot_diagnostics().displays` reports two entries with the
projector's real resolution/scale factor, activate a short verse and a
long passage on the presentation display, and confirm readability from
the back of the intended room. Record in
`pilot-evidence/display-test.json`'s `realHardware` block.

## L. Full-Service Validation (Environment-C-only procedure, not performed here)

Cannot be performed in this container (requires all of the above plus a
real church and a real operator). Documented procedure: one real,
uncompressed 60-90 minute service, worship plus sermon plus Scripture
presentation, operated by someone other than the developer, with
`pilot-evidence/service-test.json`'s `realHardware` block recording
date, duration, operator, and which of the six intelligence domains were
observed live.

## M. Canonical Acceptance Test (spec section 37)

`pilot_evidence::tests` (section F above) is this phase's canonical,
executable proof that "Automated PASS != Hardware PASS." It is not prose
— it is a passing test suite that would fail if the guardrail function
ever let an `Automated`/`Xvfb` pass alone reach `Pass` or let a real
hardware failure be overruled by unrelated passing evidence.

## N. `pilot-evidence/` Package

A portable, git-tracked evidence directory with 21 files:

```
pilot-evidence/
  machine.json                 build/dev environment (NOT a pilot machine, explicitly labeled)
  hardware-status.json         single source of truth for Environment C status
  automated-test-summary.json  full regression results + new tests + offline/secrets scans
  audio-test.json
  whisper-test.json
  display-test.json
  service-test.json
  backup-test.json
  recovery-test.json
  performance.json
  security.json
  licensing.json
  operator-timing.json
  diagnostics.json             structural example of get_pilot_diagnostics' shape, explicitly not a live capture
  checksums/release-artifact.sha256
  logs/                        5 real Xvfb run logs, including the kill -9 crash/recovery pair
  photos/README.md             explains why it's empty (no Environment C available)
```

Every JSON file separates evidence by `automated` / `xvfb` / `realHardware`
sub-objects; every `realHardware` entry that has no real evidence is an
explicit `blocked_hardware` status with `null` fields, never a fabricated
number.

## O. Crash / Recovery (real, not simulated)

Beyond Phase 3.2's clean-restart evidence, this phase added a genuine
forced-termination test: the real release binary was launched, its
actual OS PID located, and killed with `SIGKILL` (`kill -9`). The
background task exited with code 137 (128+9), independently confirming
a real uncatchable signal was delivered, not a graceful shutdown. Real
stale `-wal`/`-shm` sidecar files were confirmed present immediately
after the kill. Relaunching showed 0 migrations applied, the full BSB
dataset still intact (0 re-imported, 31,086 verses already present), and
zero panics — SQLite's WAL recovery handled the unclean shutdown
transparently. This is the strongest crash-recovery evidence obtainable
without real hardware; it remains Environment B, not Environment C. See
`pilot-evidence/recovery-test.json` and
`pilot-evidence/logs/xvfb-crash-*.log`.

## P. Security & Licensing

No new findings this phase. Re-confirmed: Tauri capabilities unchanged
(`core:default` only, no `fs`/`shell`/`http`/`dialog` plugin), CSP `null`
(accepted risk, re-justified — see `pilot-evidence/security.json`), no
`exec`/`spawn` reachable at runtime (the one new `Command::new("git")`
call is build-time-only), no SQL injection (`rusqlite::params!`
throughout), secrets scan clean. BSB dataset re-confirmed unchanged:
66 books / 1,189 chapters / 31,086 verses / checksum `d4335582ff26a3ac`
/ `verified_public_domain`; the hard import gate remains active; no
UNKNOWN→APPROVED silent conversion occurred; zero new content assets
were added this phase. See `pilot-evidence/security.json` and
`pilot-evidence/licensing.json`.

## Q. Release Artifact

Rebuilt `.deb` from this phase's source, checksummed:

```
fd99faa0cdbe3e9a460af1d1d025017eeeffbc0767f5741ef410bf5f92c8eb32  Church Intelligence Platform_0.1.0_amd64.deb
```

See `pilot-evidence/checksums/release-artifact.sha256`.

## R. Scope Discipline

This phase added zero new intelligence engines, made no changes to
`IntelligenceContext`, the Unified Operator Workspace, or the Bible
architecture, added no cloud AI/LLM, no OBS/vMix integration, no new
runtime dependency (`cargo tree --workspace --all-features` confirms no
new HTTP client crate entered the dependency graph), and made no
database redesign. Every change this phase made falls into one of the
categories this phase's spec permits: hardware diagnosis
(`pilot_evidence.rs`, expanded `get_pilot_diagnostics`), evidence
collection (`pilot-evidence/`), or qualification reporting (this
document).

## S. Regression Suite (final re-run)

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` (default features) | 768 passed, 0 failed |
| `cargo test -p cip-ai-speech --features whisper` | 7 passed, 0 failed |
| `cargo test -p cip-desktop --features whisper` | 204 passed, 0 failed |
| `npm run typecheck` (`tsc -b`) | pass |
| `npm run test` (vitest) | 179 passed, 0 failed |
| `npm run lint` (oxlint) | 0 errors, 2 pre-existing unrelated warnings |
| `npm run build` (vite build) | pass |

See `pilot-evidence/automated-test-summary.json`.

## T. Final Commit

Recorded at commit time, immediately following this document in the
single commit that adds it — see `git log` on
`claude/cip-foundation-init-i85g87` for the exact hash.

## U. MOST IMPORTANT PRINCIPLE (restated per instruction)

This phase never traded honesty for a GO label. The result below is a
legitimate, successful outcome of a qualification phase whose job was to
find the truth, not to produce a pass. Hardware genuinely was not
available in this container in this phase, exactly as in Phases 3.1 and
3.2, and the gate reflects that plainly.

## V. RELEASE GATE

```
=========================================
HOLD
=========================================

SOFTWARE RELEASE CANDIDATE:
    PASS

    Full regression suite green (768 default-feature Rust tests + 7
    whisper-feature ai/speech tests + 204 whisper-feature desktop tests +
    179 frontend tests, 0 failures). No P0/P1 defect open. BSB dataset
    complete/valid/licensed. All six intelligence domains operational.
    Unified Operator Workspace, presentation, backup, and diagnostics
    surfaces operational. A real installable .deb built and checksummed.
    Security and licensing audits clean. Hardware Pilot Qualification
    Model (pilot_evidence.rs) implemented and self-tested. Expanded
    operator diagnostics implemented. Deterministic hardware
    qualification checklist documented (section H). Portable
    pilot-evidence/ package complete (21 files).

HARDWARE QUALIFICATION:
    HOLD

    Zero of the eight hardware qualification checklist rows (section H)
    have RealHardware: Pass evidence. Microphone, Whisper transcription,
    speech latency, physical display, full-service validation,
    backup/restore-on-target, crash/restart-on-target, and operator
    timing are all RealHardware: BlockedHardware — no real audio device,
    no real display server, and no real operator exist in this
    container. This is unchanged from Phase 3.1 and Phase 3.2; this
    phase's own direct OS-level re-probe (section E) confirms it again
    rather than assuming it.

PILOT QUALIFICATION:
    HOLD

    Pilot qualification requires hardware qualification (above) plus a
    real full-service run. Neither exists. Environment C is explicitly:
    BLOCKED — REQUIRED HARDWARE NOT PRESENT.

FINAL RELEASE:
    HOLD

    A final release requires all three gates above to PASS. They do not.

REQUIRED BEFORE ANY GATE ABOVE CAN MOVE TO PASS:
    1. Run CIP on the actual target machine with a real microphone
       connected; complete the section I procedure and record results in
       pilot-evidence/audio-test.json.
    2. Place a real Whisper model on that machine; complete the section J
       procedure and record results in pilot-evidence/whisper-test.json.
    3. Connect the real projector/second display; complete the section K
       procedure and record results in pilot-evidence/display-test.json.
    4. Run one real, uncompressed 60-90 minute service with a real
       operator; complete the section L procedure and record results in
       pilot-evidence/service-test.json and operator-timing.json.
    5. Exercise backup_database and a restore, and a real crash/restart,
       on that same target machine; record in backup-test.json and
       recovery-test.json.
    6. Re-run this phase's own hardware_qualification_status() logic
       (pilot_evidence.rs) against the resulting real EvidenceRecords -
       only real RealHardware: Pass records can move any row in section H
       to PASS.
```

## W. Final Recommendation

Ship the `.deb` as a **software release candidate**, unchanged in scope
from Phase 3.2's declared supported pilot configuration (manual
transcript entry with Whisper optional; single-display/manual-preview
presentation with physical projector optional but recommended). Do
**not** represent this build as hardware-qualified or pilot-qualified at
any specific church until the six required actions above are completed
on that church's own target machine and the results are recorded in
`pilot-evidence/`. Do not begin Phase 3.4 or any new feature work
automatically; the next step is the real hardware pilot itself, carried
out on a real machine outside this environment.

## X. Answer to the Core Question

**Can a real church install CIP on the target machine, connect its
microphone and projector, run a real service, use the intelligence and
presentation workflow, recover from realistic failures, and operate it
without developer intervention?**

Based on Environment C evidence — which is none, and is honestly
reported as none — this cannot be answered "yes" this phase. What can be
said, from Environment A/B evidence: the installer builds and installs
cleanly, the application starts and runs its full intelligence and
presentation pipeline correctly against simulated/manual input, backup
and forced-crash recovery both behave correctly under the strongest
evidence obtainable without real hardware, and the software itself shows
no defect that would block a pilot. What cannot be said: whether a real
microphone captures cleanly on the target machine, whether a real
Whisper model transcribes usably in that room's acoustics, whether the
target projector renders Scripture legibly from the back pew, and
whether a non-developer operator can run the whole thing unassisted.
Those four questions can only be answered by the six required actions in
section V, executed on real hardware, outside this environment.
