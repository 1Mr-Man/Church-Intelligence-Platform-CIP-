# Phase 3.8.4 — Audit

## A. Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `36b7a9b` (Phase 3.8.3, "diagnose and fix blank Presentation
  Display on Windows")
- Working tree at start: clean

## B. What changed since the last report

The operator physically tested the Phase 3.8.3 build (which included the
Windows-only resize-nudge fix and 14-checkpoint diagnostic logging) on the
real Windows laptop. Result:

1. Main CIP window works normally.
2. Service Replay is genuinely active, processes 269 segments, database
   connected.
3. The "CIP Presentation Display" window **appears** (a real native
   window is created - the resize-nudge fix, or something else, does get
   far enough to produce a visible window frame).
4. That window is **completely WHITE**, not black.
5. No Scripture is visible.
6. The main CIP UI remains functional throughout.

Point 4 is the critical new fact. `PresentationDisplay.css` sets
`html, body { background: #000; }` unconditionally - this rule applies
the instant the stylesheet loads, before React even mounts, regardless of
whether any presentation payload exists. A **black** blank window would
mean the frontend bundle loaded correctly and the page is simply in its
correct "nothing active" state. A **white** window is the WebView2
control's own default, pre-navigation background - it means the page
never got far enough to apply that stylesheet at all. This reclassifies
the Phase 3.8.3 investigation: that phase correctly ruled out "payload
never arrives" as the steady-state Linux behav0069or, but the real Windows
symptom is upstream of that - a load/render failure, not a payload
problem.

## C. Boundary-by-boundary audit (current state, `36b7a9b`)

Traced every boundary the operator's spec lists, against the current code:

**A. Rust creates the secondary WebviewWindow** - `open_display_window`
(`presentation_display.rs`) calls `WebviewWindowBuilder::new(...)`.

**B. `WebviewWindowBuilder::build()` returns successfully** - assumed to
happen (a window *does* appear on the operator's screen, so `build()` is
not simply erroring out and returning `Err`), but see finding 1 below:
there is direct, official evidence this exact call, from this exact
calling context, is documented to deadlock on Windows. A window frame can
still exist (created by the OS) while the WebView2 control inside it
never finishes initializing.

**C-E. The secondary WebView navigates to `index.html` / fires
`PageLoadEvent::Started` / fires `PageLoadEvent::Finished`** - **no
diagnostic coverage exists for any of these today.** `open_display_window`
never calls `.on_page_load(...)` on the builder. This is a real, direct
gap: nothing in the current code can distinguish "the page never started
loading" from "it started but never finished" from "it finished but React
never ran."

**F. The secondary WebView executes `main.tsx`** - only inferred
indirectly today, via `PresentationDisplay`'s own `useEffect` logging
(checkpoint 3/4) - which cannot fire at all if this boundary fails.

**G-H. `getCurrentWebviewWindow().label` resolves to `"display"` /
`Root()` selects `PresentationDisplay`** - `main.tsx`'s `Root()` does this
correctly by inspection, but **has zero diagnostic logging of its own**.
If the frontend bundle does load but something about the branch
resolution itself failed (e.g. `label` resolving unexpectedly), there is
currently no log line that would ever reveal it, since the only logging
lives inside `PresentationDisplay`'s effect - which requires this boundary
to have already succeeded.

**I-J. `PresentationDisplay` mounts / `PresentationDisplay.css` loads** -
covered by checkpoint 3 (mount) for I; **J has no direct diagnostic** -
CSS load success is currently inferred only from visual color (black vs.
white), never logged. Given this phase's central finding is exactly a
black/white distinction, this is a real, worthwhile gap to close, though
this audit does not add a redundant JS-side CSS-load probe: the existing
Rust-side `on_page_load` `Finished` event (once added) plus the plain
fact that this CSS rule is unconditional on `html, body` already gives an
authoritative signal - if the page's `Finished` event fires and React
mounts (checkpoint 3), the CSS `<link>` tag from the same `index.html`
document has already either been requested successfully or not,
independent of anything JS does.

**K-L. `getPresentationDisplayState` invoked / returns data** - covered
by existing checkpoints 5/6.

**M-N. `PresentationStarted` event received / payload applied** - covered
by existing checkpoints 7/9-12.

**O. DOM contains the rendered heading/body** - not directly logged, but
implied by checkpoint 9-12's payload content, which is logged before
`setPayload` triggers the render.

**P. Actual pixels painted in the Windows WebView** - cannot be logged
from inside the process; this is exactly why the physical Windows
screenshot is the only evidence that ever answers it.

## D. Critical code audit: the synchronous `WebviewWindowBuilder::build()` call

Per the operator's explicit instruction, the vendored Tauri 2.11.5 crate
source (not blog posts or assumptions) was inspected directly:

```
$ grep -n "Known issues" -A2 tauri-2.11.5/src/webview/webview_window.rs
  /// # Known issues
  ///
  /// On Windows, this function deadlocks when used in a synchronous command and event handlers, see [the Webview2 issue].
  /// You should use `async` commands and separate threads when creating windows.
```

This exact warning appears on **`WebviewWindowBuilder::new`** and on
**`WebviewWindowBuilder::build`** (`webview_window.rs` lines 56 and 115),
referencing `https://github.com/tauri-apps/wry/issues/583`. The official
Tauri documentation's own worked example for "Create a window in a
command" uses `async fn create_window(app: tauri::AppHandle)` -
explicitly not a plain synchronous command - specifically because of this
issue.

**Both Tauri commands in this codebase that call `open_display_window`
are currently plain synchronous `#[tauri::command] pub fn`, not
`async fn`:**

- `display_presentation` (`commands.rs`) - the command the manual
  detect→approve→prepare→**Display** click path actually exercises.
- `open_presentation_display` (`commands.rs`) - the separate "Open
  Display" button, exercised by both `LiveChurchBrain.tsx` and
  `ServiceReplay.tsx` (`await commands.openPresentationDisplay()`) before
  anything is prepared.

This is a documented, first-party-acknowledged Windows-specific deadlock
class that applies to exactly the calling pattern this codebase uses, on
exactly the platform the operator's evidence is from, and it is
architecturally silent on Linux/WebKitGTK (the WRY issue is WebView2/COM
apartment-threading-specific) - which is fully consistent with every
piece of evidence gathered so far: Linux/Xvfb has worked correctly in
every phase's reproduction, and Windows has never once shown correct
rendering. A window frame can still be created by the OS (explaining "the
window appears") while the WebView2 control inside it deadlocks before
ever completing its `CreateCoreWebView2Controller` callback and therefore
never navigates - which produces exactly WebView2's own default *white*
background, never the app's own CSS (which never gets a chance to run),
and would fully explain why the Phase 3.8.3 resize-nudge fix (which
targets a *different*, later-stage "doesn't paint until resized" WebView2
quirk) did not resolve the symptom: if the webview never finishes
initializing at all, no amount of resizing an already-dead control
changes anything.

## E. Why Phase 3.8.3's fix did not (and could not have) caught this

Phase 3.8.3's own Linux/Xvfb end-to-end reproduction was completely valid
evidence for what it tested - it proved the *application-level* pipeline
(detect → approve → prepare → render → event → hydrate → paint) is
correct once a webview has actually loaded. It could not, by construction,
ever reproduce this defect: WebKitGTK (Linux) does not have WRY issue
#583's Windows-specific COM/message-pump deadlock at all, so the exact
same synchronous `WebviewWindowBuilder::build()` call that deadlocks on
Windows completes normally on Linux every time. This is precisely why
every Xvfb run in Phase 3.8.3 showed correct rendering while the real
Windows hardware showed a blank window both before and after that phase's
fix - two different, non-overlapping failure classes were being
investigated by two different environments, and only the physical Windows
test could ever have surfaced this one.

## F. Fix scoped from evidence

The smallest change that resolves the documented deadlock, per Tauri's
own guidance, and per the operator's own explicit menu of acceptable
options ("make the command async, OR move `WebviewWindowBuilder::build()`
to the appropriate async/threaded context, while preserving the
externally visible Tauri command name and parameters"):

1. Convert `display_presentation` and `open_presentation_display` from
   `#[tauri::command] pub fn` to `#[tauri::command] pub async fn`. Their
   names, parameters, and return types are unchanged; `invokeCommand`
   already returns a `Promise` for every command regardless of whether
   the Rust side is sync or async, so the JS command contract in
   `commands.ts` requires **zero changes** - confirmed by inspection
   (`invokeCommand` wraps `@tauri-apps/api/core`'s `invoke`, which is
   already `Promise`-based for both sync and async Tauri commands).
2. Add `.on_page_load(...)` to the `WebviewWindowBuilder` in
   `open_display_window`, logging `PageLoadEvent::Started` and
   `PageLoadEvent::Finished` (with the URL) at `log::info!`, closing the
   boundary-C/D/E diagnostic gap identified above.
3. Add diagnostic logging in `main.tsx`'s `Root()` for the display
   branch specifically (boundary G/H: which branch was selected), and a
   global `window.onerror`/`unhandledrejection` handler scoped to that
   branch only (boundary "any frontend exception") - both routed through
   the same existing `logDisplayDiagnostic` command/`log::info!` path
   Phase 3.8.3 already established and already fixed to be visible at the
   app's Info log level.

No new rendering engine, no new event system, no replacement of WebView2,
no network/cloud dependency, no change to the Bible pipeline, the
intelligence engines, the presentation lifecycle, or `RenderedSlide` -
this is exclusively a change to *when/how* the existing window-creation
call executes, plus additive logging.

## G. Temporary visible diagnostic fallback - not added

The spec permits a temporary visible fallback in the display window
"ONLY if necessary to distinguish 'React never mounted' from 'React
mounted with no payload.'" This audit finds it is **not necessary**:
`PresentationDisplay.css`'s unconditional `black` background on
`html, body` already makes that exact distinction visually, with no code
change required - a genuinely blank presentation (React mounted, no
active item) is black; a page that never loaded/mounted is whatever
WebView2's own default is (white). Combined with the new `on_page_load`
and `main.tsx` boundary logging, the existing log file is already
sufficient to confirm which case occurred without adding any visible
content that could ever be mistaken for production presentation output.
If the physical Windows re-test still leaves this ambiguous, that would
be new evidence for a future phase, not something to speculatively add
now.

## H. Implementation plan

1. `presentation_display.rs`: add `.on_page_load(...)` to the builder
   chain in `open_display_window`, logging Started/Finished + URL at
   `log::info!` under the existing `cip::presentation` target.
2. `commands.rs`: change `display_presentation` and
   `open_presentation_display` to `async fn` (no signature/name/parameter
   changes).
3. `main.tsx`: log which branch `Root()` selected (display branch only),
   and register a display-scoped `window.onerror`/`unhandledrejection`
   handler that logs via the same diagnostic path.
4. `commands.test.ts`/other frontend tests: extend as needed for any new
   testable surface; no existing test should need to change since the
   external command contract is unchanged.
5. Full regression (Rust fmt/clippy/test incl. whisper feature,
   cross-compile check for the Windows target, frontend
   typecheck/lint/test/build), then a real Xvfb E2E re-verification pass
   proving zero regression to the already-correct Linux behavior and that
   the new diagnostics fire as designed.
6. Rebuild Windows + Linux artifacts, update the release manifest and
   pilot-evidence, write the phase report, commit, push.
7. Physical Windows re-test is the decisive gate, per the operator's own
   instruction - this session cannot perform it.
