# Phase 8 — Audit: Production Integration (OBS/vMix)

## Trigger

Real Audio Fingerprinting (Phase 7.1-7.3) closed the enrollment CRUD
lifecycle for the master-architecture gap the user selected absent an
answer to Phase 7's own scoping question. The user has now explicitly
named the next target: "Production Integration" - `docs/phase-7-audit.md`
and `docs/phase-4-master-plan-gap-audit.md` both already identify this
as gap #6 ("OBS/vMix/livestream integration"), graded **NOT STARTED**:
`integrations/obs` and `integrations/vmix` are 6-line placeholder crates,
explicitly out of scope since Phase 1, holding an architectural boundary
and nothing else.

The user also supplied two new advice documents (external product
analyses of a comparable tool). Both name OBS/vMix/livestream output as
a real, expected integration for any church-service platform ("Treat
ProPresenter, EasyWorship, Resolume, OBS, vMix, etc. as first-class
citizens, not afterthoughts" / an explicit `OBS`/`vMix`/`Livestream`
branch under "06. Production" in the second document's own proposed
architecture diagram). Neither advice document changes the shape of this
gap - both converge on the same target the repo's own audit already
named.

## What's actually missing

Nothing feeds CIP's live presentation output anywhere beyond its own
Tauri display windows. A church already running OBS (for a livestream)
or vMix (for a broadcast production) has no way to have CIP push the
currently-displayed verse/slide text into that pipeline - the media
team must duplicate the text by hand into a separate OBS/vMix text
source, exactly the friction this platform exists to remove.

## Design choices

Three real decisions, each resolved below rather than left as an
open fork - none of them rise to the level of a genuine, comparably-sized
user-facing choice (the shape of "push current slide text to an external
production tool" is not ambiguous the way, say, "confirm-before vs.
undo-after" was in Phase 6.2):

1. **Scope: text/title push, not scene control.** OBS/vMix each expose
   much larger APIs (scene switching, source visibility, transitions,
   recording control). Pushing CIP's current slide text into an
   operator-designated text source/title field is the direct analog of
   what CIP's own Stage/Confidence/Lobby screens already do (mirror the
   one `Active` item, spec section 10 unchanged) - it requires no new
   domain concept, no scene-graph modeling, and cannot itself put the
   wrong *scene* on air, only wrong *text* in a source the operator
   configured for exactly this purpose. Scene switching is a
   substantially larger, riskier surface (mis-triggering it could cut
   the actual broadcast) and is left an explicit, documented gap.
2. **Connection model: fresh connection per push, not a persistent
   session.** `AppState`'s existing engines (Whisper, embedding,
   acoustic) are built once at startup and never hot-swapped - but those
   are expensive model loads. An OBS/vMix push is an infrequent (once
   per verse/slide change), cheap network round-trip; holding a
   persistent WebSocket/HTTP connection open for an entire service adds
   reconnection-on-drop complexity for no real benefit over "connect,
   send, close" on each push. This also means target configuration
   (host/port/password/source name) can be **live-editable without a
   restart** - unlike model provisioning, there is no engine to rebuild,
   only connection parameters read fresh on the next push.
3. **Failure mode: best-effort, never fatal to the local display.**
   Exactly the same discipline Phase 7.3 just established for file
   deletion on remove: CIP's own Stage/Confidence/Lobby display must
   never be blocked, delayed, or degraded by an unreachable or
   misconfigured OBS/vMix target. A push failure is logged and surfaced
   in a status panel, never thrown back at `display_presentation` itself.

## Protocol choice (technical, not user-facing)

- **OBS**: the real, published `obs-websocket` v5 protocol (JSON over a
  plain `ws://` WebSocket, RPC-request/response with a `Hello`/
  `Identify`/`Identified` handshake and SHA256-based challenge/response
  auth when a password is configured) - not scraped, not guessed. A
  `SetInputSettings` request updates a text source's `text` field.
- **vMix**: the real, published vMix HTTP API
  (`http://host:port/api/?Function=SetText&Input=...&SelectedName=...&Value=...`)
  - a plain query-string GET, XML response.
- Both are **local-network-only by design** (OBS/vMix run on the same
  machine or LAN as the media team, never over the public internet) -
  neither protocol needs TLS for this use case, so the client crates use
  `tungstenite` (WebSocket) and `ureq` (HTTP) with no TLS backend
  enabled at all, avoiding OpenSSL/native-tls entirely. This matches
  this project's own established Windows-cross-compilation discipline
  (pure-Rust dependencies wherever the actual protocol allows it, the
  same reasoning that chose `rustfft`/`hound` over any C-toolchain
  alternative in Phase 7.1) - verified against a genuine
  `x86_64-pc-windows-gnu` rebuild before this phase is called done.

## Testing boundary

Neither OBS nor vMix is installed in this container, and this project's
own established discipline (documented repeatedly - Phase 7.1's
synthetic-signal fingerprinting tests, Phase 3.8.7.3's own testing-
boundary statement) is to build real, protocol-correct clients and prove
them against an in-process test double that speaks the real wire
protocol, rather than fabricate an untestable "it should work" claim.
This phase adds:

- A minimal, real WebSocket server (via `tungstenite`, run in a test
  thread) that performs the actual `obs-websocket` v5 handshake and
  echoes back a real `RequestResponse` - the client's handshake and
  `SetInputSettings` call are proven against real wire bytes, not a
  hand-substituted mock trait.
- A minimal, real HTTP server (`std::net::TcpListener` + a hand-rolled
  HTTP/1.1 response, no new server framework dependency) that returns
  vMix's real XML success/failure shapes.

Environment C (an operator with real OBS/vMix installed) remains the
decisive, honestly-named pending gate - this phase makes the connection
real and testable at the protocol level, it does not claim to have run
against the real applications.

## What will be built

- `integrations/obs`: `ObsClient` (real v5 handshake, auth, one request
  type: `SetInputSettings`), `ObsError`, `ObsTarget` config struct.
- `integrations/vmix`: `VmixClient` (real `SetText` GET request),
  `VmixError`, `VmixTarget` config struct.
- `apps/desktop/src-tauri`: `production.rs` orchestration module - a
  `Mutex<ProductionIntegrationConfig>` in `AppState` (in-memory,
  session-scoped - persistence across restarts is an explicit, named
  deferred gap, not attempted here), a `Mutex<ProductionIntegrationStatus>`
  tracking last-push outcome per target; new Tauri commands
  `set_production_integration_config`, `get_production_integration_status`,
  `test_obs_connection`, `test_vmix_connection`. The push itself is
  wired into the same two call sites `broadcast_to_live_screens` already
  uses (`display_presentation`, `clear_active_presentation`), on a
  dedicated worker thread per push (mirroring the acoustic/speech worker
  precedent), never on the async command's own return path.
- Frontend: a new "Production Integration" panel (OBS/vMix host/port/
  password/source fields, Test Connection buttons, last-push status).

## What is explicitly deferred (named, not silently dropped)

- Scene switching / source visibility / transitions (OBS), and vMix's
  wider input-control surface beyond `SetText`.
- NDI output (a distinct, much larger real-time video-frame streaming
  concern, not a text-push concern - both advice documents name it
  alongside OBS/vMix but it is architecturally unrelated to this phase's
  scope).
- Config persistence across restarts (in-memory only this phase).
- Environment C verification against real OBS/vMix installations.
