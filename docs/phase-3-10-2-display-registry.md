# Phase 3.10.2 — Display Registry & Monitor Placement

## Baseline

- Branch: `claude/cip-foundation-init-i85g87`
- Starting HEAD: `585fb7c` (Phase 3.10.1 audit)

## Why this phase exists

Phase 3.10.1 was a pure audit (no code changes) of the presentation
window/monitor pipeline, performed at the user's explicit request before
any further multi-screen work, given how much Windows-specific debugging
that pipeline had already been through. The user then specified Phase
3.10.2 in detail: give each physical monitor a stable identity for the
session, let the operator assign it a role (Projector, Stage, Confidence,
Lobby, Operator, or Unassigned), persist that assignment, and place the
existing presentation window directly on the assigned monitor when
opened - with an explicit instruction not to touch the proven Windows-safe
window-creation pipeline itself (async command → `WebviewWindowBuilder` →
`position(...)` → `build()` → the existing resize/paint workaround).

See `docs/phase-3-10-1-audit.md` for the audit this phase builds on.

## Scope (as given by the user)

1. Discover monitors (reuse the existing Tauri monitor-enumeration API).
2. Give monitors stable logical identities for the current session.
3. Show them in an operator configuration UI.
4. Let the operator assign a role.
5. Persist the assignment if safely possible.
6. Position the existing presentation window on the assigned monitor.

Explicitly **not** in scope for this phase (per the user's own
sequencing): independently routing different content to different roles
(Phase 3.10.3) and multi-window disconnect/reconnect lifecycle handling
(Phase 3.10.4).

## Design summary

**Two distinct role concepts, deliberately not merged.** Phase 3.10 already
has a `DisplayScreen` enum (Stage/Confidence/Lobby) naming which *content
stream/window* CIP drives. This phase adds a separate `DisplayRole` enum
(Unassigned/Operator/Projector/Stage/Confidence/Lobby) naming which role a
*physical monitor* plays. They are connected only by an explicit,
documented bridge function, `display_registry::screen_role`, which maps
`DisplayScreen::Stage → DisplayRole::Projector` (the primary
congregation-facing screen is placed on the monitor assigned the
"Projector" role) and leaves Confidence/Lobby name-aligned. `DisplayRole::
Stage` (per the user's own taxonomy, "speaker-facing information") has no
corresponding content stream yet - documented as future scope, not
implemented here.

**Monitor identity is honest about its own limits.** Tauri's `Monitor` API
exposes no real OS-level stable ID - only `name` (often present, e.g.
`"HDMI-1"`), `position`, `size`, and `scale_factor`. `compute_monitor_id`
prefers the OS-reported name when present, and falls back to a
`"unnamed@{x},{y}-{width}x{height}"` position/resolution fingerprint
otherwise. This is CIP's own best-effort session identity, not a claim of
a real hardware serial number - documented as a known limitation below.

**Persistence is the one deliberate append-only-log exception in this
codebase.** Every other table in this project is an append-only event log
(matching its audit-trail design). Role assignment has "current value,"
not "one row per event," semantics - a monitor has exactly one currently
assigned role - so `display_role_assignments` uses a genuine SQL upsert
(`INSERT ... ON CONFLICT(monitor_id) DO UPDATE`). This is called out
explicitly rather than silently deviating from convention. Storage is
global (not scoped to a service), matching the Phase 3.6 precedent set by
`saved_scriptures` - a machine's monitor layout is a property of the
machine, not of any one service.

**The proven window-creation pipeline is untouched, only extended.**
`open_display_window` still does exactly what it did before this phase -
async command, build the window, apply the Windows resize-nudge
workaround - and its Windows-only `#[cfg(target_os = "windows")]` block is
unchanged in structure. The only change is that it now accepts an
`Option<MonitorPlacement>`: `Some` positions and sizes the window on the
assigned monitor; `None` (no role assigned, or the assigned monitor is
currently disconnected) falls back to the exact pre-3.10.2 behavior - an
unpositioned 1280x720 window. This preserves the explicit "single-display
machines keep working exactly as before" requirement.

**DPI-correct placement.** `Monitor`'s own position/size are in *physical*
pixels; `WebviewWindowBuilder::position`/`.inner_size` expect *logical*
pixels. `resolve_role_position` returns physical values; `open_display_window`
converts them via Tauri's own `PhysicalPosition/PhysicalSize::to_logical`
rather than a hand-rolled division, to avoid an off-by-scale-factor
placement bug on a non-100%-scaled monitor (a common real case on Windows
laptops). The pre-existing Windows resize-nudge workaround was previously
hardcoded to `1280.0, 720.0`; it now consumes the same `logical_size` the
window was actually built with, so it can no longer silently fight new
placement sizing on the one platform it exists for.

**No literal OS fullscreen toggle.** This phase places and sizes the
window to exactly cover the assigned monitor's bounds, but does not call
a platform fullscreen API. Toggling real OS fullscreen state is judged a
separate, riskier UX/OS-state decision than monitor-fill positioning, and
is called out explicitly as a known limitation below rather than silently
decided.

## What was built

- `database/migrations/0012_display_role_assignments.sql` +
  `database/src/migrations.rs` — new `display_role_assignments` table
  (`monitor_id` PK, `role`, `updated_at`), the one deliberate upsert-table
  exception to this project's append-only convention (documented in the
  migration file itself).
- `apps/desktop/src-tauri/src/display_registry.rs` (new module) —
  `DisplayRole` enum; `screen_role` bridge function; `compute_monitor_id`;
  `PhysicalMonitor`/`enumerate_monitors` (Tauri glue over
  `AppHandle::available_monitors`/`primary_monitor`); `Display` (the
  merged physical+assignment view sent to the frontend);
  `merge_displays` (pure - handles connected+assigned, connected+
  unassigned, and disconnected-but-still-assigned monitors, the last with
  placeholder geometry and `connected: false`, never fabricated real
  values); `MonitorPlacement`; `resolve_role_position` (pure - only
  matches a connected, assigned monitor). 11 unit tests.
- `apps/desktop/src-tauri/src/persistence.rs` — `assign_display_role`
  (the upsert) and `list_display_role_assignments`. 3 new unit tests,
  including one proving re-assignment replaces rather than duplicates.
- `apps/desktop/src-tauri/src/presentation_display.rs` —
  `open_display_window` gained an `Option<MonitorPlacement>` parameter;
  logical-pixel conversion via `to_logical`; the Windows resize-nudge
  workaround now targets the real placed size instead of a hardcoded
  1280x720.
- `apps/desktop/src-tauri/src/commands.rs` — new `resolve_screen_placement`
  helper (enumerates monitors, loads persisted assignments, merges, and
  resolves the placement for a given `DisplayScreen` via `screen_role`);
  `open_presentation_display` and `display_presentation` both now resolve
  and pass placement before opening a window; two new commands,
  `list_displays` and `assign_display_role`.
- `apps/desktop/src-tauri/src/lib.rs` — registers the new module and the
  two new commands.
- Frontend: `domain/presentation.ts` gained `DisplayRole` and `Display`
  types; `lib/commands.ts` gained `listDisplays`/`assignDisplayRole`
  wrappers; new `components/workspace/DisplayRegistryPanel.tsx` — a
  collapsible operator panel (mirroring `PilotDiagnosticsPanel`'s style)
  listing every detected/assigned monitor with a role `<select>` per row
  and a Refresh button; wired into `LiveChurchBrain.tsx` only (not
  `ServiceReplay.tsx` - monitor/role assignment is a one-time physical-
  setup concern relevant to the live-service operator view, not to replay
  of a past service, which drives no physical display placement decision
  of its own).

No change to the presentation domain model (`presentation.rs`'s
Prepared→Active→Stopped lifecycle), no change to event emission, no
change to `DisplayScreen`'s own three variants.

## Full regression result

Backend: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D
warnings` clean (both default and `--features whisper`), `cargo test
-p cip-desktop`: 273/273 passed (up from 256 in Phase 3.10 - 14 new
tests: 11 in `display_registry.rs`, 3 in `persistence.rs`). Frontend:
`tsc -b` (0 errors), `oxlint` (0 errors, same pre-existing
`set-state-in-effect`/`only-export-components` warning pattern already
present on `PilotDiagnosticsPanel.tsx` and other panels), `vitest`
211/211 passed (1 new domain-contract test covering every `DisplayRole`
and the disconnected-but-assigned `Display` shape), `vite build` clean.

## Known limitations

- **Monitor identity is best-effort, not a real hardware ID.** Tauri
  exposes no OS-level stable monitor identifier. `compute_monitor_id`
  prefers the OS-reported name (when present) and falls back to a
  position/resolution fingerprint. Swapping two identically-named,
  identically-positioned monitors between sessions could be
  indistinguishable to this scheme - a genuine limitation of the
  underlying platform API, not something this phase can fully solve.
- **No literal OS fullscreen.** The presentation window is positioned and
  sized to exactly cover the assigned monitor's bounds, but no platform
  fullscreen call is made. A window manager or the taskbar could still be
  visually present depending on OS/desktop-environment behavior. Judged a
  separate, riskier decision to make in this phase - a real, deliberate
  scope boundary.
- **No live monitor hot-plug detection.** `list_displays` reflects
  whatever `available_monitors` reports at the moment it's called; a
  monitor plugged/unplugged after that is only reflected on the next
  explicit Refresh. Live disconnect/reconnect handling is explicitly
  Phase 3.10.4's scope, not this phase's.
- **Existing open windows are not moved retroactively.** Assigning a new
  role to a monitor only affects the *next* window opened for a screen
  mapped to that role; an already-open display window stays where it is
  until closed and reopened.
- **`DisplayRole::Stage` (speaker-facing info) has no content stream
  yet.** Only `DisplayScreen::Stage → DisplayRole::Projector` is wired;
  a genuinely separate speaker-monitor content stream is future scope.
- Of the five original pillars, two remain fully delivered (Sermon
  Harvest, multi-screen output) with multi-screen now also gaining real
  monitor placement; semantic/paraphrase Bible detection, real audio
  fingerprinting, and multi-language support remain **not started**.
  Phase 3.10.3 (Presentation Router) and 3.10.4 (multi-window lifecycle)
  remain **not started**, per the user's own explicit sequencing.

## Final gate

| Gate (as specified by the user) | Status |
|---|---|
| Monitor discovery reuses the existing Tauri API | DONE - `enumerate_monitors` calls `AppHandle::available_monitors`/`primary_monitor`, the same inherent methods traced in the 3.10.1 audit |
| Single-monitor behavior remains usable | DONE - no assigned role (or its monitor disconnected) resolves to `None` placement, byte-for-byte the pre-3.10.2 unpositioned-window path |
| Two-monitor case allows projector selection | DONE in code (`merge_displays`/`resolve_role_position` unit-tested against multi-monitor fixtures) - **not verified on real multi-monitor hardware in this container** |
| Window placement opens directly on the selected monitor | DONE - `resolve_screen_placement` → `open_display_window`, DPI-correct via `to_logical` |
| Windows safety preserved (async command boundary unchanged) | DONE - `open_display_window`'s async-command/build-ordering structure is unchanged from Phase 3.8.4 |
| Presentation rendering pipeline untouched | DONE - no change to `presentation.rs`, the renderer, or event payloads |
| Disconnect must not crash CIP | DONE - `merge_displays`/`resolve_role_position` both unit-tested for a disconnected-but-assigned monitor; resolves to `None` placement, never a panic |
| Existing presentation tests pass | DONE - all 256 pre-3.10.2 backend tests plus the pre-existing frontend suite still pass unchanged |
| Full regression green (backend + frontend) | DONE |
| Real Windows + HDMI hardware test (Environment C) | **NOT YET PERFORMED** - no real Windows machine with a second HDMI display is available in this container; this is the decisive pending gate for the "opens directly on the projector, no manual dragging" end-to-end claim |

**Phase 3.10.2: Environment A verification PASS.** The two-monitor
projector-selection behavior is proven correct in isolated unit tests
(`merge_displays`, `resolve_role_position`) but has not been exercised on
real multi-monitor hardware, and real Windows + HDMI re-test (Environment
C) remains the pending, decisive gate, per this project's standing
discipline.
