# Phase 3.10.4 — Multi-Window Lifecycle (Disconnect/Reconnect)

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `a88679b` (Phase 3.10.3)

## Why this phase exists

The fourth and final sub-phase of the multi-screen pillar the user
proposed at 3.10.1: audit (3.10.1), Display Registry (3.10.2),
Presentation Router (3.10.3), multi-window lifecycle (3.10.4). Every
prior 3.10.x phase's own "known limitations" section named this phase
explicitly for one gap: "No live monitor hot-plug detection... Live
disconnect/reconnect handling is Phase 3.10.4's scope."

## What this phase can honestly deliver in this container

Tauri exposes no monitor-hot-plug push event this session could find in
the vendored `tauri` crate - `available_monitors`/`primary_monitor` are
pull-based snapshots, not a subscribable stream. This container also has
exactly one display and no `tauri::test` harness (this project's
standing testing boundary for window-lifecycle behavior), so a real
unplug/replug cannot be reproduced or verified here regardless of what
API existed. Building speculative OS-level hot-plug handling this
session cannot exercise or verify would violate this project's own
audit-first, evidence-based discipline.

What *can* be delivered, and is real, useful, and verifiable: closing
the one genuine functional gap this container's own architecture makes
visible - **an already-open display window was never repositioned**.
Every prior 3.10.x phase's placement logic (`resolve_screen_placement`)
already re-enumerates monitors fresh on every call, so a monitor that
reconnects, or a role reassigned to a different monitor, is already
picked up correctly *the next time a window is created*. But
`open_display_window`'s "already exists" branch only ever called
`show()`/`set_focus()` - never applied the freshly-resolved placement to
a window that was already open. That meant reconnecting a monitor, or
reassigning a role while a screen's window was already open, had no way
to actually move that window without closing and reopening it - a real,
concrete usability gap in exactly the disconnect/reconnect workflow this
phase is named for.

## What was built

- `presentation_display.rs`'s `open_display_window`: the existing-window
  branch now reapplies the freshly-resolved `placement` (`set_position`
  + `set_size`, same DPI-correct logical-pixel conversion as window
  creation) before showing/focusing, **only when `placement` is `Some`**
  - a `None` placement (nothing assigned, or the assigned monitor still
    not connected) leaves an already-open window exactly where it is,
    never overriding an operator's own manual drag on a machine with no
    Display Registry assignment. This is the same "never touch what the
    operator placed by hand" boundary 3.10.2 established for window
    creation, now extended to the reposition path.
- `commands.rs`: no change needed - `open_presentation_display` already
  resolves placement fresh on every call and passes it through to
  `open_display_window`; the fix lives entirely in how that function
  treats an already-open window.
- Frontend: `PresentationCard.tsx` gained a **Reposition** button on
  each open screen's row, alongside the existing Close and Live/Held
  controls - calls the exact same `onOpenScreen` handler the closed-state
  "Open" button already uses (the underlying command is now meaningful
  when the window is already open). No new Tauri command, no new event.

## Design decisions

- **No speculative hot-plug event handling.** Rather than guess at a
  Tauri API for monitor-change notifications this container cannot
  exercise, this phase only closes a gap that is real regardless of
  whether push notifications ever arrive: the operator explicitly
  triggering a reposition. A future phase with real Windows access could
  add push-based auto-reposition on top of this same `set_position`/
  `set_size` mechanism without any further architectural change here.
- **Reposition reuses the Open action, not a new command.** `open_display_
  window` already had to decide what to do with an existing window; the
  natural fix was making that decision placement-aware, not adding a
  parallel `reposition_display_window` command that would duplicate the
  exact same DPI-conversion logic `open_display_window` already has.
- **Never move a manually-placed window.** The `None`-placement case is
  deliberately a no-op on an existing window - the same principle 3.10.2
  established for window *creation* (preserve pre-3.10.2 behavior when
  nothing is assigned) now also holds for repositioning an *already-open*
  window.

## Full regression result

Backend: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D
warnings` clean (both default and `--features whisper`), `cargo test
-p cip-desktop`: 277/277 passed (unchanged count - this phase's change is
Tauri window-lifecycle glue, not independently unit-testable, matching
this project's established convention; verified via direct code
inspection and the full existing suite still passing). Frontend: `tsc -b`
(0 errors), `oxlint` (0 errors, same pre-existing warning pattern),
`vitest` 211/211 passed (unchanged - no new domain contract), `vite build`
clean.

## Known limitations

- **No real-time push notification when a monitor disconnects or
  reconnects.** Tauri exposes no such API this session found; the
  operator must explicitly click Reposition (or Close/Open) after a
  reconnect for a screen to snap back onto the right monitor. The
  Display Setup panel (Phase 3.10.2) already shows a disconnected
  assigned monitor's status on demand (via its Refresh button), so the
  information needed to know *when* to click Reposition is available,
  just not pushed automatically.
- **No verification of actual Windows monitor-unplug OS behavior.** What
  happens to an already-open WebView2 window's on-screen position when
  its monitor is physically unplugged is OS/driver behavior CIP does not
  control and this container cannot reproduce - documented honestly
  rather than assumed. The Reposition button is the operator's recovery
  action regardless of what that OS behavior turns out to be.
- **Reposition button has no confirmation or explanation of *why* a
  screen might need it** beyond its tooltip - a future phase could
  surface a clearer "this screen's monitor is currently disconnected"
  warning directly on the screen's own row, cross-referencing the
  Display Registry's connection state, rather than requiring the
  operator to check the separate Display Setup panel.
- This closes the last of the four sub-phases the user proposed for the
  multi-screen pillar (3.10.1 audit, 3.10.2 Display Registry, 3.10.3
  Presentation Router, 3.10.4 this phase). Of the five original pillars
  from the project's master architecture document, multi-screen
  presentation output is now the most complete; semantic/paraphrase
  Bible detection, real audio fingerprinting, and multi-language support
  remain **not started** - see `docs/phase-4-master-plan-gap-audit.md`
  for the full cross-reference against the master architecture document.

## Final gate

| Item | Status |
|---|---|
| No speculative/unverifiable OS hot-plug handling introduced | DONE |
| Reconnect/reassignment is now recoverable by an explicit operator action, not just at window creation | DONE |
| An already-open window with no monitor assignment is never moved (manual placement preserved) | DONE |
| Full regression green (backend + frontend) | DONE |
| Real Windows monitor unplug/replug test (Environment C) | **NOT YET PERFORMED** - no real Windows hardware with a second display in this container |

**Phase 3.10.4: Environment A verification PASS.** Real Windows hardware
with a second monitor remains the decisive pending gate for this entire
multi-screen pillar (3.10-3.10.4), per this project's standing
discipline - see `pilot-evidence/3.10.4/`.
