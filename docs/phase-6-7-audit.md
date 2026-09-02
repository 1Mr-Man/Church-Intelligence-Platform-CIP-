# Phase 6.7 — Operator Ergonomics: Diagnostics Mode Density Audit

## Baseline

Phase 6.6 closed onboarding - the last unscoped Phase 6 gap. This phase
opens gap #7: "still-dense Diagnostics Mode," named since Phase 6.1's
own report but never broken down further.

## What's actually there

`LiveChurchBrain.tsx`'s Diagnostics-Mode-only block
(`{mode === "diagnostics" && (...)}`, lines ~1129-2390, ~1,260 lines of
JSX) renders 11 top-level panels back to back, always fully expanded,
with no way to collapse any of them:

1. Pending Suggestions (the raw Bible-only suggestion list with
   Approve/Reject/Edit/Preview - the one panel an operator is most
   likely to actually use mid-service in Diagnostics Mode)
2. Service Timeline
3. Manual Bible Search (plus nested search-result/history `<details>`)
4. Music Intelligence
5. Sermon Foundation
6. Sermon Intelligence
7. Sermon Harvest
8. Cross-Domain Intelligence
9. Content Intelligence (its own inner "Content Registry" sub-panel is
   already a `<details>`)
10. Service Intelligence
11. Service History

Two further Diagnostics-Mode-only components render above this block:
`PilotDiagnosticsPanel` ("System Diagnostics") and `StatusBar` -
`PilotDiagnosticsPanel` already wraps itself in
`<details className="live-brain__panel">`, collapsible by default.
That's the one place in Diagnostics Mode this problem is already
solved; the other 11 panels are plain `<section className="live-brain__panel">`,
never collapsible.

A technician debugging one specific domain (say, Sermon Intelligence)
must scroll past everything else - 10 other fully-rendered panels,
several of them substantial (Sermon Foundation alone is ~165 lines of
JSX) - to reach it, and everything stays on screen regardless of
whether they need it right now.

## Design choice

The fix itself has one clear shape, already established twice in this
exact codebase: wrap each `<section className="live-brain__panel">` in
`<details className="live-brain__panel">` (`<summary>` carrying the
existing `<h2>` text), matching the precedent `PilotDiagnosticsPanel`
and the inner "Content Registry" panel already set. No new component,
no new state, no restructuring of what each panel renders - purely a
collapse affordance added around existing content.

What genuinely forks is the default state: should every panel default
**open** (identical visible density to today until an operator
manually collapses something - purely opt-in, zero risk of hiding
anything anyone currently relies on) or should the panels an operator
is *least* likely to need mid-service default **closed**, with Pending
Suggestions and Service Timeline (the two with real Approve/Reject/
history actions) left open? The second option is what actually solves
"dense" on first load, but it changes what's visible by default for 9
existing panels a technician may currently expect to just see. This is
the same shape of tradeoff Phase 6.2 and Phase 6.6 put to the operator
rather than guessing - put to the operator via `AskUserQuestion`.
