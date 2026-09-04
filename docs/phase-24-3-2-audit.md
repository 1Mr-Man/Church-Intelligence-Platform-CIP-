# Phase 24.3.2: Quality-Tier Backlog Visibility Status Label

## Trigger

Offered as one of several viable next options after Phase 24.3 (true
dual-tier Whisper): "the quality worker can fall behind and silently drop
jobs under load ... jobsDroppedBacklog in diagnostics is the only signal,
with no UI distinction between 'hardware too slow for this model' and
'transient spike'." The operator asked for this fix specifically.

## The gap

`SpeechQualityDiagnostics.jobs_dropped_backlog` (Phase 24.3) is a
cumulative, lifetime counter - it only ever grows. An operator watching
System Diagnostics mid-service had no way to tell "this counter is 12
because of one bad minute early in the service, everything's fine now"
apart from "this counter is 12 because the quality model has been too
slow for this hardware for the entire service and is still dropping
jobs right now." Both produce the identical displayed number.

## Design decision: a streak, not a duration

The fast tier's own equivalent (`OverloadState`/`classify_overload`)
classifies backlog depth from `queue_pending_ms` - a continuous quantity,
since audio keeps arriving whether or not the engine keeps up. The
quality tier's jobs are discrete and infrequent (at most one per
fast-tier final window, roughly every few seconds) - milliseconds of
"backlog" isn't a meaningful unit here. What *is* meaningful is a
**streak**: how many jobs in a row were dropped since the worker last
actually processed one. A single drop is the expected, usually-harmless
case (the fast tier produced two final windows close together while the
quality worker was still mid-decode on the first); a streak of several in
a row is real, sustained evidence the configured model is too slow for
this hardware at this cadence - not a coincidence.

## What changed

- `apps/desktop/src-tauri/src/state.rs`: `SpeechQualityDiagnostics` gained
  `consecutive_jobs_dropped: u64` - reset to 0 by `spawn_quality_worker`
  the moment it dequeues and processes any job (whatever the outcome:
  a real correction, silence/placeholder, or even a `transcribe_once`
  error - all are evidence the worker isn't stuck), incremented by
  `handle_audio_chunk` every time `try_send` fails. Distinct from the
  pre-existing, still-present cumulative `jobs_dropped_backlog`.
- `apps/desktop/src-tauri/src/commands.rs`:
  - New `QualityBacklogState` enum (`Normal`/`Busy`/`FallingBehind`/
    `Overloaded`) - mirrors `OverloadState`'s own shape, computed by a
    new pure `classify_quality_backlog` function against three small
    integer thresholds (1/2/3 consecutive drops), the same
    directly-testable style `classify_overload` already established.
  - `SpeechQualityRuntimeDiagnostics` gained `consecutive_jobs_dropped`
    (the raw signal) and `backlog_state` (the derived label, computed
    fresh at every `get_pilot_diagnostics` read - never stored, same
    discipline `overload_state` already follows).
  - 5 new unit tests for `classify_quality_backlog`, mirroring
    `classify_overload`'s own existing test suite (boundary-exact at each
    threshold, and a "recovers back to normal" property test proving no
    hidden hysteresis).
- Frontend: `config/appConfig.ts` mirrors both new fields;
  `PilotDiagnosticsPanel.tsx` gained a "Backlog status" row with a
  human-readable label (`QUALITY_BACKLOG_STATE_LABELS`, mirroring the
  fast tier's own `OVERLOAD_STATE_LABELS`) plus the raw consecutive-drop
  count when non-zero.

No new commands, no new events, no new migration - purely additive
diagnostics.

## Why this is safe

`consecutive_jobs_dropped` only ever affects diagnostics display - it is
never read by any decision-making code (the quality worker's own
backpressure behavior, drop-newest-on-full-channel, is completely
unchanged). The reset point (right after `transcribe_once` returns, before
matching on its outcome) is deliberately outcome-independent: a worker
that successfully dequeues a job but gets `Ok(None)` (silence) or an
`Err` from the engine is still proven to be alive and keeping up with the
channel, which is exactly what the label needs to reflect.

## Testing boundary

`classify_quality_backlog` is a pure function directly tested (5 new
tests, all passing). The reset/increment wiring itself (real thread
coordination between `handle_audio_chunk` and `spawn_quality_worker`)
needs a real quality engine actually processing jobs under load to
observe end to end - the same limitation every other quality-tier
behavior in this project carries (`docs/phase-24-3-audit.md`'s own
Testing boundary section).

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --workspace --all-targets
-- -D warnings` clean, and again with `--features whisper` on the desktop
crate - clean. `cargo test --workspace` 373/373 `cip-desktop` unit tests
in both feature configs (368 + 5 new `classify_quality_backlog` tests).
Frontend: `npm run typecheck` 0 errors, `npm run lint` the same 4
pre-existing warnings (unchanged), `npm run test -- --run` 303/303
(unchanged - no new pure-logic helper was added on the frontend side,
only a lookup table mirroring an existing one), `npm run build` clean.

## Final gate

Environment A (fmt/clippy/test in both feature configs, frontend
typecheck/lint/test/build): PASS. This is a diagnostics-visibility fix,
not a behavior change to the quality tier's own backpressure handling -
it does not close Environment C for the dual-tier pipeline as a whole.
See `docs/phase-24-3-audit.md`'s own Known Limitations for the standing
gap.
