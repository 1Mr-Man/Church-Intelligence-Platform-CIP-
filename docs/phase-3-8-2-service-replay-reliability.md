# Phase 3.8.2 — Real Windows Replay & Presentation Reliability

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `1da6279` (Phase 3.8.1)
- Working tree at start: clean

Full audit in `docs/phase-3-8-2-audit.md`, written before any code
changed, investigating the four pieces of real Windows evidence the
operator supplied.

## Why this phase exists

The operator physically tested the Phase 3.8.1 Windows build and reported:

- **Evidence A**: Service Replay blocked with "Start a service first,"
  even after importing a real transcript.
- **Evidence B**: "a service is already active" from a normal workflow
  step.
- **Evidence C**: "Nothing detected yet" / "No theme or key points
  detected yet."
- **Evidence D**: "CIP Presentation Display (Not Responding)" - the
  display window did not reliably render a prepared Scripture.

## Root causes

### A/B - service lifecycle dead-ends

`ServiceReplay.tsx`'s `startReplay()` threw a blocking error instead of
starting a service when none was active; `runFullService()` called
`startService` unconditionally with no guard, reproducing the backend's
correct "already active" invariant error from a normal click. Both are UX
integration gaps, not backend defects - `commands::start_service`'s
`ensure_no_active_service` guard is correct and unchanged.

### C - "nothing detected" was a downstream symptom, not a separate bug

Because Evidence A blocked replay from ever starting, zero segments were
ever fed to the real intelligence commands - "nothing detected" was
completely honest given zero input. Phase 3.8.1 already proved (via
`phase_3_8_1_service_replay_progressive_intelligence_acceptance`) that
real segments produce real, progressive findings. Fixing A resolves C;
no separate intelligence-layer defect was found or needed fixing.

### D - display window initial-state race + close/reopen race

Two independent, provable defects traced by direct code reading (see
`docs/phase-3-8-2-audit.md` sections E-F):

1. `PresentationDisplay.tsx` had no way to learn current state except by
   catching the `PRESENTATION_STARTED` event live - but
   `display_presentation` opens the window and emits that event
   immediately afterward in Rust, while the new window's own JavaScript
   loads and subscribes asynchronously. An event emitted before that
   subscription completes was lost permanently, leaving the display blank
   for the rest of the session.
2. `close_presentation_display` only closed the window; it relied on an
   asynchronous `Destroyed`-event handler to reconcile the active item to
   `Stopped`. A fast Close-then-Reopen-and-Display-another sequence could
   race ahead of that reconciliation and hit
   `PresentationError::AlreadyActive` on the new item.

The literal Windows "Not Responding" title-bar text's exact mechanism is
explicitly marked `NOT VERIFIED` in the audit - this environment has no
WinDbg/ETW/physical-Windows access to prove which failure mode the
operator's screenshot captured. Both fixes below are justified on their
own merits (real, reproducible races, provable by code reading alone)
regardless of which exact symptom they explain.

## Fixes

1. **Combined Start-Service-&-Replay** (`ServiceReplay.tsx`): pressing
   "Start Replay" (relabeled "Start Service & Replay" when no service is
   active) now starts a service automatically when none is active,
   guarded by the same `serviceActive` check used everywhere else in this
   component - never creates a second service. "Run Full Service" is now
   disabled with an inline hint whenever a service is already active,
   so it can no longer reach the backend's "already active" error from a
   normal click.
2. **Display window initial-state hydration** (`commands.rs`,
   `PresentationDisplay.tsx`, `domain/presentation.ts`):
   `PresentationDisplayState` (the existing `get_presentation_display_state`
   command's response type) gains one additive field, `active_slide:
   Option<RenderedSlide>`, computed via the same pure, already-tested
   `render_content` function `display_presentation` already calls for the
   live-event payload - no second rendering system, no new command.
   `PresentationDisplay.tsx` now calls this command once on mount
   (extracted into a pure, unit-tested `resolveHydratedPayload` helper)
   alongside its existing event subscriptions, so the display always
   reflects true current state regardless of event-vs-mount ordering.
3. **Synchronous close reconciliation** (`commands.rs`):
   `close_presentation_display` now calls the existing, already-idempotent
   `clear_active_presentation` before closing the window, making this
   command's return the actual synchronization point instead of depending
   on the asynchronous `Destroyed` handler (which remains, for a manual
   OS-level close, and is a proven no-op here).

No new Tauri command was added. No database migration, no intelligence
engine, no Bible provider/schema, and no event contract changed - see the
diff list below.

## Long transcript / performance

Traced `segmentTranscript`/`playLoop` (unchanged since Phase 3.8.1):
already O(n), already strictly sequential, one segment at a time. A new
regression test (`replay.test.ts`) feeds a ~42,000-character synthetic
single-block transcript (the scale of a real ~52-minute sermon) and
asserts segmentation completes in under 500ms and produces a bounded
number of speech-sized segments (50-1000, each ≤220 characters) - proving
no hang and no unreasonable segment count at realistic scale.

**The operator's actual file
(`tactiq-free-transcript-k-PJ1yu1pZQ.txt`) was never transferred into
this environment** (confirmed via `find / -iname "*tactiq*"` returning no
results) - this session did not have it and does not claim to have used
it. The synthetic fixture above substitutes for it at comparable scale.

## Tests added

- `apps/desktop/src/components/presentationDisplayHydration.ts` +
  `.test.ts` (new, 4 tests): the pure hydration-decision logic extracted
  from `PresentationDisplay.tsx`.
- `apps/desktop/src-tauri/src/presentation.rs`:
  `three_display_stop_close_reopen_cycles_never_leave_a_stale_active_item`
  (new) - directly encodes the spec's "Display Window Reopen Test":
  Display → Stop → Close → Reopen → Display another, three times, proving
  no cycle ever leaves a stale `Active` row blocking the next one.
- `apps/desktop/src/components/servicereplay/replay.test.ts`: new
  long-transcript regression test (above).
- `apps/desktop/src/domain/contracts.test.ts`: updated for the new
  `activeSlide` field.

Both presentation-display fixes are Tauri-command-layer glue
(`AppHandle`/`WebviewWindow` orchestration); this project has no
`tauri::test` harness (a pre-existing, documented convention - see
`presentation_display.rs`'s own module docs) and none was added this
phase. The tests above prove the underlying invariants each fix depends
on at the layer this project's tests can reach; the Tauri command
ordering itself is exercised by real desktop runtime validation
(Xvfb/Windows), not a unit test - consistent with every prior phase.

## Regression

Rust workspace: **786 passed, 0 failed** (up from 785 - 1 new
`presentation.rs` test). `cargo fmt --check`, `clippy -D warnings`: clean.
Whisper feature: 7 passed, 0 failed. Frontend: **208 passed, 0 failed**
(up from 203 - 5 new tests: 4 hydration + 1 long-transcript regression).
`typecheck`, `build`: clean. `lint`: 0 errors, 4 warnings (unchanged from
Phase 3.8.1's baseline - no new warnings).

## Architectural safety diff (section 18)

```
FILES MODIFIED: apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src-tauri/src/presentation.rs,
  apps/desktop/src/components/PresentationDisplay.tsx,
  apps/desktop/src/components/servicereplay/ServiceReplay.tsx,
  apps/desktop/src/components/servicereplay/replay.test.ts,
  apps/desktop/src/domain/contracts.test.ts,
  apps/desktop/src/domain/presentation.ts
FILES CREATED: apps/desktop/src/components/presentationDisplayHydration.ts,
  apps/desktop/src/components/presentationDisplayHydration.test.ts,
  docs/phase-3-8-2-audit.md, docs/phase-3-8-2-service-replay-reliability.md,
  pilot-evidence/3.8.2/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
TAURI COMMANDS CHANGED: NONE (0 renamed, 0 removed, 0 signature changes -
  `get_presentation_display_state` and `close_presentation_display` keep
  identical names/parameters; only `PresentationDisplayState`'s response
  struct gained one additive field, and `close_presentation_display`'s
  internal body gained one already-existing, already-idempotent call)
TAURI COMMANDS ADDED: NONE
EVENT CONTRACTS CHANGED: NONE (confirmed via empty
  `git diff 1da6279 -- apps/desktop/src-tauri/src/events.rs apps/desktop/src/events/`)
INTELLIGENCE ENGINES CHANGED: NONE (confirmed via empty
  `git diff 1da6279 --stat -- core/intelligence/`)
BIBLE DATA CHANGED: NONE (confirmed via empty
  `git diff 1da6279 --stat -- core/bible/ integrations/bible/ database/seed*`)
PRESENTATION CONTRACT CHANGED: additive only - `PresentationDisplayState`
  gained `activeSlide`; `PresentationItem`/`PresentationContent`/
  `RenderedSlide`/the Prepared->Active->Stopped lifecycle are unchanged
  (confirmed via empty `git diff 1da6279 --stat -- presentation/renderer/ core/presentation/`)
NETWORK/CLOUD CAPABILITIES ADDED: NONE (confirmed via empty
  `git diff 1da6279 -- apps/desktop/src-tauri/capabilities/ apps/desktop/src-tauri/tauri.conf.json`)
```

## Windows artifact

Rebuilt this phase - see `pilot-evidence/3.8.2/windows/` for the checksum
and `release/windows/release-manifest.json` for full provenance.

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above.
- **Environment B (Xvfb)**: see `pilot-evidence/3.8.2/xvfb/` - Linux
  runtime/smoke only, never Windows or hardware evidence.
- **Environment C (real Windows hardware)**: **NOT VERIFIED** against
  this rebuilt artifact. No physical Windows machine was accessible to
  Claude Code in this container. The operator's own Phase 3.8.1 Windows
  testing (which surfaced Evidence A-D) was against the *prior* build,
  not this fixed one - per this spec's own explicit instruction, that is
  not converted into PASS evidence for this rebuild.

## Known limitations

- The exact mechanism behind the literal "Not Responding" Windows
  title-bar text was not, and could not be, proven in this environment -
  see the audit's explicit `NOT VERIFIED` note. Both fixes address real,
  independently provable races that are the most plausible proximate
  causes, but this phase does not claim to have reproduced or confirmed
  the exact Windows-side failure.
- The operator's real transcript file was not available in this
  environment; a synthetic fixture of comparable scale was used instead.
- Presentation-display fixes are proven at the layer this project's test
  architecture can reach (no `tauri::test` harness, a pre-existing,
  documented convention) - real confirmation requires the physical
  Windows re-test described in the final gate below.

## Deferred work

Real Windows re-test of this rebuilt artifact (the hard blocker for
PASS), broader Windows-specific WebView2 diagnostics if the display issue
recurs after this fix, the full aspirational UX redesign (still
deliberately out of scope, unchanged from Phase 3.8.1).

## Final gate

Per the operator's own stated bar for this phase: *"The same Windows
laptop that produced 'Not Responding' must successfully replay a real
sermon transcript, produce real intelligence, approve a real Scripture
detection, display it, close the display, reopen it, and display another
Scripture."* That physical re-test has not occurred in this session.

```
AUTOMATED TESTS: PASS
LINUX/XVFB: PASS
REAL WINDOWS MACHINE: NOT VERIFIED
SERVICE REPLAY: PASS (automated)
REAL BIBLE DETECTION: PASS (automated)
REAL SERMON INTELLIGENCE: PASS (automated)
PRESENTATION DISPLAY: PASS (automated invariant only - NOT VERIFIED on real Windows hardware)
DISPLAY REOPEN: PASS (automated invariant only - NOT VERIFIED on real Windows hardware)
LONG TRANSCRIPT: PASS (automated, synthetic fixture)
HISTORY: PASS (automated, unchanged)

FULL OFFLINE WINDOWS SERVICE TEST: HOLD
```

This stops here. Phase 3.9 does not begin automatically.
