# Phase 6.5 — Operator Ergonomics: Error Banner Dismiss/Context

## Baseline

Phase 6.4 closed buried audio/speech error visibility. This phase closes
gap #5 from Phase 6's own audit: "no dismiss/context on the generic
error banner" - see `docs/phase-6-5-audit.md` for the full breakdown.

## Audit

The `live-brain__error` banner had no dismiss affordance at all -
`error` cleared only at the start of the *next* tracked (`withBusy`)
action, regardless of outcome, so a failed action's banner sat on screen
indefinitely until the operator happened to trigger some other action.
Every `setError` call site discarded context about *which* action
failed - `withBusy`'s catch block already had a descriptive `key` in
scope (`approve-${id}`, `start-service`, etc., already used for
button-disabling) but never surfaced it, so two different failures in
quick succession produced indistinguishable banners. No generic
dismiss-button/toast pattern existed anywhere in the codebase to reuse.

## What was built

- **`apps/desktop/src/lib/errorContext.ts`** (new): `humanizeBusyKey(key)`
  - a pure, mechanical transform (strip a trailing UUID, turn hyphens
  into spaces, capitalize) turning a busy key into a short label
  ("approve-<uuid>" -> "Approve") without a lookup table for ~40+ call
  sites.
- **`LiveChurchBrain.tsx`**: `withBusy`'s catch block now prepends
  `` `${humanizeBusyKey(key)} failed: ` `` to the error message (one call
  site covers every `withBusy`-wrapped action); the three other
  `setError` sites without a busy key (status poll, history load,
  timeline load) get an equivalent static label each. The one already-
  contextual site (`FileReader.onerror`, which already embeds the
  filename) is unchanged. The banner gained a small × dismiss button
  calling the same `setError(null)` that already clears it today - no
  new clearing mechanism, just a manual trigger for the existing one.
- **Tests**: 6 new cases for `humanizeBusyKey` (`errorContext.test.ts`) -
  UUID-suffix stripping, multi-word keys, plain hyphenated keys,
  single-word keys, a non-UUID suffix left untouched, and the empty-
  string edge case.

## Full regression result

Frontend only - no Rust files changed this phase. `npm run typecheck`:
clean. `npm run lint`: same pre-existing warnings as before this phase -
no new ones. `npm run test`: 248/248 passing (242 pre-existing + 6
new). `npm run build`: succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.5/windows/installer-contents-verification.json` for
the rebuild's direct binary verification, following the same
strings-tooling-limitation disclosure established in Phase 6.1-6.4.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - purely a frontend message-formatting and dismiss-UI
  addition.
- The dismiss button calls the exact same `setError(null)` already used
  elsewhere - no new state, no new clearing path with different
  semantics.
- `humanizeBusyKey` never changes *what* fires - it only relabels the
  same key already computed for `setBusy`/`isBusy`, purely for display.
- Every existing `setError` call site still sets the same underlying
  `error` state; only the string content changed.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including 6 new unit tests for
  `humanizeBusyKey` covering UUID-stripping, multi-word/single-word
  keys, a non-UUID suffix, and the empty-string edge case.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: trigger a real command failure (e.g. approve a suggestion while
  offline, or attempt an action during a network hiccup), confirm the
  banner names the failed action, then click the × and confirm it
  clears without needing to trigger another action first.

## Known limitations

- **`humanizeBusyKey` is mechanical, not a curated label set** - for
  keys embedding non-UUID text (e.g. a Bible reference in
  `search-preview-${reference}`), the label reads literally
  ("Search preview ROM 8:28") rather than a hand-tuned phrase. Readable,
  not polished English - a deliberate tradeoff to avoid a per-call-site
  lookup table for ~40+ actions.
- **Dismissing the banner does not retry the failed action** - it only
  clears the message; the operator must re-trigger the original action
  themselves if they still want it to happen.
- **3 more ergonomics gaps from Phase 6's own audit remain unaddressed**
  after this phase (onboarding, Diagnostics Mode density, unified-queue
  Edit support) - each a candidate for a future Phase 6.x slice.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The remaining Phase 6 ergonomics gaps from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase adds a manual
dismiss for an existing clear mechanism and relabels existing error
text with context already computed elsewhere - it introduces no new
backend surface and changes no existing action's behavior.
