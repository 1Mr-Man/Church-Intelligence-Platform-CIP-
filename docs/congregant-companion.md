# Local Congregant Companion View

A read-only, LAN-only web page a congregant's phone can open to follow
along with whatever CIP's Stage display currently shows - no app to
install, no cloud, never leaves the church wifi. See
`docs/phase-11-audit.md` for the full design record and
`docs/phase-11-congregant-companion.md` for the phase report.

## What it shows

Exactly the same `Active` `PresentationItem` Stage/Confidence/Lobby
already display (Phase 3.10's multi-screen model, unchanged) - heading,
body lines, footer - or "Nothing is currently being displayed" when
nothing is active. The page polls `GET /api/current` every two seconds;
a missed poll just means the phone is briefly one interval behind Stage,
never wrong forever.

## Personal notes

The page has a notes textarea. It is saved to the phone's own browser
`localStorage` only - the companion server has no write endpoint at all,
only two read-only routes (`GET /`, `GET /api/current`). CIP never
receives, stores, or transmits a congregant's notes; the page's own
copy says so ("Notes stay on this device only").

## Turning it on

**Admin only** (`enable_congregant_companion`/`disable_congregant_companion` -
see `docs/roles-permissions.md`): starting a LAN-listening server is a
configuration act, joining the seven commands Phase 10 already gates.
Off by default. Once running, the page itself needs no login - a
congregant's phone is not an operator account.

The desktop app's Congregant Companion View panel shows the server's
status and, when running, the `http://` address(es) to share - read
these off to the congregation, put them on a slide, or announce a short
link. Address detection is best-effort (see `docs/phase-11-audit.md`
item 6); if none is found, the panel says so and the port is still shown
so the operator can construct the address manually from this machine's
own IP.

## Port and protocol

Fixed port `49876` (from IANA's dynamic/private range, chosen to avoid
colliding with common dev-server ports). Plain `http://`, no TLS - the
same LAN-only reasoning Phase 8's vMix client already established for a
local, no-login protocol; there is no sensitive data in transit and no
browser padlock expectation to meet.

## What this is - and isn't - a privacy boundary against

The server has no authentication and no rate limiting: anyone on the
same LAN can open the page and see what Stage currently shows - the same
thing anyone in the room can already see by looking at the projector.
It is not a boundary against a determined attacker on the same network;
it is a convenience for congregants who want to follow along on their
own phone. See `docs/phase-11-audit.md`'s "What this phase does NOT do"
for the complete list of deliberately out-of-scope items (QR codes,
auto-discovery, push/WebSocket, history, rate limiting, IPv6).

## Cross-references

- [`docs/phase-11-audit.md`](phase-11-audit.md) - the full design record
  and every deliberate trade-off's rationale.
- [`docs/phase-11-congregant-companion.md`](phase-11-congregant-companion.md) -
  the phase report (what was built, tests, regression, Windows proof).
- [`docs/roles-permissions.md`](roles-permissions.md) - the Admin gate
  this phase's `enable`/`disable` commands join.
- [`docs/presentation.md`](presentation.md) - Stage/Confidence/Lobby's
  own multi-screen model, which this phase mirrors rather than
  replaces.
