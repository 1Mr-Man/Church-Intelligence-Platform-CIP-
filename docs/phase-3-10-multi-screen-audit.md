# Phase 3.10 — Multi-Screen Presentation Output (design + audit)

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `799de41` (Phase 3.9)

## Why this phase exists

The second of the five pillars the user asked for after sharing the
project's original master architecture document: "multi-screen
presentation output." The user confirmed, in response to this session's
own proposed sequencing, that all four remaining pillars (multi-screen,
semantic Bible detection, audio fingerprinting, multi-language support)
should continue, one real phase at a time, matching this project's
established discipline.

## Audit of the existing single-screen architecture

`presentation_display.rs` manages exactly one Tauri `WebviewWindow`, hardcoded
to the label `"display"`:

- `open_display_window(app)` / `close_display_window(app)` /
  `is_display_window_open(app)` - all label-agnostic in name but hardcoded
  to `DISPLAY_WINDOW_LABEL = "display"` in body.
- The window's own `Destroyed` handler calls
  `commands::clear_active_presentation` unconditionally on close - correct
  for exactly one window, wrong once more than one can exist (closing a
  *second* screen must not blank a *first* screen that is still open and
  visible to the congregation).

`presentation.rs` (the domain lifecycle - `Prepared -> Active -> Stopped`)
never references a window at all. The "at most one `Active` item per
service" invariant (spec section 10) is enforced purely against
persistence, with zero coupling to how many windows exist. This is the key
finding: **multi-screen does not mean multiple concurrent active items -
it means the one active item reaching more physical screens.** No change
to `presentation.rs` is needed or made.

`commands.rs`'s `display_presentation` opens the display window, then
commits `Prepared -> Active`, then emits `AppEvent::PresentationStarted`.
Critically, `events.rs::emit` calls `tauri::Emitter::emit` with no target
filter - **this already broadcasts to every listening webview, not just
one window.** Confirmed by reading the vendored Tauri `Emitter` trait
directly: `AppHandle::emit` with no `_to`/`_filter` suffix reaches the
whole app. This means a second (or third) display window that has
already subscribed to `PresentationStarted`/`PresentationStopped`
receives the exact same live update the first window always has, with
*zero* change to the emit call sites - the entire remaining gap is window
lifecycle management (open/close per screen) and per-window rendering
choice, not event plumbing.

The frontend (`main.tsx`) resolves which component to render purely by
reading the current webview's own label - `"display"` -> `PresentationDisplay`,
anything else -> `App`. `PresentationDisplay.tsx` is a passive renderer:
it hydrates once via `getPresentationDisplayState()` and then only ever
reacts to the two broadcast events. It already receives the full
`PresentationItem` (not just the rendered slide) in its payload, but only
ever renders the slide.

## Design

Three fixed display **roles**, matching what the master document's framing
and this session's own proposed scope named: **Stage** (the primary
congregation-facing output - this is the pre-3.10 single display, renamed
conceptually but not in window label, so nothing about its existing
behavior changes), **Confidence Monitor** (an operator/platform-facing
screen), and **Lobby / Overflow** (a second room's mirror of Stage).
Bounded to exactly these three - not an arbitrary N-window system - because
that is what was actually asked for and is honestly buildable and
verifiable in one phase; an unbounded screen-management UI is a distinct,
larger feature this phase does not attempt.

- New `presentation_display::DisplayScreen` enum (`Stage`/`Confidence`/`Lobby`),
  each with its own fixed window label (`"display"` for Stage - **unchanged**,
  preserving every pre-3.10 behavior byte-for-byte for the screen that
  already existed - `"display-confidence"`, `"display-lobby"`), window
  title, and operator-facing label. `open_display_window`/`close_display_window`/
  `is_display_window_open` all take a `DisplayScreen` parameter now. A new
  `any_display_window_open(app)` checks all three.
- The `Destroyed` handler's reconciliation is corrected to the honest
  invariant this phase's audit surfaced: only stop the active item when
  **no** screen remains open, not on any single screen's closure.
  `commands::close_presentation_display` (the explicit operator Close
  action) gets the same fix, for the same reason.
- `display_presentation` (the operator's core "Display" action) continues
  to open the **Stage** screen specifically, matching its pre-3.10 contract
  exactly - the Confidence Monitor and Lobby screens are opened separately,
  on operator request, via the existing `open_presentation_display`
  command (now screen-parametrized). Once open, they receive the exact
  same broadcast event Stage always has - no separate activation path, no
  second lifecycle.
- `get_presentation_display_state` now reports open/closed per screen
  (`screens: [{ screen, label, windowOpen }]`) instead of one boolean, so
  the operator UI can show and control all three independently.
- **No new detection logic, no new AI, no fabricated data.** The Confidence
  Monitor's frontend variant renders exactly the fields already present in
  the existing `PresentationDisplayPayload` (`item.template`,
  whether `item.sourceSuggestionId` is set, `item.status`) - fields this
  payload already carried and the display window already received, just
  never rendered. No new backend query, no new field invented for this
  purpose.

## Deliberately out of scope this phase

- Per-screen custom templates/branding (e.g. a church logo overlay) - a
  real, separate future feature.
- Network/NDI/OBS/HDMI-matrix output - explicitly out of scope since the
  original local-display foundation (`docs/presentation.md`), unchanged by
  this phase.
- An operator-configurable *arbitrary* number of screens - three fixed
  roles only, per the reasoning above.
- Any change to the "one active item per service" invariant - deliberately
  preserved exactly as-is; multi-screen is purely a broadcast-reach change.
