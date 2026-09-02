# Phase 11: Local Congregant Companion View

## Baseline

Trigger: the user's own item 11 from a pasted advice list ("a tiny
local-network HTTP server broadcasting the current slide/verse to a
phone browser... for 'follow along and save a note'"), followed by the
explicit instruction "Keep going into the local congregant companion
view next." Full reasoning in `docs/phase-11-audit.md`.

## Design choices

See `docs/phase-11-audit.md` in full. Summary: mirrors Stage's existing
`Active` `PresentationItem` exactly, never a second content path; no
congregant data ever reaches CIP (a "note" lives only in the phone's own
`localStorage`, the server has no write endpoint at all); Admin-gated to
enable (joining Phase 10's seven gated commands), not to view; plain
`std::net::TcpListener`, hand-rolled, no new dependency, mirroring Phase
8's own "plain `http://` for a LAN-local protocol" reasoning; fixed port
`49876`; LAN address detection via the dependency-free `UdpSocket::connect`
routing-table trick, honestly returning no candidate address rather than
fabricating one when no route exists.

## What was built

- **`apps/desktop/src-tauri/src/companion.rs`** (new): `CompanionSnapshot`,
  `CompanionServerHandle`, `CompanionStatus`, `CompanionError`,
  `spawn_server`/`stop_server` (a real, Tauri-agnostic
  `TcpListener`-based server on a dedicated worker thread with a genuine
  stop mechanism - a shared `AtomicBool` plus a self-connect to unblock
  the blocking `accept()` call), `enable`/`disable`/`status`/
  `update_snapshot` (the thin Tauri-specific wiring), the served HTML
  page (fully self-contained, inline CSS/JS, polls `/api/current` every
  two seconds, notes textarea backed by `localStorage` only).
- **`apps/desktop/src-tauri/src/state.rs`**: `companion_snapshot`
  (`Arc<Mutex<Option<CompanionSnapshot>>>`, shared with the server
  thread) and `companion_server` (`Mutex<Option<CompanionServerHandle>>`,
  `None` by default - off unless an Admin turns it on).
- **`apps/desktop/src-tauri/src/errors.rs`**: new `AppError::Companion`
  variant, categorized under `LogCategory::Network`.
- **`apps/desktop/src-tauri/src/commands.rs`**: `CompanionStatusDto`; 3
  new commands (`enable_congregant_companion`, `disable_congregant_companion` -
  both Admin-gated via `ensure_admin`; `get_congregant_companion_status` -
  open to any logged-in operator); `companion::update_snapshot` calls
  added to `display_presentation` and `clear_active_presentation`, the
  exact same two call sites `production::push_to_configured_targets`
  already hooks into, so the companion view and any configured OBS/vMix
  target always change in lockstep with Stage.
- **Frontend**: `domain/companion.ts` (`CompanionStatus`); 3 new
  `lib/commands.ts` wrappers; new `components/workspace/CongregantCompanionPanel.tsx`
  (mirrors `ProductionIntegrationPanel`'s own shape: status, candidate
  URLs, Start/Stop/Refresh); wired into `LiveChurchBrain.tsx` next to
  `ProductionIntegrationPanel`.
- **`docs/congregant-companion.md`** (new): the permanent reference doc
  naming the mechanism, the privacy posture, and what this is/isn't a
  boundary against.

## Testing boundary (a genuine strengthening)

Unlike almost every other Tauri-command-backed feature in this codebase,
`companion.rs`'s core server logic is entirely Tauri-agnostic (it
operates on a plain `Arc<Mutex<Option<CompanionSnapshot>>>`, never an
`AppHandle`/`State`), so its tests are not limited to pure-function
proxies for an untestable command - they bind a real `TcpListener` on an
OS-assigned ephemeral port and make real `TcpStream` GET requests
against it: `GET /` returns the real served HTML and contains the
local-only notes disclosure; `GET /api/current` returns accurate JSON
both before and after the snapshot changes; an unknown path 404s; and
the server genuinely stops accepting connections after `stop_server` (a
`TcpStream::connect` retry loop that expects eventual refusal). 9 new
Rust tests in `companion.rs` (4 pure-function, 5 real-socket). The Tauri
command wrappers stay thin and untested directly, per this project's
standing "no `tauri::test` harness" convention. Frontend: 4 new
`commands.ts` wrapper tests (forwarding + outside-Tauri rejection) and 2
new domain contract tests (`CompanionStatus` running/stopped shapes).

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs (default and `--features whisper`).
- `cargo check --workspace` / `cargo check --features whisper`: clean.
- `cargo test --workspace`: 1010 passed, 0 failed (default config, up
  from Phase 10's 1001 - 9 new, all in `companion.rs`).
- `cargo test --features whisper` (desktop crate): 341 passed, 0 failed
  (up from Phase 10's 332 - 9 new).
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings,
  unchanged) / `npm run test -- --run` (280 passed, up from Phase 10's
  274 - 6 new) / `npm run build`: all clean.

## Architectural safety

- 3 new Tauri commands, zero new events, zero new migrations (no
  persistence at all - the snapshot is in-memory/session-scoped,
  identical precedent to `production_integration_config`/
  `current_operator`).
- The two `companion::update_snapshot` call sites sit immediately beside
  `production::push_to_configured_targets`'s own existing calls in
  `display_presentation`/`clear_active_presentation` - neither call's
  existing behavior is touched, both are additive lines beside it.
- `core/bible`, `core/service`, `core/presentation` (every domain
  contract crate) are entirely untouched - the companion server only
  ever reads the same `RenderedSlide` the display-window path already
  produces, never a second render or a second content decision.
- No new workspace dependency - `spawn_server`/`stop_server` use only
  `std::net`/`std::sync`/`std::thread`, matching this codebase's
  existing worker-thread precedent (`production.rs`) and Phase 8's own
  "plain `http://`, no framework" reasoning for LAN-local protocols.

## Windows rebuild

Required: this phase changes Rust code compiled into the desktop binary
(new module, new `AppState` fields, new `AppError` variant, three new
commands, two new call sites in existing commands). See
`pilot-evidence/11/windows/installer-contents-verification.json` and the
updated `release/windows/release-manifest.json`.

## Known limitations (honest, not deferred silently)

- **No authentication, no rate limiting, no connection cap** on the
  companion server itself - a LAN-only, read-only, no-PII surface judged
  not to need either for this phase. Anyone on the same LAN can open the
  page while it's running; this is a convenience for congregants who
  want to follow along, not a boundary against a determined attacker on
  the same network - see `docs/congregant-companion.md`'s own explicit
  section on this.
- **No QR code, no mDNS/Bonjour auto-discovery** - the operator reads
  the address off the desktop panel and shares it manually (a slide,
  a spoken announcement).
- **No push/WebSocket** - the page polls every two seconds; a missed
  poll is at most a two-second lag, never a stuck stale state (the next
  poll always corrects it).
- **No history** - only the current item, matching Stage's own
  single-item model; a congregant who looks away misses what was shown,
  same as looking away from the projector.
- **IPv4 only** - both the `TcpListener::bind` and the
  `UdpSocket::connect` address-detection trick use plain IPv4; an
  IPv6-only network would not reach this server.
- **Address detection can return nothing** - on a machine with no
  configured network route at all, `detect_local_ip` honestly returns
  `None` rather than fabricating an address; the panel shows the port so
  the operator can construct the address manually.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware**, and the companion server has never been
  opened from a real phone browser on a real church LAN - see
  `physicalHardwareStatement` item 20 in the updated release manifest.

## Final gate

Environment A (build-time verification, full regression including 5 new
real-socket integration tests, direct binary symbol inspection):
PASS. Environment C (a real operator enabling the server, and a real
phone on the same LAN opening the page and seeing it track Stage live):
not yet performed - carried forward into `physicalHardwareStatement` per
this project's standing discipline.
