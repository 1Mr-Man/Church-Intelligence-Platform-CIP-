# Phase 3.8.7.2 — Real-Time Speech Performance & Detection

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `91a918e` (Phase 3.8.7.1, model provisioning)

## Why this phase exists

The operator installed the Phase 3.8.7.1 artifact, successfully
installed a real Whisper model via the new "Select Existing Model
File" picker, restarted CIP, and started listening on Stereo Mix.
Transcription genuinely activated for the first time - but the
operator also reported: CIP becoming slow/unresponsive, transcript
text poor/repetitive, the Intelligence Feed staying empty, and the UI
sometimes flashing `NO SIGNAL`. The operator's own instruction: audit
the real running implementation first, don't guess, don't redesign the
whole pipeline.

## Audit — see `docs/phase-3-8-7-2-audit.md`

Full runtime path traced with file:line citations for every stage
(cpal callback → resample → buffer → Whisper inference → transcript →
persistence → frontend → intelligence detection). Three findings:

**Finding 1 (root cause)**: `handle_audio_chunk` - including the real,
blocking whisper.cpp inference call it triggers every ~3 seconds - ran
inline on cpal's own real-time audio capture callback thread. Every
other consumer of audio (the acoustic/Music path) was already given a
dedicated worker thread specifically to avoid this; speech was the one
consumer left running inline.

**Finding 2 (direct consequence)**: that same code held
`state.speech_engine`'s mutex for the full inference duration, and the
frontend's own `getLiveStatus` poll (every 3000ms - the same cadence as
Whisper's 3-second inference window) needs that exact lock just to read
`is_ready()`. A poll landing during inference blocks, which is the
mechanical explanation for intermittent `NO SIGNAL`/UI staleness.

**Finding 3 (pre-existing, out of scope)**: Sermon/Content/Cross-Domain
Intelligence are never invoked from the live transcript path at all -
only Bible Intelligence and the separate acoustic Music worker are.
Confirmed by tracing `pipeline.rs::handle_final_transcript` (Bible
detection only) and the frontend's `onTranscriptUpdated` handler
(display-only). This matches this project's own history (Phase 3.8's
`ServiceReplay` screen exists for exactly this reason) - not a
regression, not fixed this phase.

## Fix applied

`apps/desktop/src-tauri/src/commands.rs`: `handle_audio_chunk` now runs
on a dedicated `spawn_speech_worker` thread (new function, mirroring
`spawn_acoustic_worker` exactly), fed by an unbounded `mpsc::channel`.
The cpal audio callback's `sink` closure now only does cheap,
non-blocking work: downmix/RMS (in `integrations/audio`, unchanged) and
two channel sends (`acoustic_tx.try_send` + `speech_tx.send`), then
returns immediately - restoring the real-time-audio contract.

Deliberately unbounded (unlike acoustic's bounded/`try_send` channel):
dropping a chunk mid-Whisper-buffer would reintroduce a different
flavor of audio corruption, and steady-state throughput is not at risk
since the worker's own per-chunk cost is far cheaper than real-time
arrival except during the brief, bounded inference window.

No change to `WhisperSpeechEngine`'s buffering, resampling, Bible
detection, database schema, or event contracts - purely relocates
*where* existing per-chunk work executes.

## Full regression result

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (both
default and `--features whisper`): clean. `cargo test --workspace`:
`cip-desktop` 227/227 passed on both feature configs (unchanged count),
`cip-ai-speech --features whisper` 7/7 passed, every other workspace
crate green, zero failures anywhere. `cargo check --target
x86_64-pc-windows-gnu`: clean. Frontend (untouched this phase):
typecheck/lint (0 errors, 4 pre-existing warnings)/test (210
passed)/build all clean.

## Windows artifact

- SHA-256: `e9fd4d2d28719ca28a6b7269498d0b0f4a542bb12dc2dcde43c28f106032ed7c`
- Size: 8,566,926 bytes (down slightly from 8,572,190 - expected for a
  pure code-reorganization fix, no new dependency)
- Direct proof the fix compiled in: `x86_64-w64-mingw32-strings`
  against the extracted `cip-desktop.exe` finds the mangled symbol for
  `cip_desktop_lib::commands::spawn_speech_worker` - not inferred from
  source, read directly out of the shipped binary.
- Runtime DLLs (Phase 3.8.6.1), model picker (Phase 3.8.7.1), whisper
  feature (Phase 3.8.6): all re-verified present and unaffected - see
  `pilot-evidence/3.8.7.2/`.

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/commands.rs,
  release/windows/*
FILES CREATED: docs/phase-3-8-7-2-audit.md,
  docs/phase-3-8-7-2-real-time-speech-performance.md,
  pilot-evidence/3.8.7.2/*
FILES DELETED: NONE
RUST SOURCE CHANGED: apps/desktop/src-tauri/src/commands.rs only -
  new spawn_speech_worker function, sink closure in start_listening
  changed from an inline handle_audio_chunk call to a channel send
FRONTEND CHANGED: NONE
TAURI COMMANDS ADDED/REMOVED/RENAMED: NONE
EVENT CONTRACTS CHANGED: NONE
SPEECHENGINE / AUDIOENGINE TRAITS: UNCHANGED
DATABASE / MIGRATIONS: UNCHANGED
BIBLE DETECTION LOGIC: UNCHANGED (still the only live-wired intelligence
  domain besides acoustic Music - Finding 3, not addressed this phase)
NETWORK CAPABILITIES: NONE ADDED
OFFLINE ARCHITECTURE: preserved
```

Pure build-tooling/logic-relocation - no application behavior changed
except *where* existing work executes.

## Environment A / B / C

- **Environment A (automated)**: full pass, including direct
  compiled-binary symbol verification of the fix itself.
- **Environment B (Xvfb)**: unavailable, pre-existing, unrelated.
- **Environment C (real Windows hardware)**: **NOT YET VERIFIED for
  this exact artifact.** The decisive pending gate is the operator's
  own re-test: with a real model installed, does CIP stay responsive
  while listening, does the input level stay stable, does
  transcription produce meaningfully better text, and does Bible
  Intelligence find real scripture references when one is spoken.

## Known limitations

- Sermon/Content/Cross-Domain Intelligence remain unwired from the live
  path - a pre-existing, deliberate scope boundary, not addressed here.
- This fix addresses *where* inference runs, not whisper.cpp/tiny.en's
  inherent transcription accuracy - "meaningfully better," not
  "perfect," is the honest bar for the next real test.
- The real-Windows relaunch/listening test has not yet occurred.

## Deferred work

The operator's own real-Windows re-test (responsiveness, transcript
quality, Bible detection with clean input). If Sermon/Content/
Cross-Domain live-wiring is ever wanted, that is a distinct, larger
design decision - deliberately not undertaken here.

## Final gate

| Item | Status |
|---|---|
| Runtime path audited with file:line evidence | DONE |
| Root cause identified and confirmed (not guessed) | DONE - audio-thread-blocking inference, confirmed via direct code citations and a converging second signal (the 3000ms status-poll cadence matching the 3s inference window) |
| Sermon/Content/Cross-Domain scope traced and classified | DONE - pre-existing, out of scope, not a regression |
| Smallest justified fix implemented, architecture preserved | DONE - mirrors the already-proven acoustic worker pattern |
| Full regression green | DONE |
| Windows artifact rebuilt + fix verified in compiled binary | DONE |
| Real Windows re-test (Environment C) | **NOT YET PERFORMED** - pending the operator |

**Phase 3.8.7.2: Environment A verification PASS, including direct
proof the fix is compiled into the shipped binary. Real Windows
relaunch/listening test (Environment C) is the pending, decisive
gate.** Per the operator's own instruction, this is not marked PASS
merely because the code change is sound - only the operator's real
hardware can confirm responsiveness and transcript quality actually
improved.
