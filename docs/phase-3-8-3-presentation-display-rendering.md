# Phase 3.8.3 — Real Presentation Output Rendering & Windows Display Verification

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `61205b5` (Phase 3.8.2, "Real Windows Replay & Presentation Reliability")
- Working tree at start: clean

Full audit in `docs/phase-3-8-3-audit.md`, written before any code changed,
re-tracing the complete detect -> approve -> prepare -> display path listed
in the operator's spec.

## Why this phase exists

The operator physically tested the Phase 3.8.2 build. The service-lifecycle
and intelligence fixes now work correctly: Service Replay starts, a real
269-segment sermon transcript processes sequentially, Bible/Sermon
Intelligence produce real findings, Needs Attention/Intelligence Feed are
populated, and the "CIP Presentation Display" window opens. The **only**
remaining defect: that window renders completely blank. The operator was
explicit that this is a different symptom than Phase 3.8.2's "Not
Responding" - the window is responsive and correctly labeled, it simply
shows nothing.

## Root cause

**Not confirmed by direct reproduction on real Windows/WebView2** - no
physical Windows machine was accessible in this container. This section
states the best-supported explanation, not a proven fact, per the
operator's own instruction to be explicit about that distinction.

A real, systematic Xvfb + xdotool + xwd end-to-end reproduction of the
actual compiled binary (not a simulation - see `docs/phase-3-8-3-audit.md`
sections D-H) found the entire detect -> approve -> prepare -> display ->
real BSB text rendering -> stop -> close -> reopen -> display-a-different-
item pipeline working correctly on Linux/WebKitGTK, every time it was run,
including under deliberately adversarial near-zero-delay timing between
Prepare and Display. This directly rules out, with real pixel evidence
rather than reasoning, seven of the ten hypotheses the operator's spec
listed: A (React never mounts), B (mounts but payload null), C (payload
exists but slide empty), D (CSS not loaded), E (event never reaches the
display webview, as a *permanent* failure), G (display webview loads the
wrong document), H (renderer returns undisplayable content).

The best-supported remaining explanation is hypothesis J: a known
Tauri/WRY/WebView2-specific defect class where a newly created secondary
webview window does not paint its initial frame until it receives a
resize/redraw signal - the window exists, is responsive, loads its
content correctly, but shows nothing until moved or resized.

## Evidence proving the root cause

This phase's own re-verification pass against the *rebuilt* binary (with
the fix and diagnostics both included) produced direct, real evidence of
the exact race the Phase 3.8.2 hydration fix was designed to cover -
proving it happens on essentially every real invocation in this
environment, not just under artificial timing. The rebuilt binary's own
log (`pilot-evidence/3.8.3/e2e/diagnostic-log-checkpoints.log`) shows:

```
[15:27:27] [diagnostic] display window created (checkpoint 1)
[15:27:27] [diagnostic] display_presentation: about to emit PresentationStarted ... (checkpoint 14)
[15:27:28] [diagnostic] display window: mounted - PresentationDisplay component mounted (checkpoint 3)
[15:27:28] [diagnostic] display window: hydration-result - windowOpen=true activeItem=true activeSlide=true (checkpoint 6)
[15:27:28] [diagnostic] display window: payload-applied - source=hydration heading=MAT 6:9 ... (checkpoints 9-12)
```

The backend emits `PresentationStarted` a full second before the
frontend's React component even mounts. The event is therefore genuinely
missed by the listener on essentially every real run in this environment;
the display only shows correct content because the Phase 3.8.2 on-mount
hydration call independently recovers true current state afterward. This
is real, direct proof that the hydration fallback is not a defensive
nicety but the actual, load-bearing mechanism - and it strengthens (though
does not prove) hypothesis J: if the equivalent WebView2-side paint delay
on Windows is longer than this Linux mount delay, the window could remain
visually blank even though the same hydration mechanism has already
correctly set React state, because nothing has forced WebView2 to paint
that state yet.

## Exact fix

A single, Windows-only, explicit resize call immediately after the
display window is created, in `presentation_display.rs`'s
`open_display_window`:

```rust
#[cfg(target_os = "windows")]
{
    if let Err(e) = window.set_size(tauri::LogicalSize::new(1280.0, 720.0)) {
        log::warn!(..., "failed to nudge the display window's initial paint via resize: {e}");
    }
}
```

This compiles to nothing on any other platform (confirmed via
`cargo check --target x86_64-pc-windows-gnu -p cip-desktop`, since ordinary
`cargo check` on this Linux container cannot see `#[cfg(target_os =
"windows")]`-gated code at all). It does not touch the event contract, the
renderer, the lifecycle, or any other platform's already-proven-correct
behavior.

## Why the fix is sufficient (and its limits)

It is sufficient *if* hypothesis J is the true cause: forcing an explicit
resize immediately after window creation is the standard, minimal
mitigation for this WebView2 defect class, and it composes correctly with
the existing Phase 3.8.2 hydration mechanism regardless of exact timing.
It is **not proven** sufficient, because it was not tested against a real
Windows/WebView2 runtime. Per the operator's own explicit instruction, if
the display is still blank after this fix on the next real Windows test,
the correct next step is to stop and read the diagnostic log rather than
stack further speculative changes - which is exactly why the 14-checkpoint
instrumentation was added in this same commit, not deferred.

## Temporary diagnostic instrumentation

Fourteen checkpoints along the full path (window created, WebView loaded,
component mounted, effect ran, hydration called/returned, event
received/stopped, payload applied, renderer output, emit-lifecycle
ordering) are now logged via `log::info!` under the existing
`cip::presentation` log target, reachable from the display window's own
JavaScript via a new minimal command, `log_display_diagnostic(stage,
detail)`, which does nothing but forward its two string arguments into
the existing log stream. No state read or written, no capability beyond
`core:default`, nothing persisted or sent anywhere else. This is
explicitly temporary, development-only instrumentation - intended to
directly answer, from the log file alone, "which checkpoint failed"
without further guessing on the next real Windows test.

### A real defect in the diagnostic instrumentation itself, found and fixed this phase

While re-verifying the rebuilt binary in Xvfb, the diagnostic checkpoints
did not appear anywhere in the log output at all - not because anything
was broken, but because this app's log level is hardcoded to
`log::LevelFilter::Info` in `apps/desktop/src-tauri/src/lib.rs` (this
project has no `env_logger`; `RUST_LOG` has no effect on it). The
checkpoints had been written using `log::debug!`, which that filter
silently drops. This meant the instrumentation - the exact tool the
operator would need if the resize-nudge fix turns out to be insufficient
- would have been completely useless on the next real Windows test. Fixed
by changing the checkpoint calls (only) from `log::debug!` to
`log::info!`; re-verified via `grep '\[diagnostic\]'` against the final
rebuilt binary's log output, confirming all 14 checkpoint categories now
appear as designed.

## Real E2E reproduction (this phase's central evidence)

Built and ran the actual compiled `cip-desktop` binary's real GUI under
Xvfb, driven by `xdotool` (mouse/keyboard) and captured via `xwd`+`convert`
(direct `XGetImage`, proven more reliable than ImageMagick's `import` for
overlapping windows with no window manager present). Full click-through
sequence, repeated against both the pre-fix and the final rebuilt binary:

1. Start Service -> expand Manual/test transcript entry -> submit "Turn to
   Matthew chapter 6 verse 9." -> Bible detection MAT 6:9 at 97% confidence
   -> Approve -> Prepare -> real BSB text visible in the "Ready to Present"
   card -> **Display**.
2. Real pixel screenshot of the separate `CIP Presentation Display` window:
   `MAT 6:9 / "So then, this is how you should pray: 'Our Father in
   heaven, hallowed be Your name." / BSB` - correctly styled, gold heading,
   white body, black background.
3. **Stop** -> display card returns to "Nothing prepared yet" while
   `PresentationStopped` (checkpoint 8) is logged; **Close Display** ->
   `DISPLAY CLOSED`.
4. Submitted "Turn to Romans chapter 8 verse 28." -> ROM 8:28 detected at
   97% -> Approve -> Prepare -> real BSB text ("And we know that God works
   all things together...") -> **Display** (reopening the window fresh).
5. Real pixel screenshot of the reopened display window: correct ROM 8:28
   text, proving the full close -> reopen -> display-a-different-item
   cycle works with zero stale state.

All screenshots and the full diagnostic log are saved under
`pilot-evidence/3.8.3/e2e/`.

## Windows result

**NOT VERIFIED.** No physical Windows machine was accessible to Claude
Code in this container. The Windows installer was rebuilt
(`docs/phase-3-4-windows-pilot.md`'s cross-compilation toolchain,
unchanged), its Windows-only code path was independently verified via
`cargo check --target x86_64-pc-windows-gnu -p cip-desktop` (real compiler
verification, not inspection), and its embedded application binary was
confirmed genuinely x64 via `file(1)`. None of this substitutes for a real
Windows/WebView2 launch.

## Presentation rendering result

**PASS on Linux/WebKitGTK (Environment B), NOT VERIFIED on Windows
(Environment C).** See "Real E2E reproduction" above.

## Close/reopen result

**PASS on Linux/WebKitGTK (Environment B), NOT VERIFIED on Windows
(Environment C).** See step 3-5 above; also covered by the pre-existing
`three_display_stop_close_reopen_cycles_never_leave_a_stale_active_item`
Rust test from Phase 3.8.2, unchanged and still passing.

## Real BSB result

**PASS.** `build_scripture_slide()` -> `render_content()` -> `RenderedSlide`
was confirmed producing the real local BSB text for both MAT 6:9 and ROM
8:28 - visible directly in the real pixel screenshots, not fabricated test
content.

## Full regression result

Rust workspace (default features): **786 passed, 0 failed** (flat vs. the
Phase 3.8.2 baseline - this phase added diagnostic logging and one
additive command, not new unit-testable domain logic; the fix itself is
proven via real Xvfb reproduction, consistent with this project's
documented no-`tauri::test`-harness convention for Tauri-command-layer
glue code). `cargo fmt --check`, `clippy --all-targets -- -D warnings`:
clean. Whisper feature: 7 passed, 0 failed. Cross-compilation check for
the Windows-only code path: clean. Frontend: **210 passed, 0 failed** (up
from 208 - 2 new `commands.test.ts` cases for `logDisplayDiagnostic`).
`typecheck`, `build`: clean. `lint`: 0 errors, 4 pre-existing warnings
(unrelated files, unchanged from the Phase 3.8.2 baseline).

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/presentation_display.rs,
  apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src-tauri/src/lib.rs,
  apps/desktop/src/lib/commands.ts,
  apps/desktop/src/lib/commands.test.ts,
  apps/desktop/src/components/PresentationDisplay.tsx
FILES CREATED: docs/phase-3-8-3-audit.md,
  docs/phase-3-8-3-presentation-display-rendering.md,
  pilot-evidence/3.8.3/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
BIBLE DATABASE CHANGED: NO
INTELLIGENCE ENGINES CHANGED: NO
SERVICE REPLAY CONTRACT CHANGED: NO
TRANSCRIPT CONTRACT CHANGED: NO
TAURI COMMANDS RENAMED/REMOVED: NONE
TAURI COMMANDS ADDED: 1 (log_display_diagnostic - temporary diagnostic
  only, logs its two string arguments verbatim, reads/writes no state)
EXISTING COMMAND SIGNATURES CHANGED: NONE (confirmed via
  `git diff 61205b5 -- apps/desktop/src-tauri/src/lib.rs` showing exactly
  one added registration line)
EVENT CONTRACTS CHANGED: NONE (confirmed via empty
  `git diff 61205b5 -- apps/desktop/src-tauri/src/events.rs apps/desktop/src/events/`)
PRESENTATION LIFECYCLE: Prepared -> Active -> Stopped unchanged (confirmed
  via empty `git diff 61205b5 --stat -- presentation/renderer/ core/presentation/`)
PERSISTENCE: unchanged
OFFLINE ARCHITECTURE: preserved (confirmed via `cargo tree` - no HTTP
  client crate in the dependency graph)
NETWORK CAPABILITIES: NONE ADDED (confirmed via empty
  `git diff 61205b5 -- apps/desktop/src-tauri/capabilities/ apps/desktop/src-tauri/tauri.conf.json`)
NEW PRESENTATION ENGINE: NONE - the architecture remains exactly
  Backend: RenderedSlide -> PresentationStarted payload -> PresentationDisplay
  -> HTML/CSS -> Windows display, with one presentation renderer
NEW RENDERER: NONE
```

## Windows artifact

Rebuilt this phase - see `pilot-evidence/3.8.3/windows/` for the checksum
and `release/windows/release-manifest.json` for full provenance.

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above.
- **Environment B (Xvfb)**: full pass - real GUI automation, real
  screenshots, real diagnostic log output; see `pilot-evidence/3.8.3/xvfb/`
  and `pilot-evidence/3.8.3/e2e/`.
- **Environment C (real Windows hardware)**: **NOT VERIFIED** against this
  rebuilt artifact. No physical Windows machine was accessible to Claude
  Code in this container. The operator's own Phase 3.8.2 Windows testing
  (which surfaced the blank-display symptom) was against the *prior*
  build, not this fixed one - per this phase's own explicit instruction,
  that is not converted into PASS evidence for this rebuild.

## Known limitations

- The Windows-only resize-nudge fix is the best-supported remaining
  explanation, not a confirmed root cause - it could not be directly
  reproduced or confirmed without a real Windows/WebView2 environment.
- The 14-checkpoint diagnostic logging is temporary, development-only
  instrumentation. If the real Windows re-test still shows a blank
  display, the next step is to read the log file directly (no `RUST_LOG`
  needed - checkpoints log at Info level) rather than guess further; this
  instrumentation should be removed once the real root cause is
  confirmed.
- Presentation-display fixes are proven at the layer this project's test
  architecture can reach (no `tauri::test` harness, a pre-existing,
  documented convention) plus real Xvfb GUI reproduction - real
  confirmation still requires the physical Windows re-test described in
  the final gate below.

## Deferred work

Real Windows re-test of this rebuilt artifact (the hard blocker for
PASS); removal of the temporary diagnostic instrumentation once the real
cause is confirmed or ruled out; the full aspirational UX redesign (still
deliberately out of scope, unchanged from prior phases).

## Final gate

Per the operator's own closing instruction for this phase: *"Do NOT mark
the phase PASS based on Linux/Xvfb alone. The physical Windows test is the
decisive gate. If the display is still blank after the first fix, STOP and
document the exact evidence instead of stacking speculative workarounds."*
That physical re-test has not occurred in this session.

```
AUTOMATED TESTS: PASS
LINUX/XVFB: PASS (real GUI reproduction, real pixel evidence, real diagnostic log)
REAL WINDOWS MACHINE: NOT VERIFIED
PRESENTATION DISPLAY RENDERING: PASS (Environment B only - NOT VERIFIED on real Windows hardware)
CLOSE/REOPEN CYCLE: PASS (Environment B only - NOT VERIFIED on real Windows hardware)
REAL BSB TEXT: PASS (Environment B only - NOT VERIFIED on real Windows hardware)
DIAGNOSTIC INSTRUMENTATION: PASS (verified functional in this environment after fixing its own log-level defect)

FULL WINDOWS PRESENTATION DISPLAY TEST: HOLD
```

This stops here, per the operator's explicit instruction. Phase 3.9 does
not begin automatically.
