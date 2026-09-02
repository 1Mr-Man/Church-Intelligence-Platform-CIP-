# Phase 11 audit: Local Congregant Companion View

## Trigger

The user's own item 11 from a pasted advice list, after Phase 10 (Church/
User Roles & Permissions) shipped: *"a tiny local-network HTTP server
broadcasting the current slide/verse to a phone browser (no app, no
cloud) for 'follow along and save a note.' Fits CIP's offline-first
constraint naturally since it never leaves the LAN."* Followed by the
user's explicit instruction: "Keep going into the local congregant
companion view next."

## What this is

A read-only, LAN-only HTTP server, off by default, that a congregant's
phone browser can open to see exactly what CIP's own Stage display is
currently showing - the same `Active` `PresentationItem`, nothing more,
nothing different. No app to install: the phone just needs a browser and
the church wifi. Plus a personal note box that never leaves the phone.

## Scope decisions

1. **Mirrors Stage exactly, never a second content path.** The
   companion page shows the same `RenderedSlide` content Stage/
   Confidence/Lobby already display (Phase 3.10's multi-screen model,
   unchanged). There is no separate "what should the congregation see"
   decision to make - it is definitionally the operator's own existing
   choice, broadcast one more place.

2. **No congregant data ever reaches CIP.** "Save a note" is real, but
   the note lives only in the phone's own browser `localStorage` - the
   HTTP server has no write endpoint at all, only two read-only routes
   (`GET /`, `GET /api/current`). This keeps the feature inside CIP's
   existing "no cloud, offline-first, no PII" posture (spec sections 33/
   34) without inventing a new local database table, a new sync
   mechanism, or any notion of congregant identity - there isn't one,
   and this phase doesn't add one.

3. **Admin-gated to enable, not to view.** Turning the server on is a
   configuration act with a real (if small) new attack-surface
   consequence - the app now listens on the LAN - so `enable_congregant_companion`/
   `disable_congregant_companion` require a logged-in Admin, joining the
   seven commands Phase 10 already gates. Once running, the page itself
   requires no login (a congregant's phone is not an operator account),
   matching the plain-HTTP, no-auth precedent Phase 8's vMix client
   already established for LAN-local protocols (`docs/phase-8-audit.md`'s
   "Protocol choice").

4. **Plain HTTP, hand-rolled, no new dependency.** No `axum`/`hyper`/
   `tiny_http` is added to the workspace. `apps/desktop/src-tauri`
   already has no HTTP-server dependency at all (OBS uses `tungstenite`
   as a *client*, vMix uses `ureq` as a *client* - Phase 8's own
   `integrations/vmix` docs explain why plain `http://` is the right,
   honestly-scoped choice for a LAN-local protocol; the companion
   server is the same reasoning applied to the *serving* side). A
   `std::net::TcpListener` on a dedicated worker thread, with a minimal
   hand-parsed GET-request-line reader and two fixed routes, is the
   smallest real implementation - and, unlike every other worker thread
   this codebase already has (`production.rs`'s fire-and-forget OBS/
   vMix pushes), this one is genuinely long-running, so it gets a real
   stop mechanism: a shared `AtomicBool` plus a self-connect to unblock
   the blocking `accept()` call, the standard dependency-free way to
   stop a blocking `TcpListener` thread from another thread.

5. **Fixed port, no TLS.** Port `49876` (from IANA's dynamic/private
   range, chosen to avoid collision with common dev-server ports like
   3000/5000/8080/8000). No `https://` - the same LAN-only reasoning as
   vMix's plain HTTP, and there is no browser padlock expectation to
   meet for a "follow along" page with no login and no sensitive data
   in transit.

6. **LAN address shown to the operator, not guessed for the congregant.**
   The desktop UI detects and displays the address(es) a phone should
   type in - it does not print a QR code or auto-discover anything on
   the congregant's device (out of scope; the operator can read a short
   URL off a slide or announce it). Address detection uses the
   dependency-free `UdpSocket::connect` routing-table trick (connect a
   UDP socket to a public address; nothing is transmitted, the OS just
   resolves which local interface/IP would be used) - this can return
   `None` on a machine with no configured network route at all, in
   which case the UI says so honestly rather than fabricating an
   address. See `docs/congregant-companion.md` for the full picture.

## Testing boundary (a genuine strengthening, not a rehash)

Unlike almost everything else in this codebase, the companion server's
core logic is a *real* `std::net::TcpListener`-based server, entirely
Tauri-agnostic (it operates on a plain `Arc<Mutex<Option<CompanionSnapshot>>>`,
never an `AppHandle`/`State`) - so, uniquely, this phase's tests are not
limited to pure-function unit tests standing in for an untestable Tauri
command. `companion.rs`'s tests bind a real listener on an OS-assigned
ephemeral port (`port: 0`) and make real `TcpStream` GET requests against
it, asserting on the real HTTP response bytes: the served HTML contains
the local-only notes disclosure, `/api/current` returns accurate JSON
before and after the snapshot changes, an unknown path 404s, and the
server actually stops accepting connections after `stop_server`. The
Tauri command wrappers (`enable_congregant_companion`, etc.) stay thin
and untested directly, per this project's standing "no `tauri::test`
harness" convention - but the thing that matters (does the server
actually work) is proven directly, not by proxy.

## What this phase does NOT do

- No QR code generation, no mDNS/Bonjour auto-discovery of the companion
  URL from a phone.
- No push notifications, no WebSocket - the page polls `/api/current`
  every two seconds, a deliberately simple and honest mechanism (a
  missed poll just means the phone is briefly one poll-interval behind
  Stage, never wrong forever).
- No congregant-facing history ("show me the last five things
  displayed") - only the current item, matching Stage's own single-item
  model.
- No authentication on the companion page itself, and no rate limiting
  or connection-count cap on the HTTP server - a LAN-only, read-only,
  no-PII surface judged not to need either for this phase; documented as
  an explicit, honest limitation rather than silently absent.
- No IPv6 support - `UdpSocket::connect`'s routing lookup and the
  `TcpListener::bind` both use plain IPv4 (`0.0.0.0`) for simplicity;
  most home/church wifi networks are IPv4-first for LAN clients anyway,
  but a IPv6-only network would not reach this server.
