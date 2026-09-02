# Phase 6.5 — Audit: Error Banner Dismiss/Context

## Baseline

Phase 6.4 closed buried audio/speech error visibility. This audit opens
gap #5 from Phase 6's own audit: "no dismiss/context on the generic
error banner" (`docs/phase-6-1-operator-ergonomics-shortcuts.md`).

## What exists today

`LiveChurchBrain.tsx`'s `live-brain__error` banner (`<p role="alert">
{error}</p>`) has no dismiss affordance at all - no button, no ×, no
keyboard path. `error` is cleared only at the start of the *next*
`withBusy` call (line 504, unconditionally, before that new action's own
outcome is known); there is no independent success-clears-old-error
logic and no auto-dismiss timer. A failed action's banner therefore sits
on screen, unrelated to anything else happening, until the operator
happens to trigger some other tracked action.

`withBusy(key, action)` already computes a descriptive `key` for every
one of its ~40+ call sites (`approve-${id}`, `start-service`, `search`,
`` `cross-domain-dismiss-${correlation.id}` ``, etc.) - used only for
`setBusy`/`isBusy` (button-disabling), never surfaced. The catch block
calls `setError(String(e))` with no reference to which action was being
attempted, so two different failures in quick succession (e.g. Approve
on one item, then Reject on another) are visually indistinguishable.
Three other `setError` sites (status poll, history load, timeline load)
have the identical problem. The one exception, a `FileReader.onerror`
during dataset import, already embeds the filename - the only call site
that already does this right.

No generic dismiss-button/toast pattern exists anywhere in
`apps/desktop/src/` to reuse - the two "Dismiss" buttons that do exist
(`AmbiguousCard`, cross-domain correlation) are both domain-specific list
actions wired to their own state/command, not a reusable "close this
banner" primitive.

## Design (no fork - proceeding directly)

Both sub-parts of this gap have one clear, minimal shape - not a
comparably-sized architectural choice - so this phase proceeds straight
to implementation:

- **Dismiss**: a small × button inside the banner, calling the exact
  `setError(null)` that already clears it today - no new state, no new
  clearing mechanism, just a manual trigger for the one that exists.
- **Context**: a new pure function, `humanizeBusyKey(key)`
  (`lib/errorContext.ts`), turns a busy key into a short, readable label
  ("approve-<uuid>" -> "Approve", "cross-domain-dismiss-<uuid>" -> "Cross
  domain dismiss") by stripping a trailing UUID (the common id-suffix
  shape across call sites) and turning the remaining hyphens into
  spaces. `withBusy`'s single catch block prepends this label to the
  error message it already builds - one call site, not all ~40+ -
  and the three non-`withBusy` setError sites get an equivalent static
  label each ("Status update", "Load history", "Load service detail").
  The one already-contextual site (`FileReader.onerror`) is untouched.
