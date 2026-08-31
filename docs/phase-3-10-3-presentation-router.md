# Phase 3.10.3 — Presentation Router (per-screen Live/Held routing)

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `406c295` (Phase 3.10.2)

## Why this phase exists

The third of the sub-phases the user themselves proposed for the multi-
screen pillar (3.10.1 audit, 3.10.2 Display Registry, 3.10.3 Presentation
Router, 3.10.4 multi-window lifecycle). The user described 3.10.3 in one
line - "Presentation Router - independently route content to different
roles" - without the exhaustive spec 3.10.2 had. Because this touches the
same presentation window pipeline the user has repeatedly asked to be
careful with, and because "independently route content to different
roles" is genuinely ambiguous between two very different designs, this
phase started with a scoping question rather than an assumption.

## Scoping decision

Asked the user directly which of two designs was intended:

1. **Per-screen Live/Held toggle** (chosen): each open screen can be
   independently taken off the live broadcast (`Held`, frozen on its last
   content) or put back on it (`Live`). The domain model still has
   exactly one `Active` presentation item per service - unchanged from
   every prior 3.10.x phase.
2. **True independent content per screen**: different screens show
   genuinely different content at the same time. This would require
   breaking `presentation.rs`'s "at most one Active item per service"
   invariant - a much larger change to the core domain model that
   extensive existing tests, and three prior phases' own audits, treat as
   load-bearing.

The user picked option 1. This keeps `presentation.rs`'s domain lifecycle
(Prepared → Active → Stopped) and `presentation_display.rs`'s window-
creation pipeline completely untouched - the router is a pure delivery-
layer addition on top of both.

## Design summary

**Two enums stay separate on purpose.** `DisplayScreen` (Phase 3.10: which
content stream/window) and the new `RouteMode` (Live/Held: whether a
screen currently receives that stream's live updates) are unrelated
concerns - a screen's identity and its current subscription state.

**Delivery, not domain state.** `PresentationStarted`/`PresentationStopped`
previously broadcast to every open screen unconditionally
(`tauri::Emitter::emit`, confirmed to have no target filter in the
3.10.1 audit). This phase replaces that at exactly the two call sites
that emit them (`display_presentation`, `clear_active_presentation`) with
`broadcast_to_live_screens`, which computes the open, currently-`Live`
screens (`presentation_router::screens_to_broadcast`, pure and unit-
tested) and delivers to each individually via a new `events::emit_to`
(Tauri's per-window `emit_to`, as opposed to the broadcast `emit`). A
screen missing from the in-memory route-mode map defaults to `Live`, so
with nothing ever set this behaves identically to the pre-3.10.3
broadcast - the "single-display machines/simple case keeps working
exactly as before" property every 3.10.x phase has preserved.

**Held is a freeze, not a blank.** A `Held` screen keeps showing whatever
it last received; it is never sent a fabricated "different" slide, and it
is never force-blanked either - it simply stops being a target of future
`PresentationStarted`/`PresentationStopped` deliveries until switched back.

**Catching a screen back up.** When an open screen is switched from
`Held` to `Live`, it needs the *current* state, not just the next future
change. Rather than inventing a second content-delivery path, this phase
reuses the exact hydration pull every display window already performs on
mount (`getPresentationDisplayState` → `resolveHydratedPayload`): the
backend emits a new, payload-less `PresentationScreenSynced` event
targeted (again via `emit_to`) at just that one window; the frontend's
`PresentationDisplay` component, on receiving it, re-runs the same
`hydrate()` call its own mount effect uses. One hydration mechanism,
triggered from two places.

**Route mode is in-memory, not persisted.** `AppState.screen_route_modes:
Mutex<HashMap<DisplayScreen, RouteMode>>` matches this struct's existing
pattern for "current live-session state" (e.g. which screens are open is
derived from `AppHandle`, not stored at all) - a route mode is an
operator's live-session choice, not a durable setting, and resets to
`Live` for every screen on restart, same as every other live-session
field in `AppState`.

## What was built

- `apps/desktop/src-tauri/src/presentation_router.rs` (new module) -
  `RouteMode` enum (Live/Held) with `as_str`/`parse`; pure
  `screens_to_broadcast(open_screens, modes) -> Vec<DisplayScreen>`. 7
  unit tests.
- `apps/desktop/src-tauri/src/presentation_display.rs` - `DisplayScreen`
  gained `Hash` (needed as a `HashMap` key); no other change.
- `apps/desktop/src-tauri/src/state.rs` - new `screen_route_modes` field.
- `apps/desktop/src-tauri/src/events.rs` - new `AppEvent::
  PresentationScreenSynced` variant (`PRESENTATION_SCREEN_SYNCED`); new
  `emit_to` helper (thin wrapper over `tauri::Emitter::emit_to`, mirroring
  the existing `emit` wrapper over `tauri::Emitter::emit`).
- `apps/desktop/src-tauri/src/commands.rs` - new
  `broadcast_to_live_screens` helper, used at `display_presentation`'s and
  `clear_active_presentation`'s emission sites in place of the prior
  unconditional `emit`; `PresentationScreenState` gained a `route_mode`
  field, populated in `get_presentation_display_state`; new
  `set_screen_route_mode` command, which updates the mode and, only when
  the screen just became `Live` and its window is open, emits the sync
  signal to catch it up.
- `apps/desktop/src-tauri/src/lib.rs` - registers the new module and
  command.
- Frontend: `domain/presentation.ts` gained `RouteMode` and
  `PresentationScreenState.routeMode`; `events/eventNames.ts` gained
  `PresentationScreenSynced`; `lib/liveEvents.ts` gained
  `onPresentationScreenSynced`; `lib/commands.ts` gained
  `setScreenRouteMode`; `PresentationDisplay.tsx`'s hydration pull was
  extracted into a reusable `hydrate()` function, called both on mount
  and on receiving the sync event; `PresentationCard.tsx` gained a
  Live/Held toggle button per screen row; both operator surfaces that
  render `PresentationCard` (`LiveChurchBrain.tsx`, `ServiceReplay.tsx`)
  wire the new `onSetRouteMode` prop identically to their existing
  `onOpenScreen`/`onCloseScreen` wiring.

No change to `presentation.rs`'s domain model, no new database migration,
no change to `presentation_display.rs`'s window-creation pipeline.

## Full regression result

Backend: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D
warnings` clean (both default and `--features whisper`), `cargo test
-p cip-desktop`: 277/277 passed (up from 270 at the Phase 3.10.2 commit -
7 new tests, all in `presentation_router.rs`). Frontend: `tsc -b` (0
errors), `oxlint` (0 errors, same pre-existing warning pattern), `vitest`
211/211 passed (the event-count guard test updated from 54 to 55 for the
new `PRESENTATION_SCREEN_SYNCED` event; the `PresentationDisplayState`
domain-contract test extended to cover `routeMode`), `vite build` clean.

## Known limitations

- **Not "true" independent content per screen.** By explicit user choice
  (see Scoping decision above): a `Held` screen freezes on its last
  content, it does not show something different from what `Live` screens
  show. Multiple concurrently different content streams per screen would
  require breaking the single-`Active`-item domain invariant - out of
  scope for this phase.
- **Route mode is session-only, not persisted.** Every screen resets to
  `Live` on app restart - matches every other live-session field in
  `AppState`, but is a real limitation if an operator wants a `Held`
  screen (e.g. a Lobby permanently pinned to an announcement) to survive
  a restart.
- **No visual "frozen" indicator on the display window itself** - only
  the operator's own Presentation card shows a screen's current Live/Held
  state via the toggle button's label. The display window (what the
  congregation/room actually sees) looks identical whether it is showing
  live content or content it was frozen on - a deliberate simplicity
  choice for this phase, not evaluated against whether operators would
  want an on-screen indicator.
- **Not exercised on real multi-window hardware in this container** - the
  routing logic itself is proven in isolated unit tests
  (`screens_to_broadcast`); the full path (open two screens, hold one,
  display something new, confirm only the live screen updates, switch the
  held one back and confirm it catches up) has not been run against real
  Tauri windows, consistent with this project's standing "no `tauri::test`
  harness" testing boundary for window-lifecycle behavior.
- Of the five original pillars, multi-screen output continues to be the
  one under active development (now with monitor placement and per-screen
  routing); semantic/paraphrase Bible detection, real audio fingerprinting,
  and multi-language support remain **not started**. Phase 3.10.4
  (multi-window lifecycle - disconnect/reconnect) remains **not started**,
  per the user's own sequencing.

## Final gate

| Item | Status |
|---|---|
| Design ambiguity resolved with the user before implementing (not assumed) | DONE |
| `presentation.rs`'s single-active-item domain invariant left completely unchanged | DONE |
| `presentation_display.rs`'s window-creation pipeline left completely untouched | DONE |
| Default behavior (nothing ever set to Held) identical to pre-3.10.3 broadcast | DONE - unit-tested (`a_screen_with_no_explicit_mode_defaults_to_live`) |
| A Held screen freezes rather than blanking or fabricating content | DONE - by construction (it is simply not a delivery target) |
| Switching back to Live catches the screen up via the existing hydration path, no second content mechanism | DONE |
| Full regression green (backend + frontend) | DONE |
| Real multi-window operator test (open two screens, Hold one, verify independence, un-Hold, verify catch-up) | **NOT YET PERFORMED** - no `tauri::test` harness in this project; requires a real desktop run |

**Phase 3.10.3: Environment A verification PASS.** The real multi-window
operator walkthrough is the pending, decisive gate, per this project's
standing discipline - see `pilot-evidence/3.10.3/` for the rebuilt
installer's direct binary verification.
