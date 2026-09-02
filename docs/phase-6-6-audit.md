# Phase 6.6 — Operator Ergonomics: Onboarding Audit

## Baseline

Phase 6.5 closed error banner dismiss/context. This phase opens gap #6
from Phase 6's own audit: "no onboarding" - named but never scoped in
any prior phase's report.

## What exists today

`App.tsx` has zero first-run affordance of any kind:

- No `localStorage`/first-run flag exists anywhere in the frontend
  (verified via `grep -rn "localStorage\|firstRun\|Welcome" apps/desktop/src`
  - the only "Welcome" hits are `ServiceReplay`'s canned demo transcript
  and a library test fixture, unrelated to onboarding).
- `App.tsx:69-77` renders the section nav and `LiveChurchBrain` (Live
  Service, the default tab) immediately on every launch, first-ever or
  the thousandth - identical either way.
- `LiveChurchBrain.tsx` opens straight into Operator Mode: the
  workspace, the Needs Attention queue, the Presentation card, the
  Intelligence Feed - fully live-service-shaped UI with no explanatory
  text anywhere for what any of it does or what order to use it in.
- The one nod to setup readiness that already exists,
  `PilotDiagnosticsPanel` (`get_pilot_diagnostics`, Phase 3.2), is
  buried inside Diagnostics Mode - a mode switch an operator has no
  reason to know exists on first launch, let alone reach. It already
  surfaces exactly the readiness signals a first-time operator needs
  before they can run a real service: `whisperModel` status
  (missing/unreadable/present), `bible` dataset presence (`null` when
  not registered), `audioDevices`, and `displays` - all fetched by a
  single existing command, `getPilotDiagnostics()`
  (`apps/desktop/src/lib/commands.ts`).
- No explanation anywhere (in-app) of the operator workflow itself:
  Start Service → speak/replay → Needs Attention queue fills →
  Approve/Reject (or A/R) → Display projects it. An operator who has
  never seen this app has no in-app way to learn this without trial
  and error or reading the repository's own docs, which do not ship
  with the installer.

## Two genuinely different things "onboarding" could mean

1. **A first-launch workflow walkthrough** - explains what the app
   does and the Start Service → queue → Approve/Reject → Display loop,
   shown once (dismissible, gated on a `localStorage` first-run flag),
   never blocking. Solves "I don't know what to do," not "I don't know
   if my setup is ready."
2. **A setup-readiness checklist** - surfaces `PilotDiagnosticsPanel`'s
   existing readiness data (Whisper model present, Bible dataset
   registered, at least one audio device, at least one display) in
   Operator Mode itself, not buried in Diagnostics Mode, so an operator
   sees before they try to start a service whether it will actually
   work. Solves "I don't know if I'm ready," not "I don't know what to
   do next."

These are not two implementations of the same gap - they answer two
different operator questions, at two different moments (once, ever vs.
every launch until resolved), with two different genuinely reasonable
shapes (a modal/tour vs. a persistent status panel). This is the same
kind of real, consequential fork Phase 6.2 put to the operator directly
(confirm vs. undo vs. both) rather than picking unilaterally - proceeding
alone here would mean guessing which operator problem actually matters
most, with no way to know without asking.

## Design choice

Put to the operator via `AskUserQuestion`: workflow walkthrough, setup
checklist, or both.
