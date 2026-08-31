# Phase 3.10.1 — Presentation Window / Multi-Monitor Architecture Audit

## A. Scope of this document

**No code was changed to produce this document.** Per the user's explicit
instruction after Phase 3.10 (multi-screen presentation output, commit
`27a637f`), the next step is a careful, audit-only trace of the entire
presentation window pipeline before any further code is written — because
this pipeline has already gone through real, hardware-evidenced Windows
debugging three times (Phases 3.8.3, 3.8.4, 3.8.5), and any change made
without first tracing the current implementation risks reintroducing a
defect already fixed once.

This document traces the pipeline the user asked for end to end, audits
every specific item the user named, states the target architecture gap
honestly, and identifies the safe extension points a future
implementation phase would use — without writing that implementation.
Sections **B–H** are the audit. Section **I** restates the user's own
proposed Phase 3.10.2–3.10.5 structure as *proposed, not-yet-started*
future work, so the sequencing is on record without implying any of it
has begun.

- Branch: `claude/cip-foundation-init-i85g87`
- Baseline commit: `27a637f` (Phase 3.10)
- Tauri version actually in use: `2.11.5` (`Cargo.lock`; `Cargo.toml`
  pins `2.11.3` as a minimum) — verified directly, not assumed, since
  the monitor API's exact shape depends on it (see section D).

## B. Full pipeline trace

The user's requested trace, walked against the real current code (file
and line references are to the Phase 3.10 baseline):

```
Operator clicks a screen's "Open" or "Display" button
        ↓
PresentationCard.tsx (onOpenScreen / onDisplay prop)
        ↓
LiveChurchBrain.tsx / ServiceReplay.tsx handler → commands.openPresentationDisplay(screen)
   or → commands.displayPresentation(itemId)          [lib/commands.ts]
        ↓
Tauri invoke("open_presentation_display", { screen })
   or invoke("display_presentation", { itemId })
        ↓
commands.rs: open_presentation_display() / display_presentation()   [async fn]
        ↓
   display_presentation only: presentation::prepare_to_activate()   [presentation.rs — Tauri-agnostic]
   validates Prepared status, renders the slide, rejects if another item is already Active
        ↓
presentation_display::open_display_window(app, screen)              [presentation_display.rs]
        ↓
   app.get_webview_window(label) — reuse if it already exists (show + focus), else:
        ↓
   WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html"))
     .title(...) .inner_size(1280.0, 720.0) .resizable(true) .visible(true)
     .on_page_load(...)   [Phase 3.8.4 diagnostic]
     .build()
        ↓                                              ← NO MONITOR ENUMERATION HAPPENS HERE
        ↓                                              ← NO .position(x, y) CALL EXISTS ANYWHERE
   #[cfg(windows)] window.set_size(...)  [Phase 3.8.3 paint-nudge workaround, unchanged]
        ↓
   window.on_window_event(Destroyed → reconcile if last open screen)
        ↓
display_presentation only: presentation::commit_activation() → record_timeline() → emit(PresentationStarted)
        ↓
Tauri's global event bus (tauri::Emitter::emit, no target filter — reaches every open webview)
        ↓
PresentationDisplay.tsx (in the new/reused window, distinguished from `main` purely by
  reading its own window label in main.tsx) — hydrates once via getPresentationDisplayState(),
  then reacts live to PRESENTATION_STARTED/PRESENTATION_STOPPED
        ↓
Operator clicks "Stop" / "Close" (per screen)
        ↓
commands.clearPresentationDisplay() [blanks, keeps window open]
   or commands.closePresentationDisplay(screen) [closes that screen's window]
        ↓
commands.rs: clear_active_presentation() / close_presentation_display()
        ↓
   close_presentation_display: reconciles (stops the Active item) only if `screen` was the
   *last* open screen (any_display_window_open check), THEN presentation_display::close_display_window()
        ↓
window.close() → OS destroys the window → Destroyed handler fires (idempotent no-op here,
   since close_presentation_display already reconciled synchronously) → PresentationStopped
   already emitted → PresentationDisplay.tsx (if another screen is still open) clears its slide
```

This confirms the trace the user asked for is real and traceable end to
end. The one link that **does not exist today** is explicit in the
diagram above: monitor enumeration and window positioning never happen
between "window creation" and "window becomes visible." A newly created
display window appears wherever the OS/WebView2 defaults to placing an
unpositioned window (observed in this project's own prior Xvfb testing
and consistent with Windows' own default new-window placement: cascaded
near the last-focused window, i.e. typically on the *same* monitor as the
operator's own `main` window) — never targeted at a second monitor
automatically.

## C. Specific items audited

### `display_presentation` (commands.rs)

`async fn`, unchanged contract since Phase 3.10 (`item_id: String`).
Always opens `DisplayScreen::Stage` specifically
(`presentation_display::open_display_window(&app, DisplayScreen::Stage)`)
before committing `Prepared -> Active`. Never touches Confidence/Lobby.
No monitor awareness.

### `open_presentation_display` (commands.rs)

`async fn` (required — see section E). Takes a `screen: String`,
resolved via `parse_display_screen` into a `DisplayScreen`. Delegates
directly to `presentation_display::open_display_window`. No monitor
awareness; no way for the caller to request a specific monitor.

### Presentation window labels and URLs

Three fixed labels, one per `DisplayScreen` variant (`presentation_display.rs`):
`"display"` (Stage), `"display-confidence"`, `"display-lobby"`. All three
load the identical `WebviewUrl::App("index.html".into())` — the same
single-page-app entry point the `main` window loads. There is no second
Vite build; `main.tsx` branches purely on `getCurrentWebviewWindow().label`
at module scope (see section G). No window is ever declared statically in
`tauri.conf.json`'s `windows` array (only `main` is, with
`width: 800, height: 600, resizable: true, fullscreen: false`) — all
three display windows are created exclusively at runtime, on explicit
operator action, matching spec section 18's "never opened automatically."

### `WebviewWindowBuilder`

Confirmed real API surface directly in the vendored `tauri-2.11.5`
source (`~/.cargo/registry/.../tauri-2.11.5/src/webview/webview_window.rs`),
not assumed from memory:

- `.position(x: f64, y: f64) -> Self` exists (line 799) — **never called**
  anywhere in this codebase today.
- `.inner_size(w, h)`, `.resizable(bool)`, `.visible(bool)`,
  `.fullscreen(bool)`, `.decorations(bool)`, `.always_on_top(bool)`,
  `.skip_taskbar(bool)` all exist and are real extension points; only
  `.inner_size`/`.resizable`/`.visible`/`.title`/`.on_page_load` are used
  today.
- The synchronous-vs-`async fn` requirement (section E below) applies to
  `.build()` regardless of what other builder methods are chained before
  it — adding `.position(...)` would not change that constraint.

### Monitor detection APIs

Confirmed real, already-available, **zero new dependency** API surface
in the same vendored source:

- `AppHandle::available_monitors(&self) -> tauri::Result<Vec<Monitor>>`
  and `AppHandle::primary_monitor(&self) -> tauri::Result<Option<Monitor>>`
  (`src/app.rs`, `shared_app_impl!(AppHandle<R>)` macro instantiation at
  line 1231 — confirmed these are inherent methods on the exact
  `AppHandle` type every command in `commands.rs` already receives, not
  on some other type that would need extra plumbing).
- `Monitor` (`src/window/mod.rs`) exposes `.name() -> Option<&String>`,
  `.size() -> &PhysicalSize<u32>`, `.position() -> &PhysicalPosition<i32>`,
  `.work_area() -> &PhysicalRect<i32, u32>`, `.scale_factor() -> f64`.
- **This API is already used in this exact codebase** — see the next
  finding, which is the single most important discovery of this audit.

### ⚠️ Key finding: monitor enumeration already exists, but only as a read-only diagnostic, completely disconnected from window placement

`commands.rs`'s `get_pilot_diagnostics` command (Phase 3.2–3.4 era, lines
~351–372 and ~645–659) already calls `app.primary_monitor()` and
`app.available_monitors()` and maps every result into a
`DisplayDiagnostic { name, width_px, height_px, position_x, position_y,
scale_factor, is_primary }` — structurally almost exactly the "Display
Registry" the user is now asking for. This has been in the codebase since
Phase 3.2/3.4 and is fully exercised: `PilotDiagnostics.displays:
Vec<DisplayDiagnostic>` is returned to the frontend, mirrored in
`apps/desktop/src/config/appConfig.ts` (`displays: DisplayDiagnostic[]`),
and rendered in `PilotDiagnosticsPanel.tsx` as `"{displays.length}
detected"`, plus the literal text `"(no second display/projector detected
- single-display/manual-preview mode)"` when fewer than two displays are
detected.

**But this is read-only informational text only.** Nothing in
`presentation_display.rs` or `commands.rs`'s presentation-display section
reads `displays`, checks its length, or uses any monitor's `position()`/
`size()` for anything. Opening a display window behaves identically
whether zero, one, or three monitors are attached — the operator always
gets an unpositioned 1280×720 window they must find and drag themselves.
The "single-display/manual-preview mode" phrase, confirmed by reading
`docs/phase-3-2-hardware-pilot.md` directly (lines 577–582, 598–599), was
always a **declared support-scope statement** ("CIP is proven and
supported when the operator manually positions the display window
themselves, on any monitor count") — never an automatic code branch that
detects monitor count and changes behavior. No regression risk exists
today for the simple reason that there is no monitor-count-dependent
logic to regress.

This also clarifies a real terminology risk for the next phase: this
project already has an unrelated **"Preview"** feature
(`previewPresentation`/`preview_scripture` commands, Phase 1.4) that
renders a slide inline in the operator's own `main` window without
touching any display window at all. That is a different concept from the
"manual-preview mode" phrase in the Phase 3.2 docs (which describes the
*display window itself*, manually positioned). Any future design
document should use distinct names for these two unrelated things.

### Primary vs. secondary monitor selection

No code anywhere selects a monitor for a presentation window today —
confirmed by the trace in section B: `open_display_window` never queries
`available_monitors`/`primary_monitor` at all. The only place those
Tauri APIs are called is the diagnostics command described above, which
never influences window placement.

### Current single-display / manual-preview fallback

As established above: there is no automatic *fallback behavior* —
there is only the operator's own manual action (drag the window to
whichever screen they want), which works identically regardless of how
many monitors are attached, and is CIP's actual declared-supported path
today per the Phase 3.2 release documentation. A future Display Registry
phase should preserve this exact behavior as the true baseline (not
"regress to" — it already **is** the baseline) when zero or one monitor
is detected, and only add automatic-positioning behavior as an
*additive* enhancement when 2+ monitors are detected, exactly as the
user's own "Preserve existing behavior" diagram specifies.

### `PresentationDisplay.tsx`

Traced in full in section B and quoted from the current file directly
above (this document's author read the live file, not a summary of it).
Purely a passive renderer: hydrates once via
`getPresentationDisplayState()`, then reacts to the two broadcast events.
Accepts a `role: PresentationScreen` prop (`"stage" | "confidence" |
"lobby"`) that changes only which CSS class is applied and whether the
Confidence-only metadata panel renders — it has no awareness of which
physical monitor it is displayed on, nor does it need any (window
placement is exclusively the Rust side's responsibility; the React
component only ever renders content, never positions its own window).

### Presentation events

`PresentationStarted`/`PresentationStopped` (`events.rs`) are emitted via
`events::emit`, a thin wrapper over `tauri::Emitter::emit(event_name,
payload)` with **no target/window filter** — confirmed directly in
`events.rs`'s own doc comment ("Emit an `AppEvent` with a serializable
payload to every listening webview") and in the vendored `Emitter` trait.
This is why Phase 3.10's three-screen generalization required zero event
changes: every open display window, regardless of role, already receives
the same broadcast. This remains true and is a safe foundation for a
future Presentation Router (`per-role output policy`) — the event
payload itself would need to grow (e.g. to carry a per-role directive),
but the broadcast mechanism itself needs no change.

### Window lifecycle: open/reuse

`open_display_window` checks `app.get_webview_window(label)` first —
if present, `.show()` + `.set_focus()`, never creates a duplicate window
for the same label (spec section 17). This reuse check would need to
become monitor-aware in a future phase (e.g., "the window already exists
but the target monitor changed since it was opened" is not handled
today — the existing window is simply refocused wherever it already is).

### Close/reopen behavior

Traced in section B. `close_presentation_display` (Phase 3.10) now only
reconciles the `Active` item to `Stopped` when the closing screen was the
*last* one open (`any_display_window_open` check) — the multi-screen-safe
version of the Phase 3.8.2 synchronous-reconciliation fix
(`docs/phase-3-8-2-...` — the original single-window fix this
generalizes). A real, already-passing test
(`presentation.rs::three_display_stop_close_reopen_cycles_never_leave_a_stale_active_item`)
proves the underlying `Prepared -> Active -> Stopped` state machine
survives repeated open/close/reopen cycles at the persistence layer —
but this test, by the module's own documented convention (no
`tauri::test` harness in this project), does not and cannot exercise the
real `WebviewWindowBuilder`/OS window layer. Window-level reopen
behavior (does a *literal* Alt+F4 close, followed by a real reopen,
still show content correctly) has only ever been verified via real
Windows hardware testing in Phase 3.8.5, not by any automated test.

### Windows-specific Tauri/WebView2 behavior — established constraints a future phase MUST respect

Both real, hardware-evidenced, already-fixed Windows defects, re-verified
present in the current code (not just cited from memory):

1. **Synchronous `WebviewWindowBuilder::build()` deadlocks WebView2 init
   on Windows** (`docs/phase-3-8-4-audit.md` section D, referencing the
   vendored Tauri crate's own doc comments citing
   `github.com/tauri-apps/wry/issues/583`). This is why
   `open_presentation_display` and `display_presentation` are `async fn`,
   not synchronous `#[tauri::command] fn`. **Any future command that
   calls `open_display_window` (directly or transitively) must remain
   `async fn`.** Confirmed still true in the current code — both
   commands are still `async fn` (commands.rs, lines ~2685 and ~2825).
2. **A newly created secondary WebView2 window sometimes does not paint
   its initial frame until it receives a resize/redraw signal**
   (`docs/phase-3-8-3-audit.md` section D). The `#[cfg(target_os =
   "windows")] window.set_size(...)` call immediately after `.build()` is
   the still-present workaround (`presentation_display.rs`, line ~221).
   **Any future change to window creation must not remove or reorder
   this call relative to `.build()`.**
3. Both fixes are annotated as targeting **Windows specifically**
   (`#[cfg(target_os = "windows")]` for the resize nudge; the async
   requirement applies to all platforms for build-call-site consistency,
   but the underlying deadlock is Windows-only per the cited WRY issue).
   A future monitor-positioning change is the same *kind* of
   Windows-vs-other-platform risk (window placement APIs are notoriously
   inconsistent across Windows/macOS/X11/Wayland) and should budget for
   the same audit-then-real-hardware-verify discipline this project has
   already established, not assume `.position(x, y)` behaves identically
   everywhere without testing.

### Existing diagnostics and logging

Two families exist today, both explicitly labeled **temporary** in their
own doc comments and neither yet removed:

- **Phase 3.8.3 diagnostics**: `.on_page_load()` checkpoint logging
  (page-load-started/finished) and a numbered checkpoint sequence
  (comments reference "checkpoint 1" through "checkpoints 9-12") threaded
  through `presentation_display.rs`, `commands.rs`, and
  `PresentationDisplay.tsx`/`presentationDisplayHydration.ts`.
- **Phase 3.8.4 diagnostics**: `main.tsx`'s module-scope
  `window.onerror`/`unhandledrejection` handlers (display window only)
  and the `log_display_diagnostic` Tauri command
  (`commands.rs`) that routes the display window's own frontend
  checkpoints into the Rust log stream, since this app has no
  devtools/logging plugin and a secondary webview's `console.log` is
  otherwise invisible.

Both remain present and functional after Phase 3.10 (re-verified: the
checkpoint log lines are still emitted for all three screens, not just
Stage — confirmed by direct reading of the current
`PresentationDisplay.tsx`). A future phase adding monitor positioning
should **reuse this exact logging infrastructure** (add new checkpoints
for "monitor enumerated," "target monitor selected," "position applied")
rather than inventing a second diagnostic system, and should consider
whether these Phase 3.8.3/3.8.4 diagnostics are now stable enough to
promote from "temporary" to permanent, or whether they should finally be
removed — a real decision this audit surfaces but does not make.

## D. Tauri/monitor API version note

`Cargo.toml` pins `tauri = { version = "2.11.3", ... }`; `Cargo.lock`
resolved `2.11.5`. All API signatures cited in section C were read
directly from the vendored `2.11.5` source
(`~/.cargo/registry/src/.../tauri-2.11.5/`), not from documentation or
memory, so they are exact for the version this project actually builds
against today.

## E. Target architecture gap analysis

Mapping the user's requested target architecture against what exists:

| User's target concept | Current state |
|---|---|
| `Display` struct (id/monitor_name/position/resolution/role/enabled) | **Does not exist.** `DisplayScreen` (Phase 3.10) is a *role* enum with zero monitor awareness — no id, no position, no resolution, no enabled flag, no link to a physical `Monitor`. `DisplayDiagnostic` (Phase 3.2–3.4) has the monitor-descriptor fields (position/resolution/name) but no role and is diagnostics-only, never consulted by window creation. |
| Presentation Router (per-item, per-role output policy: SHOW/NEXT-PREVIEW/etc.) | **Does not exist.** Today every open screen receives the exact same broadcast event and renders the exact same content (Stage/Lobby identically; Confidence adds metadata client-side, but still shows the *same* active item, never a different one like "next up"). There is no concept of a queued/ordered sequence for a "next item" preview to draw from at all (noted as a known limitation in `docs/phase-3-10-multi-screen.md`). |
| Auto-assign roles to detected displays | **Does not exist.** All three screens are opened only by explicit, individual operator click — there is no "N displays detected → auto-open/assign" logic anywhere. |
| Monitor disconnect/reconnect handling | **Not explicitly handled.** No code listens for a monitor-configuration-changed event (Tauri does not appear to expose one directly in the current API surface reviewed — would need further research in a dedicated phase). The only disconnect-adjacent behavior that exists is generic: if a window's OS-level handle becomes invalid (e.g. its monitor disappears and the OS closes/minimizes it), the existing `Destroyed` handler would fire the same reconciliation it already does for a manual close — this is *incidental* coverage, not designed-for, and has never been tested against an actual monitor disconnect. |
| Preserve single-display behavior exactly | **Preserved by omission today** (there is no monitor-count branch to regress), and this is the correct baseline to explicitly preserve going forward per the user's own diagram. |

## F. Safe extension points (for a future implementation phase — not built here)

1. `AppHandle::available_monitors()`/`primary_monitor()` are callable
   directly inside `presentation_display::open_display_window` (which
   already receives `&AppHandle`) with zero new dependency and zero new
   Cargo feature.
2. `WebviewWindowBuilder::position(x, y)` is real and chainable in the
   exact same builder chain `open_display_window` already constructs,
   before `.build()`.
3. Neither of the above requires any change to `capabilities/*.json` —
   both are Rust-side calls made from inside a Tauri command, not
   JS-side `@tauri-apps/api` calls, and Tauri's capability/permission
   system governs the latter, not the former (confirmed by capability
   file structure: `capabilities/*.json` list `permissions` like
   `core:default`, which gate frontend `invoke`/`listen` calls, not
   backend `AppHandle` method calls).
4. The existing `DisplayDiagnostic` computation in `get_pilot_diagnostics`
   is the ready-made template for a future `Display` registry type's
   monitor-descriptor half — it would need a `role`/`enabled` field
   added and to be computed at window-open time (or cached and
   invalidated) rather than only on-demand for diagnostics.
5. The existing Phase 3.10 `DisplayScreen` enum's `window_label()`/
   `operator_label()` pattern is the ready-made template for the *role*
   half of a future `Display` registry — today the mapping is
   role→window only; a future phase would extend it to
   role→(window, monitor).
6. `events::emit`'s already-global broadcast means a future
   per-role-output-policy design does not need per-window targeted
   emission — it needs the *payload* to carry enough information for
   each already-listening window to decide locally whether/how to
   render (e.g. "this is the Confidence role's Next-Up payload" vs.
   "this is the Stage role's Active payload"), or a second, role-scoped
   event.

## G. Frontend window-label branching (confirmed, for completeness)

`main.tsx` resolves which React tree to render purely by reading
`getCurrentWebviewWindow().label` once at module scope, mapping it
through a fixed `DISPLAY_WINDOW_ROLES` record. This is unaffected by
monitor topology today (a window's *label* is independent of which
monitor it happens to be sitting on) and would remain unaffected by a
future monitor-aware phase — monitor placement is a Rust-side window
*creation-time* decision; the frontend's job (rendering the right
content for the right role) does not change based on where the window
physically ended up.

## H. Final gate

| Item | Status |
|---|---|
| Full pipeline traced end to end against real, current code | DONE |
| Every specifically-named item audited (`display_presentation`, `open_presentation_display`, window labels/URLs, `WebviewWindowBuilder`, monitor detection APIs, primary/secondary selection, single-display fallback, `PresentationDisplay.tsx`, events, lifecycle, close/reopen, Windows/WebView2 constraints, diagnostics/logging) | DONE |
| Monitor-enumeration API confirmed real, version-exact, already-used-elsewhere-in-this-codebase (not assumed) | DONE |
| Target architecture gap stated honestly (Display Registry, Router, auto-role-assignment, disconnect/reconnect all confirmed NOT to exist yet) | DONE |
| Safe extension points identified without implementing them | DONE |
| **Zero code changes made** (verify: `git status`/`git diff` empty for all tracked source) | DONE |

**Phase 3.10.1: audit complete. No functional changes were made in this
phase**, per the user's explicit instruction. Sections I below record the
user's own proposed follow-on phase structure for reference; none of
Phase 3.10.2 through 3.10.5 has been started.

## I. Proposed follow-on phases (NOT STARTED — recorded for sequencing only)

As the user specified:

- **Phase 3.10.2 — Display Registry**: a real `Display` type
  (id/monitor_name/position/resolution/role/enabled), built on the
  `DisplayDiagnostic` monitor-descriptor pattern (section F.4) and the
  `DisplayScreen` role pattern (section F.5), unifying the two for the
  first time.
- **Phase 3.10.3 — Presentation Routing**: per-item, per-role output
  policy (SHOW/NEXT-PREVIEW/etc.), extending the event payload or
  introducing a role-scoped event (section F.6).
- **Phase 3.10.4 — Multi-window Lifecycle**: open/reuse/update/disconnect/
  reconnect/close/reopen, explicitly required to preserve the async-command
  and resize-nudge constraints from section C's Windows-specific findings
  without reintroducing either defect.
- **Phase 3.10.5 — Windows Hardware Verification**: real hardware testing
  (laptop + HDMI monitor/TV, ideally a third screen), covering monitor
  disconnect/reconnect survival and correct window updates on stop —
  this project's established decisive gate (Environment C) for any
  presentation-window change, per Phases 3.8.3–3.8.5's own precedent.

None of these are implied complete by this document. This document is
Phase 3.10.1 only.
