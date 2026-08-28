# Phase 3.8.4 — Windows Secondary WebView Load & Presentation Rendering Reliability

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `36b7a9b` (Phase 3.8.3, "diagnose and fix blank Presentation
  Display on Windows")
- Working tree at start: clean

Full audit in `docs/phase-3-8-4-audit.md`, written before any code changed,
auditing every boundary the operator's spec listed (A-P) against the
current code.

## Why this phase exists

The operator physically tested the Phase 3.8.3 build (Windows-only
resize-nudge fix + 14-checkpoint diagnostic logging) on the real Windows
laptop. The intelligence pipeline continues working correctly (main
window normal, Service Replay active, 269 segments processed, database
connected). The Presentation Display window now **appears** (a real
native window is created), but is **completely white**, not black.
`PresentationDisplay.css` sets `html, body { background: #000; }`
unconditionally - applied the instant the stylesheet loads, regardless of
payload state. A white window therefore means the frontend document never
got far enough to apply that stylesheet - reclassifying this from "no
payload" to a load/render failure upstream of anything the Phase 3.8.3
diagnostics could observe (they all require the frontend to have already
loaded and run).

## Root cause

Per the operator's explicit instruction, the vendored Tauri 2.11.5 crate
source (not blog posts or assumptions) was inspected directly:

```
$ grep -n "Known issues" -A2 tauri-2.11.5/src/webview/webview_window.rs
  /// # Known issues
  ///
  /// On Windows, this function deadlocks when used in a synchronous command and event handlers, see [the Webview2 issue].
  /// You should use `async` commands and separate threads when creating windows.
```

This exact warning appears on both `WebviewWindowBuilder::new` and
`WebviewWindowBuilder::build`, citing
`https://github.com/tauri-apps/wry/issues/583`. The official Tauri
documentation's own worked example for "Create a window in a command"
uses `async fn`, not a plain synchronous command, specifically because of
this issue.

**Both Tauri commands in this codebase that call `open_display_window`
were plain synchronous `#[tauri::command] pub fn`, not `async fn`:**
`display_presentation` (the manual detect→approve→prepare→**Display**
click path) and `open_presentation_display` (the separate "Open Display"
button used by `LiveChurchBrain.tsx` and `ServiceReplay.tsx`).

This is a documented, first-party-acknowledged Windows-specific deadlock
class, architecturally silent on Linux/WebKitGTK (the WRY issue is
WebView2/COM-apartment-threading-specific). A window frame can still be
created by the OS while the WebView2 control inside it deadlocks before
ever completing its navigation callback - producing exactly WebView2's
own default *white* background, never this app's CSS, and fully
explaining why the Phase 3.8.3 resize-nudge fix (which targets a
different, later-stage "doesn't paint until resized" WebView2 quirk) did
not resolve the symptom: if the webview never finishes initializing at
all, resizing an already-dead control changes nothing.

**Not confirmed by direct reproduction** - no physical Windows machine or
WebView2 runtime was accessible in this container. This is the
best-supported explanation given first-party documentation matching this
codebase's exact calling pattern and platform, not a proven fact.

## Why Phase 3.8.3's fix did not (and could not have) caught this

Phase 3.8.3's Linux/Xvfb reproduction was valid evidence for the
application-level pipeline once a webview has actually loaded - it could
not, by construction, reproduce this defect, since WebKitGTK has no
equivalent to WRY issue #583's Windows-specific deadlock. The exact same
synchronous `WebviewWindowBuilder::build()` call that deadlocks on
Windows completes normally on Linux every time.

## Exact fix

Converted both commands from `#[tauri::command] pub fn` to
`#[tauri::command] pub async fn`. Names, parameters, and return types are
unchanged. `invokeCommand` in `commands.ts` already returns a `Promise`
for every Tauri command regardless of whether the Rust side is sync or
async, so the JS command contract required **zero changes** - confirmed
by an unmodified `commands.ts` and a fully passing, unmodified frontend
test suite (210/210).

No new rendering engine, no new event system, no replacement of WebView2,
no network/cloud dependency, and no change to the Bible pipeline,
intelligence engines, presentation lifecycle, or `RenderedSlide`.

## Additional diagnostic coverage

- `presentation_display.rs`: added `.on_page_load(...)` to the
  `WebviewWindowBuilder`, logging `PageLoadEvent::Started`/`Finished`
  with the URL at `log::info!` - closing the previously-uncovered
  boundaries C/D/E (does the display webview's document navigation ever
  start/finish).
- `main.tsx`: module-scope (never during a component's render, to avoid
  a real lint violation caught during this phase - see below) logging of
  which branch `Root()` selected (display window only, to avoid noise on
  every ordinary main-window launch), and a display-scoped
  `window.onerror`/`unhandledrejection` handler for any otherwise-invisible
  frontend exception - closing boundaries G/H and "any frontend
  exception."
- `presentationDiagnostics.ts` (new): the `logCheckpoint` helper
  extracted out of `PresentationDisplay.tsx` so `main.tsx` and
  `PresentationDisplay.tsx` share one implementation rather than
  duplicating the pattern.

A temporary *visible* fallback in the display window (permitted by the
spec "ONLY if necessary") was considered and **not added**: the existing
black/white distinction from `PresentationDisplay.css` already
distinguishes "mounted with no payload" (black) from "never
loaded/mounted" (white/whatever WebView2's default is), and the new log
coverage now gives an authoritative signal for the same distinction
without risking any content that could be mistaken for production
presentation output.

### A real lint defect found and fixed during implementation

The first draft assigned `window.onerror` directly inside `Root()`'s
render body. `oxlint`'s `react(immutability)` rule correctly flagged this
as mutating a global during render. Fixed by moving the branch check and
diagnostic/handler registration to module scope, computed once before
React ever renders - `Root()` itself became a pure function reading a
precomputed boolean. Re-verified: the warning is gone, and no new
warnings were introduced.

## Real E2E reproduction (Linux/Xvfb re-verification)

Real Xvfb + xdotool + xwd reproduction of the rebuilt (async-command)
binary repeated the full manual-pipeline acceptance sequence:

1. Start Service → expand Manual/test transcript entry → submit "Turn to
   Matthew chapter 6 verse 9." → MAT 6:9 detected at 97% → Approve →
   Prepare → **Display**.
2. Real pixel screenshot of the display window: correct MAT 6:9 text,
   gold heading, white body, black background - identical to Phase
   3.8.3's baseline, proving zero Linux regression.
3. Stop → display card returns to "Nothing prepared yet" → **Close
   Display** → `DISPLAY CLOSED`.
4. Submitted "Turn to Romans chapter 8 verse 28." → ROM 8:28 detected at
   97% → Approve → Prepare → **Display** (reopening the window fresh).
5. Real pixel screenshot of the reopened window: correct ROM 8:28 text,
   proving the full close→reopen→display-a-different-item cycle still
   works with zero stale state.

Critically, the app's log file confirms every new checkpoint fires
correctly, in order:

```
[diagnostic] display window created (checkpoint 1)
[diagnostic] display window: page-load-started url=tauri://localhost
[diagnostic] display window: page-load-finished url=tauri://localhost
[diagnostic] display window: root-branch-selected - display label detected, rendering PresentationDisplay
[diagnostic] display window: mounted - PresentationDisplay component mounted (checkpoint 3)
[diagnostic] display window: effect-ran - useEffect body executing (checkpoint 4)
[diagnostic] display window: hydration-call - calling getPresentationDisplayState (checkpoint 5)
[diagnostic] display window: hydration-result - windowOpen=true activeItem=true activeSlide=true (checkpoint 6)
[diagnostic] display window: payload-applied - source=hydration heading=MAT 6:9 bodyLines=3 footer=BSB (checkpoints 9-12)
```

This is a complete, unbroken diagnostic trail from window creation
through pixels-ready state on Linux - the exact trail that will pinpoint
the failing boundary on the next real Windows test if the async-command
fix is not, by itself, sufficient.

All screenshots and the full diagnostic log are saved under
`pilot-evidence/3.8.4/e2e/`.

## Windows result

**NOT VERIFIED.** No physical Windows machine was accessible to Claude
Code in this container. The Windows-only compile path (the
`.on_page_load` closure and the two now-`async fn` commands) was
independently verified via `cargo check --target x86_64-pc-windows-gnu`,
and the embedded application binary was confirmed genuinely x64 via
`file(1)`. None of this substitutes for a real Windows/WebView2 launch.

## Full regression result

Rust workspace (default features): **786 passed, 0 failed** (unchanged
from the Phase 3.8.3 baseline - this phase changed *how* two existing
commands execute, not new unit-testable domain logic). `cargo fmt
--check`, `clippy --all-targets -- -D warnings`: clean. Whisper feature: 7
passed, 0 failed. Windows-target cross-compile check: clean. Frontend:
**210 passed, 0 failed** (unchanged - the JS command contract did not
change). `typecheck`, `build`: clean. `lint`: 0 errors, 4 pre-existing
warnings (unrelated files, unchanged from the Phase 3.8.3 baseline); one
new warning was introduced and fixed during implementation (see above).

## Architectural safety diff

```
FILES MODIFIED: apps/desktop/src-tauri/src/presentation_display.rs,
  apps/desktop/src-tauri/src/commands.rs,
  apps/desktop/src/main.tsx,
  apps/desktop/src/components/PresentationDisplay.tsx
FILES CREATED: apps/desktop/src/components/presentationDiagnostics.ts,
  docs/phase-3-8-4-audit.md,
  docs/phase-3-8-4-presentation-display-rendering.md,
  pilot-evidence/3.8.4/*
FILES DELETED: NONE
DATABASE MIGRATIONS ADDED: NONE
BIBLE DATABASE CHANGED: NO
INTELLIGENCE ENGINES CHANGED: NO
SERVICE REPLAY CONTRACT CHANGED: NO
TRANSCRIPT CONTRACT CHANGED: NO
TAURI COMMANDS RENAMED/REMOVED: NONE
TAURI COMMANDS ADDED: NONE
EXISTING COMMAND SIGNATURES CHANGED: NONE (display_presentation and
  open_presentation_display's names/parameters/return types are
  identical; only their execution model changed from synchronous to
  async fn, which invoke() already handles identically on the JS side -
  confirmed via an unmodified commands.ts and 210/210 passing frontend
  tests)
EVENT CONTRACTS CHANGED: NONE (confirmed via empty
  `git diff 36b7a9b -- apps/desktop/src-tauri/src/events.rs apps/desktop/src/events/`)
PRESENTATION LIFECYCLE: Prepared -> Active -> Stopped unchanged (confirmed
  via empty `git diff 36b7a9b --stat -- presentation/renderer/ core/presentation/`)
PERSISTENCE: unchanged
OFFLINE ARCHITECTURE: preserved (confirmed via `cargo tree` - no HTTP
  client crate in the dependency graph)
NETWORK CAPABILITIES: NONE ADDED (confirmed via empty
  `git diff 36b7a9b -- apps/desktop/src-tauri/capabilities/ apps/desktop/src-tauri/tauri.conf.json`)
NEW PRESENTATION ENGINE: NONE - architecture remains exactly Backend:
  RenderedSlide -> PresentationStarted payload -> PresentationDisplay ->
  HTML/CSS -> Windows display, with one presentation renderer
NEW RENDERER: NONE
EXTERNAL BROWSER: NOT INTRODUCED - WebView2 remains the only display
  mechanism; this phase changes when/how the existing window is created,
  never what renders inside it
```

## Windows artifact

Rebuilt this phase - see `pilot-evidence/3.8.4/windows/` for the checksum
and `release/windows/release-manifest.json` for full provenance.

## Environment A / B / C

- **Environment A (automated)**: full pass, detailed above.
- **Environment B (Xvfb)**: full pass - real GUI automation, real
  screenshots, real diagnostic log output proving the new checkpoints
  fire correctly; see `pilot-evidence/3.8.4/xvfb/` and
  `pilot-evidence/3.8.4/e2e/`.
- **Environment C (real Windows hardware)**: **NOT VERIFIED** against
  this rebuilt artifact. No physical Windows machine was accessible to
  Claude Code in this container. The operator's own Phase 3.8.3 Windows
  testing (which surfaced the white-display symptom) was against the
  *prior* build, not this fixed one - per this phase's own explicit
  instruction, that is not converted into PASS evidence for this
  rebuild.

## Known limitations

- The async-command fix is strongly supported by first-party Tauri/WRY
  documentation citing the exact calling pattern this codebase used, not
  a confirmed root cause - it could not be directly reproduced or
  confirmed without a real Windows/WebView2 environment.
- The diagnostic logging (Phase 3.8.3's 14 checkpoints plus this phase's
  page-load and branch-selection checkpoints) remains temporary,
  development-only instrumentation. If the real Windows re-test still
  shows a white or blank display, the next step is to read the log file
  directly (checkpoints log at Info level, no `RUST_LOG` needed) to
  determine the exact first failed boundary, rather than guess further.
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

Per the operator's own instruction: *"Do not mark Windows PASS unless
actual Scripture pixels are visible in the secondary display window."*
That physical re-test has not occurred in this session.

```
WINDOW CREATION: NOT VERIFIED
PAGE LOAD: NOT VERIFIED
REACT MOUNT: NOT VERIFIED
CSS LOAD: NOT VERIFIED
HYDRATION: NOT VERIFIED
PRESENTATION EVENT: NOT VERIFIED
MAT 6:9 DISPLAY: NOT VERIFIED
ROM 8:28 DISPLAY: NOT VERIFIED
CLOSE/REOPEN: NOT VERIFIED

AUTOMATED TESTS: PASS
LINUX/XVFB (all boundaries, including the newly-diagnosed ones): PASS

FULL WINDOWS PRESENTATION DISPLAY TEST: HOLD
```

This stops here, per the operator's explicit instruction. Phase 3.9 does
not begin automatically.
