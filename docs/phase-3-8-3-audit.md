# Phase 3.8.3 — Audit

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `61205b5` (Phase 3.8.2, "Windows service replay lifecycle and
  presentation display reliability")
- Working tree at start: clean

Read before any code changed: `docs/phase-3-8-2-audit.md`,
`docs/phase-3-8-2-service-replay-reliability.md`, `docs/presentation.md`
("Local display foundation" section in full).

## B. What changed since the last report

The operator's real Windows re-test of the Phase 3.8.2 build now shows the
service-lifecycle and intelligence fixes working correctly: Service Replay
starts, a real 269-segment transcript processes sequentially, Bible/Sermon
Intelligence produce real findings, and the "CIP Presentation Display"
window opens. The **only** remaining defect: that window renders
completely blank. This is explicitly a different symptom than Phase
3.8.2's "Not Responding" - the window is responsive and correctly
labeled, it simply shows nothing.

## C. Code re-trace (current state, `61205b5`)

Traced the complete path listed in the task, file by file:

1. `PresentationDisplay.tsx` - mounts, subscribes to
   `onPresentationStarted`/`onPresentationStopped`, and (since Phase
   3.8.2) also calls `commands.getPresentationDisplayState()` once on
   mount via the pure `resolveHydratedPayload` helper.
2. `PresentationDisplay.css` - inactive state is solid black (`#000`)
   with **no** placeholder text/logo by design; active state is gold
   heading + white body text on the same black background. This is
   relevant to first-diagnostic-question hypothesis D: if CSS failed to
   load, the page would show the browser/WebView2 default (white)
   background, not black - so "the window is black" alone does not
   distinguish "correctly inactive" from "CSS loaded but payload never
   arrived" from "React never mounted but the CSS file loaded anyway."
   This ambiguity is exactly why real pixel evidence (section E) was
   necessary rather than reasoning from the report alone.
3. `main.tsx` - routes to `PresentationDisplay` purely by reading
   `getCurrentWebviewWindow().label === "display"`. Unchanged, simple,
   already re-verified.
4. `presentation_display.rs` - `open_display_window`:
   `WebviewWindowBuilder::new(app, DISPLAY_WINDOW_LABEL,
   WebviewUrl::App("index.html".into())).title(...).inner_size(1280.0,
   720.0).resizable(true).visible(true).build()?`. Registers a
   `Destroyed` handler. No change since Phase 3.8.2.
5. `commands.rs`'s `display_presentation`: renders the slide via
   `render_content`, opens the display window, commits `Prepared ->
   Active`, records the timeline entry, then `emit(&app,
   AppEvent::PresentationStarted, PresentationDisplayPayload { item,
   slide })`. `get_presentation_display_state` (the Phase 3.8.2 addition)
   independently recomputes `active_slide` via the same `render_content`
   call whenever an `Active` item exists.
6. `capabilities/display.json` - `"permissions": ["core:default"]`,
   identical in content to `capabilities/default.json` (the `main`
   window's own grant, `"permissions": ["core:default"]`). Since the main
   window's identical grant already permits every custom `#[tauri::command]`
   this app registers (proven throughout every prior phase's countless
   successful command invocations from the main window), the display
   window's grant permits the same set - **capability restriction is
   ruled out** as a cause; nothing was changed here.
7. `presentation/renderer/src/lib.rs`'s `render_content` - pure,
   deterministic, unchanged since Phase 1.4, already exhaustively tested.

No code defect was found by re-reading alone. This matches the operator's
own framing: the previous rounds' fixes (service lifecycle, hydration,
synchronous close reconciliation) already addressed everything provable
by code inspection. What remained was **empirical**, not textual.

## D. Real end-to-end reproduction (this phase's central contribution)

Given no static-analysis lead, this audit built and exercised the actual
compiled Linux binary under Xvfb, driving the real GUI with `xdotool`
(installed this phase: `apt-get install xdotool imagemagick x11-apps`)
and capturing real pixels with `xwd`/`convert` - not `import` (which
proved unreliable for overlapping windows with no window manager present;
`xwd`'s direct `XGetImage` capture was used for every screenshot cited
below once this was discovered).

**Full real click-through sequence performed** (screenshots retained in
this session's evidence, described in `pilot-evidence/3.8.3/e2e/`):

1. Launched the real `cip-desktop` binary, `DISPLAY=:99`.
2. Clicked **Start Service**.
3. Expanded **Manual / test transcript entry**, typed `"Turn to Matthew
   chapter 6 verse 9."`, clicked **Submit**.
4. Real detection appeared: `MAT 6:9`, 97% confidence, via the real
   BSB-backed pipeline.
5. Clicked **Approve**, then **Prepare** - the Prepared card showed the
   real BSB text: *"So then, this is how you should pray: 'Our Father in
   heaven, hallowed be Your name."*
6. Clicked **Display**. A new window (`CIP Presentation Display`,
   1280x720, confirmed via `xdotool getwindowname`/`getwindowgeometry`)
   was created.
7. **Screenshot of the actual display window's real pixels**: heading
   `MAT 6:9` in gold, the full real BSB verse text in white, `BSB` footer
   in gray - **rendered correctly, on the first attempt, with normal
   human-paced clicking**.
8. Clicked **Stop** - main window's Presentation card correctly returned
   to "Nothing prepared yet."
9. Clicked **Close Display** - window count returned to 2 (display window
   gone).
10. Submitted a second reference (`"Romans chapter 8 verse 28."`),
    detected as `ROM 8:28` at 97%, approved.
11. Clicked **Prepare**, then **immediately** (`sleep 0.05` between click
    and screenshot - no meaningful human-perceptible delay) clicked
    **Display** and captured the new display window as fast as this
    environment could issue the capture command.
12. **Screenshot at ~50ms after window creation**: `ROM 8:28` fully
    rendered, real BSB text, correct layout - **the reopen-and-display-
    a-different-item cycle also worked correctly under adversarial rapid
    timing**, the exact scenario Phase 3.8.2's synchronous-close fix
    targeted.

**Conclusion from this evidence**: On Linux/WebKitGTK, under Xvfb, the
entire pipeline - detection, approve, prepare, display, close, reopen,
display-a-different-item, including at near-zero timing between Prepare
and Display - works correctly, every time, with the current `61205b5`
code and zero changes. This directly rules out, with real evidence rather
than reasoning:

- **A** (React never mounts) - disproven; it mounted and rendered.
- **B** (payload null) - disproven; real payload rendered.
- **C** (slide empty) - disproven; full heading/body/footer rendered.
- **D** (CSS not loaded) - disproven; the gold heading, white body, and
  layout all match `PresentationDisplay.css` exactly.
- **E** (Tauri event never reaches the display webview) - disproven, even
  under near-zero-delay adversarial timing.
- **F** (`getPresentationDisplayState` fails/returns no active slide) -
  not directly isolated from the event path in this test, but the
  combined pipeline succeeded consistently.
- **G** (wrong/invalid document loads) - disproven; the correct component
  rendered.
- **H** (renderer content undisplayable) - disproven; real, correct BSB
  text rendered both times.
- **I** (race between window creation and event/hydration) - not
  reproduced despite deliberately adversarial (near-zero-delay) timing on
  this platform.

## E. What this evidence does NOT prove

This is Environment A/B (automated + Xvfb) evidence, on Linux/WebKitGTK -
**not** Environment C (real Windows hardware, WebView2). It does not, and
cannot, prove the Windows-specific rendering path behaves identically.
Per spec's own instruction not to guess: the remaining live hypotheses,
in order of plausibility given everything ruled out above, are:

- **J (Windows WebView2-specific rendering issue)** - now the leading
  hypothesis by elimination. A well-documented class of real Tauri/WRY
  issues on Windows involves a newly created secondary `WebviewWindow`
  whose WebView2 controller does not paint its initial frame until the
  window receives a resize/redraw signal - the window exists, is
  responsive, loads its content, but shows nothing until moved or
  resized. This matches the operator's reported symptom precisely (a
  live, correctly-labeled, non-frozen, visually blank window) and is
  distinct from - and consistent with having been introduced or exposed
  independently of - the "Not Responding" symptom Phase 3.8.2 addressed.
- **Another concrete Windows-only cause** this environment cannot access
  or reproduce (GPU driver interaction, WebView2 runtime version/install
  state, DPI scaling, or something else) - explicitly marked
  `NOT VERIFIED` below rather than guessed further.

## F. Fix scoped from this evidence

Given hypothesis J is the best-supported remaining explanation and is a
well-known Windows/WebView2 behavior class (not specific to this
codebase), the smallest justified fix is a **Windows-only** explicit
resize nudge immediately after the display window is created - forcing a
`WM_SIZE`-equivalent event that causes WebView2 to paint its initial
frame. This is:

- Additive only (a few lines in `presentation_display.rs`, gated behind
  `#[cfg(target_os = "windows")]` so Linux/macOS behavior - already
  proven correct above - is completely unaffected).
- Not a new renderer, not a second presentation architecture, not a
  change to the event contract, the command signature, or the
  `Prepared -> Active -> Stopped` lifecycle.
- Consistent with the "do not stack speculative workarounds" instruction:
  this is the one, single, best-supported fix - not a pile of guesses.

**Explicitly, honestly**: this fix is justified by a well-documented
Windows/WebView2 behavior class matching the reported symptom, and by
having exhausted every other explanation this environment can test - it
is **not** confirmed by direct reproduction, because no physical Windows
machine is available here. Per the spec's own instruction, if the display
is still blank after this fix, the correct next step is to stop and
gather the temporary diagnostic evidence (section G) rather than stack
further speculative changes - which is exactly why that instrumentation
is added in the same commit, not deferred.

## G. Temporary diagnostics added

Per the spec's explicit requirement, structured, local-only logging is
added at each stage of the path, backend and frontend, so that if the
Windows-only resize fix is insufficient, the *next* session has real
Windows-side evidence instead of having to re-derive this same audit from
scratch. Frontend checkpoints report through one new, minimal, clearly
diagnostic-only Tauri command (`log_display_diagnostic`) so they land in
the same log file/terminal output the operator already captures - there
is no other way to observe a secondary webview's own console output in
this project (no devtools/logging plugin is installed, and none is added
here). See section H of the implementation plan below for the exact
checkpoint list and section I for why this is the smallest mechanism that
achieves real visibility.

## H. Implementation plan

1. `presentation_display.rs`: `open_display_window` gains a
   `#[cfg(target_os = "windows")]` block that calls `window.set_size(...)`
   with the same target size immediately after `build()` succeeds, to
   force WebView2's initial paint. No effect on any other platform.
2. New minimal diagnostic command `log_display_diagnostic(stage: String,
   detail: String)` in `commands.rs` - logs via the existing `log::`
   macros under a distinct target, nothing else. Registered in `lib.rs`.
   No new capability needed (identical reasoning to section C item 6).
3. `PresentationDisplay.tsx` and `presentationDisplayHydration.ts`: call
   this new command at each of the checkpoints the spec lists that are
   reachable from the frontend (mount, useEffect ran, hydration call
   made, hydration result, event received, payload applied, slide
   details).
4. `presentation_display.rs`/`commands.rs`: add `log::debug!` calls at
   the backend checkpoints (window created, `build_scripture_slide`/
   `render_content` result, `display_presentation` lifecycle ordering).
5. Rebuild Windows + Linux, re-run the exact Xvfb E2E click-through above
   against the rebuilt binary to prove no regression, rebuild the Windows
   installer, full regression suite, docs, evidence, commit.

Proceeding to implementation as scoped above.
