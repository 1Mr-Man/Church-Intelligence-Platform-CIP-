# Phase 8 — Production Integration (OBS/vMix)

## Baseline

`docs/phase-4-master-plan-gap-audit.md` and `docs/phase-7-audit.md` both
named "OBS/vMix/livestream integration" as a real, `NOT STARTED` master-
architecture gap - `integrations/obs`/`integrations/vmix` were 6-line
placeholder crates, explicitly out of scope since Phase 1. The user
explicitly named this as the next phase and supplied two new external
product-analysis documents; both converge on the same target the repo's
own audit already identified (treat OBS/vMix as first-class citizens,
not afterthoughts). See `docs/phase-8-audit.md`.

## Design choices

Three real decisions, resolved in the audit rather than left open (see
`docs/phase-8-audit.md`'s own "Design choices" for the full reasoning):

1. **Scope**: text/title push into an operator-designated source, never
   scene switching or recording/streaming control.
2. **Connection model**: fresh connection per push, not a persistent
   session - this also makes config live-editable without a restart.
3. **Failure mode**: best-effort, on a dedicated worker thread, never
   fatal to CIP's own local Stage/Confidence/Lobby display.

## What was built

- **`integrations/obs`**: a real `obs-websocket` v5 client
  (`ObsClient`/`push_text`) - plain `ws://` (no TLS backend pulled in at
  all), the real `Hello`/`Identify`/`Identified` handshake with
  SHA256-based challenge/response auth, one request type
  (`SetInputSettings`). 6 tests, all against a real in-process WebSocket
  server speaking the actual protocol (not a mock trait) - handshake with
  and without a password, a real OBS-reported request failure, a
  connection-refused case, and a standalone vector proving the auth
  algorithm matches `obs-websocket`'s own spec.
- **`integrations/vmix`**: a real vMix HTTP API client
  (`VmixClient`/`push_text`) - plain `http://`, a `SetText` GET request
  with `Input`/`SelectedName`/`Value` query parameters. 4 tests against a
  real hand-rolled HTTP/1.1 server, including one asserting the exact
  query string vMix would receive.
- **`apps/desktop/src-tauri/src/production.rs`**: orchestration module -
  `ProductionIntegrationConfig`/`ProductionIntegrationStatus` in
  `AppState` (both in-memory `Mutex`s, defaulting to disabled),
  `push_to_configured_targets` (best-effort, worker-thread, no-op when
  nothing is configured), `slide_push_text` (the plain-text form of a
  `RenderedSlide` this module pushes), `test_obs_connection`/
  `test_vmix_connection` (synchronous, for the operator's "Test
  Connection" button).
- **`apps/desktop/src-tauri/src/commands.rs`**: `set_production_integration_config`,
  `get_production_integration_status`, `test_obs_connection`,
  `test_vmix_connection` - wired into `display_presentation` and
  `clear_active_presentation` (the exact two call sites
  `broadcast_to_live_screens` already uses) as an additional best-effort
  push step.
- **Frontend**: `domain/production.ts` (wire-format mirrors),
  `commands.ts` wrappers, a new `ProductionIntegrationPanel.tsx` (OBS/
  vMix host/port/credentials/source fields behind an enable checkbox
  each, Test Connection buttons, last-push status), wired into
  `LiveChurchBrain.tsx` only (matching `DisplayRegistryPanel`'s own
  precedent - an operator physical-setup concern, not a replay concern).

## Testing boundary

Neither OBS nor vMix is installed in this container. Both client crates
are proven against real, protocol-correct in-process servers (a real
`tungstenite`-based WebSocket handshake for OBS, a hand-rolled HTTP/1.1
response for vMix) rather than mock traits - the wire format itself is
tested, not a substitute. `production.rs`'s own orchestration (worker-
thread dispatch, best-effort failure handling) is thin glue over these
already-tested clients and follows this codebase's established
discipline of not adding redundant command-level tests for such glue
(see `commands.rs`'s own test-module header, and Phase 7.3's identical
reasoning for `remove_acoustic_reference`). Environment C (an operator
with real OBS/vMix installed) is the decisive, honestly-named pending
gate.

## Full regression result

`cargo fmt/clippy/check/test --workspace`: clean, both feature configs
(10 new tests: 6 in `cip-integrations-obs`, 4 in `cip-integrations-vmix`;
zero new tests in `cip-desktop` itself, per the testing-boundary
decision above; zero regressions anywhere else). Frontend
`typecheck`/`lint`/`test`/`build`: clean, 266/266 tests (261 pre-existing
+ 5 new), same 5 pre-existing lint warnings as before this phase.

## Windows rebuild

This phase adds two new Rust crates (`cip-integrations-obs`,
`cip-integrations-vmix`) and new orchestration code compiled into the
desktop binary - a genuine rebuild with direct binary proof, not a
frontend-only rebuild. New dependencies (`tungstenite`, `ureq`, `sha2`,
`base64`) are pure-Rust with no TLS backend enabled (`ureq`'s default
`tls`/`gzip` features explicitly disabled) - verified to add no
OpenSSL/`ring`/`rustls` dependency via `cargo tree`, matching this
project's established Windows-cross-compilation discipline. See
`pilot-evidence/8/windows/installer-contents-verification.json`.

## Architectural safety diff

- Exactly four new Tauri commands, zero new events, zero new database
  schema.
- `core/presentation` and `core/music` (domain contract crates) are
  untouched - this phase adds a new *sink*, not a new domain concept.
- `broadcast_to_live_screens`, every existing `DisplayScreen`/
  `RouteMode` code path, and CIP's own local display windows are
  byte-identical to before this phase - the production push is a purely
  additive step alongside the existing broadcast, never a replacement
  for it, and a push failure never touches the local display path.
- `production_integration_config`/`production_integration_status` are
  in-memory only - a restart clears them, matching the "in-memory/
  session-scoped" design choice, not a bug.

## Environment A / B / C

- **Environment A** (this container): PASSED - full backend and frontend
  regression green, including 10 new tests proving both clients against
  real protocol wire bytes.
- **Environment B**: unavailable, pre-existing container limitation.
- **Environment C**: NOT YET VERIFIED - the decisive pending gate is the
  operator's own real-hardware test: with real OBS and/or vMix running,
  configure a target in the Production Integration panel, click Test
  Connection and confirm the named source/input updates, then Save and
  confirm displaying a real verse/slide in CIP updates that same source/
  input live, and that clearing the display blanks it.

## Known limitations

- Text/title push only - no scene switching, no source visibility
  toggling, no recording/streaming control (explicitly deferred, see the
  audit's "Design choices" #1).
- No NDI output - a distinct, much larger real-time video-frame
  streaming concern, architecturally unrelated to this phase's text-push
  scope.
- Config does not persist across restarts (in-memory/session-scoped by
  design this phase - a small, clean, explicitly named follow-up, not
  attempted here).
- vMix's own `SetText` API returns a bare HTTP 200 regardless of whether
  the named input/field actually exists - a property of vMix's own API
  surface, not a gap in this client (see `integrations/vmix`'s own
  module docs).
- This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware, and neither client has ever been exercised
  against a real OBS/vMix installation - see `physicalHardwareStatement`
  in `release/windows/release-manifest.json`.

## Deferred work

- Scene switching / source visibility / recording control (OBS), wider
  input control (vMix).
- NDI output.
- Config persistence across restarts.
- Real-hardware Environment C verification against actual OBS/vMix
  installations.
- The remaining master-architecture gaps: internet/hybrid intelligence,
  multi-language support, church/user roles & permissions.

## Final gate

Environment A: **PASS**. Environment C: **PENDING**. This phase adds a
new, optional, best-effort production-integration sink behind two new
crates and four new commands - it introduces no new domain concept, and
every existing display/presentation code path is unchanged.
