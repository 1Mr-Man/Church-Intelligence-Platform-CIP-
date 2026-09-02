# Phase 6.6 — Operator Ergonomics: Onboarding

## Baseline

Phase 6.5 closed error banner dismiss/context. This phase closes gap #6
from Phase 6's own audit - the last of the 8 gaps to be scoped, since
"no onboarding" was named but never broken down. See
`docs/phase-6-6-audit.md`.

## Audit

`App.tsx` had zero first-run affordance: no `localStorage`/first-run
flag anywhere in the frontend, and every launch - first-ever or the
thousandth - dropped identically into the live workspace with no
explanation of the operator workflow.

The audit found two genuinely different things "onboarding" could mean:
a first-launch workflow walkthrough (explains *what to do*, once), and
a setup-readiness checklist (explains *whether you're ready*, every
launch until resolved). These answer different operator questions and
have genuinely different shapes - the same kind of real fork Phase 6.2
put to the operator directly rather than guessing. Put to the operator
via `AskUserQuestion`: build both.

While scoping the setup-readiness half, the audit found it already
mostly exists: `SystemStatusStrip` (Phase 3.5) already renders
Microphone/Speech/Bible/Display readiness in plain language,
unconditionally, in Operator Mode - not buried in Diagnostics Mode as
the initial audit doc assumed. What's actually missing is
*actionability*: an operator seeing "Speech: Optional — not configured"
has no path from Operator Mode to the one control that fixes it
(`PilotDiagnosticsPanel`'s "Select Existing Model File" button), since
that panel only renders in Diagnostics Mode - a mode a first-time
operator has no reason to know exists.

## What was built

- **`lib/onboarding.ts`** (new): `shouldShowWalkthrough(storedValue)` -
  the one piece of first-run logic worth unit testing on its own,
  separated from the actual `localStorage` read/write (which stays in
  the component, wrapped in try/catch - this project has no DOM testing
  environment, and a walkthrough that fails to persist should just show
  again next launch, never block anything).
- **`components/OnboardingWalkthrough.tsx`** (new): a dismissible
  overlay, rendered once from `App.tsx` above the section nav, showing
  the Start Service → Needs Attention → Approve/Reject → Display
  workflow plus a pointer to Diagnostics Mode. One "Got it" button
  dismisses it and marks it seen via `localStorage`; it never blocks
  any control underneath, and reappears only if browser storage is
  cleared.
- **`lib/setupGaps.ts`** (new): `computeSetupGaps(bibleInstalled,
  speechStatus)` - a pure function deciding which of two genuinely
  operator-actionable setup items (Bible dataset, Whisper model) are
  still outstanding, from `LiveStatus` fields already fetched for
  `SystemStatusStrip` - no new command, no new fetch. Deliberately
  narrower than everything `SystemStatusStrip` shows: a live
  `speechStatus === "error"` is already surfaced loudly elsewhere
  (Phase 6.4's real error text, the Phase 6.5 dismissible banner) -
  repeating it here as a "setup" item would be misleading noise, not a
  new fact. Only `"unavailable"` (no model configured at all - the
  expected, unremarkable first-run state) counts as a gap.
- **`LiveChurchBrain.tsx`**: a new banner, visible in Operator Mode only
  and only while `setupGaps.length > 0`, naming the outstanding item(s)
  in plain language with an "Open Diagnostics Mode" button
  (`setMode("diagnostics")` - the exact same state setter the header's
  own toggle already uses). Disappears on its own once both items are
  resolved. Purely a pointer to the pre-existing Diagnostics Mode
  controls, not a new setup mechanism.
- **Tests**: 4 new cases for `shouldShowWalkthrough`
  (`onboarding.test.ts`) - unseen/empty/other-value all show it, the
  exact seen marker suppresses it. 5 new cases for `computeSetupGaps`
  (`setupGaps.test.ts`) - both ready, Bible-only gap, speech-only gap,
  both gaps (Bible first), and a live speech error producing no gap.

## Full regression result

Frontend only - no Rust files changed this phase (the third Phase 6.x
frontend-only slice, after 6.2/6.3). `npm run typecheck`: clean.
`npm run lint`: same 5 pre-existing warnings as before this phase - no
new ones. `npm run test`: 257/257 passing (248 pre-existing + 9 new).
`npm run build`: succeeds.

## Windows rebuild

Frontend-only change - see
`pilot-evidence/6.6/windows/installer-contents-verification.json` for
the rebuild's direct binary verification, following the same
strings-tooling-limitation disclosure established in Phase 6.1-6.5.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - both additions are purely frontend, reading data
  (`LiveStatus`) or storage (`localStorage`) that already existed or
  was already fetched.
- The walkthrough never blocks: it is dismissible with one click and
  every control underneath renders normally regardless of its state.
- The setup notice never invents a new setup path: its one button calls
  `setMode("diagnostics")`, the exact same state setter the header's
  own Operator/Diagnostics toggle already uses.
- `computeSetupGaps` is read-only derived state - it never triggers a
  fetch, a command, or a state write of its own; it only decides what
  to render from data already in scope.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including 9 new unit tests covering the
  first-run-flag decision and the setup-gap computation.
- **Environment B**: unavailable in this session's container, a
  pre-existing, already-documented limitation - not this phase's
  regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: on first launch (or after clearing app storage), confirm the
  walkthrough appears and dismissing it never reappears on the next
  launch; with either the Bible dataset or a Whisper model not yet
  installed, confirm the setup notice names the right item(s) and that
  "Open Diagnostics Mode" switches modes; once both are installed,
  confirm the notice disappears.

## Known limitations

- **The walkthrough is a fixed 4-step summary, not interactive** - it
  does not highlight the real UI elements it describes (no spotlighting
  or step-by-step pointer), and it cannot be reopened once dismissed
  short of clearing browser storage. A "replay walkthrough" affordance
  is a reasonable future addition, not attempted this phase.
- **The setup notice covers exactly two items (Bible dataset, Whisper
  model)** - it does not attempt a full readiness score covering
  displays, audio device selection, or database health; those already
  have their own always-visible signal (`SystemStatusStrip`'s Display
  item, or `PilotDiagnosticsPanel` for full detail) and were judged not
  to need a second, redundant pointer.
- **`localStorage` persistence is per-browser-profile, not per-install**
  - clearing the Tauri webview's storage (or a fresh install to a new
  data directory in some configurations) will show the walkthrough
  again. This is the expected, accepted behavior for a "shown once"
  affordance with no server-side account to persist against.
- **This phase does not change what "ready" means** - `computeSetupGaps`
  reads the exact same `LiveStatus` fields `SystemStatusStrip` already
  used; no new readiness criterion was invented.
- **All 8 ergonomics gaps from Phase 6's own original audit are now
  scoped** - onboarding was the last unscoped one. 2 remain
  unaddressed: Diagnostics Mode density, unified-queue Edit support -
  each a candidate for a future Phase 6.x slice.
- **This exact rebuilt artifact has NOT yet been installed or launched
  on real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The remaining 2 Phase 6 ergonomics gaps from the original audit.
- Real-hardware Environment C verification.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase adds a
dismissible first-run explanation and a pointer to existing Diagnostics
Mode controls - it introduces no new backend surface, no new setup
mechanism, and changes no existing action's behavior.
