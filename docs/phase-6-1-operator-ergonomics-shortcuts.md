# Phase 6.1 — Operator Ergonomics: Keyboard Shortcuts for the Needs Attention Queue

## Baseline

Phase 5.1-5.4 closed the "Reliability & Trust" theme. This phase opens
the next roadmap theme, "Operator Ergonomics" (Phase 6), with an
audit-first slice the operator chose from a broader list of findings.

## Audit

An audit of the live operator workflow (`LiveChurchBrain.tsx`,
`workspace/AttentionQueue.tsx`, `workspace/IntelligenceCard.tsx`,
`lib/keyboardShortcuts.ts`) found 8 genuine ergonomics gaps. The operator
selected the highest-priority one to fix first:

**Keyboard shortcuts (A/R/E/P) only ever acted on `suggestions[0]` - the
raw, Bible-only pending-suggestion list - regardless of which mode the
operator was in.** The diagnostics-mode "Pending Suggestions" panel
correctly documents this ("A/R/E/P act on the first one"), but Operator
Mode's primary panel, the Needs Attention queue
(`AttentionQueue`/`unifiedFeed`), is a *different* list: its own
confidence-based ordering, spanning six domains (Bible/Music/Sermon/
Service/Content/Correlation), built by
`lib/attentionQueue.ts::buildAttentionQueue`. Before this phase, pressing
"A" while looking at Operator Mode's Needs Attention queue silently acted
on `suggestions[0]` - not necessarily the item visually at the top of the
queue, and not even guaranteed to be a Bible item at all. A solo live
operator trusting the shortcut to act on what they see on screen could
approve or reject the wrong thing.

## Architecture decision

- **Mode-gated shortcut targets, not a shared one**: Diagnostics Mode
  keeps its exact existing A/R/E/P behavior against `suggestions[0]`,
  unchanged - that panel's Edit/Preview tools have no equivalent in the
  domain-generic unified action model, and rewriting it wasn't in scope.
  Operator Mode gets its own A/R behavior against `attentionQueue[0]`,
  dispatched through the exact same `handleUnifiedAction` every card's
  own buttons already call - never a second command-dispatch path.
- **Domain-generic key mapping, not per-domain special-casing**: "A"
  always maps to the domain's primary action (`actionsFor(domain)[0]` -
  display/accept/acknowledge/review), "R" to its secondary/negative one
  (`actionsFor(domain)[1]` - reject/dismiss), if the domain has one. A
  new pure function, `resolveUnifiedShortcutAction` (`lib/keyboardShortcuts.ts`),
  encodes this mapping once so it can never fire an action a button on
  screen wasn't already offering, and is directly unit-testable without a
  DOM.
- **A visible legend, not a silent feature**: the Needs Attention queue's
  heading now shows exactly what A/R will do to the top item (e.g. "A =
  Display, R = Reject for the first item"), mirroring the discoverability
  hint the diagnostics panel already had. Computed by a new pure function,
  `shortcutLegend` (`lib/attentionQueue.ts`), from the same
  `actionsFor`/`ACTION_LABELS` the cards themselves render - never a
  second source of truth for what a shortcut does.
- **No new commands, no new backend surface**: this is a pure frontend
  dispatch-target fix. Every action the shortcuts trigger already existed
  and was already reachable by clicking the matching card's button.

## What was built

- **`apps/desktop/src/lib/keyboardShortcuts.ts`**: new
  `resolveUnifiedShortcutAction(key, actions)` - maps "a"/"r" to the
  primary/secondary `UnifiedItemAction` for a domain's action list, `null`
  for any other key or an absent position.
- **`apps/desktop/src/lib/attentionQueue.ts`**: new
  `shortcutLegend(queue)` - describes what A/R will do to the queue's top
  item, or `null` for an empty queue.
- **`apps/desktop/src/components/LiveChurchBrain.tsx`**: the keyboard
  shortcut effect relocated to after `handleUnifiedAction` is declared
  (so it can reference `attentionQueue`) and rewritten to branch on
  `mode`: Operator Mode uses `attentionQueue[0]` +
  `resolveUnifiedShortcutAction` + `handleUnifiedAction`; Diagnostics
  Mode keeps its exact prior `suggestions[0]`-based A/R/E/P behavior. "S"
  (focus manual search) is unchanged in either mode.
- **`apps/desktop/src/components/workspace/AttentionQueue.tsx`**: the
  "Needs Attention" heading now renders `shortcutLegend`'s output as a
  hint span when nonempty.
- **Tests**: 5 new cases for `resolveUnifiedShortcutAction`
  (`keyboardShortcuts.test.ts`), 6 new cases for `shortcutLegend`
  (`attentionQueue.test.ts`) - both plain-object/pure-function tests per
  this project's no-DOM-testing-environment convention.

## Full regression result

Frontend only - no Rust files changed this phase. `npm run typecheck`:
clean. `npm run lint`: only the 4 pre-existing `set-state-in-effect`/
`only-export-components` warnings present before this phase, in files
this phase did not touch - no new warnings. `npm run test`: 231/231
passing (220 pre-existing + 11 new). `npm run build`: succeeds.

## Windows rebuild

Tauri embeds the built frontend bundle into `cip-desktop.exe`, so even a
frontend-only change requires a rebuild for the shipped binary to
actually contain it - see `pilot-evidence/6.1/windows/installer-contents-verification.json`
for direct proof the new dispatch logic and legend text are present in
the extracted, embedded frontend bundle.

## Architectural safety diff

- Zero new Tauri commands, zero new events, zero new database
  columns/tables - a pure frontend dispatch-target and discoverability
  fix.
- Zero change to Diagnostics Mode's shortcut behavior - `suggestions[0]`,
  A/R/E/P, all byte-identical to before.
- Zero change to what any action *does* - `resolveUnifiedShortcutAction`
  only ever returns an action already in `actionsFor(domain)`, and
  `handleUnifiedAction` is the exact same dispatcher the cards' own
  buttons call.
- The one real behavior change: A/R in Operator Mode now act on the
  visually-correct top item of the Needs Attention queue instead of a
  hidden, potentially-mismatched `suggestions[0]` - a correctness fix
  found during this phase's own audit, not a new feature surface.

## Environment A / B / C

- **Environment A** (this container): PASSED - full frontend regression
  green as detailed above, including unit tests for both new pure
  functions covering every domain's action shape (two actions, one
  action, empty).
- **Environment B** (Xvfb GUI reproduction): unavailable in this
  session's container, a pre-existing, already-documented limitation
  since Phase 3.8.5 - not this phase's regression.
- **Environment C** (real Windows hardware, a real live service): NOT YET
  VERIFIED. The decisive pending gate is the operator's own real-hardware
  test: with items from more than one domain in the Needs Attention
  queue, confirm the legend text matches the top item, and that pressing
  A/R acts on that exact item, not a different one.

## Known limitations

- **Scoped to A/R only** - "E" (edit) and "P" (preview) remain
  Diagnostics-mode-only, since editing a reference has no equivalent
  action in the domain-generic unified model yet (audit finding #3,
  named as a larger/architectural gap, not attempted this phase).
- **The remaining 7 ergonomics gaps from this phase's audit are
  unaddressed**: no confirmation/undo on the one-click Display action, no
  text search over the live feed, buried audio/speech error visibility,
  no dismiss/context on the generic error banner, no onboarding, and a
  still-dense Diagnostics Mode. Each remains a candidate for a future
  Phase 6.x slice.
- **No visual on-screen indicator of which item is "first"** beyond the
  queue's own existing ordering - an operator must still visually
  identify the top card; the legend describes the action, not which card
  it applies to.
- **This exact rebuilt artifact has NOT yet been installed or launched on
  real Windows hardware** - see `physicalHardwareStatement` in
  `release/windows/release-manifest.json`.

## Deferred work

- The remaining Phase 6 ergonomics gaps identified in this phase's audit
  (Display confirmation/undo, feed search, error visibility, onboarding,
  Diagnostics Mode density, unified-queue Edit support).
- Real-hardware Environment C verification against a real multi-domain
  Needs Attention queue.

## Final gate

Environment A: **PASS**. Environment C: **PENDING** (real Windows
hardware, outside this container's reach). This phase is a real,
verifiable, fully-tested correctness fix and ergonomics improvement - it
introduces no new backend surface, changes no existing action's
behavior, and only makes an already-existing shortcut mechanism target
what the operator actually sees on screen.
