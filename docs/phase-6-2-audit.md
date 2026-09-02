# Phase 6.2 — Audit: Display Confirmation / Undo

## Baseline

Phase 6.1 closed the first Operator Ergonomics slice (keyboard shortcuts
correctly targeting the Needs Attention queue). This audit opens the
second: gap #2 from Phase 6's own audit, named tersely in
`docs/phase-6-1-operator-ergonomics-shortcuts.md` ("no confirmation/undo
on the one-click Display action") but never broken down further - this
document is that breakdown.

## What "Display" actually does today

`apps/desktop/src/components/LiveChurchBrain.tsx`'s `handleUnifiedAction`
fires immediately on click for the Bible domain's `"display"` action -
no dialog, no intervening step:

```
await commands.approveSuggestion(item.id);
const prepared = await commands.preparePresentation(item.id);
await commands.displayPresentation(prepared.id);
```

`display_presentation` (`commands.rs`) opens the Stage window if needed,
commits `Prepared -> Active`, and broadcasts `PresentationStarted` to
every Live screen - by the time the click handler returns, the wrong
verse (if it was a misclick) is already on the real projector. No other
action in `handleUnifiedAction` - approve/reject/accept/acknowledge/
review/dismiss - has a confirmation step either; this is a codebase-wide
pattern, not a Display-specific oversight.

This is deliberate, not accidental: Phase 3.8.7.8 built Display
specifically to replace a multi-click Approve -> scroll -> Prepare ->
Display sequence, at the operator's own explicit request, because the
old flow "cost several seconds and clicks during a live service." A
confirmation dialog in front of Display would partially undo that
phase's own point.

## What already exists as a recovery path

- A manual **Stop** button already exists on the Presentation card
  (`PresentationCard.tsx`), wired to `clear_presentation_display` ->
  `presentation::stop_active_item`. It transitions the Active item to
  Stopped and blanks the display window. It is *not* a restore: it only
  clears the screen to blank, it does not bring back whatever was
  showing before the mistaken Display.
- `cancel_item`/`cancel_presentation` can only retract a still-`Prepared`
  item - once `display_presentation` has committed `Active`, this path
  is no longer available.
- Presentation Router's per-screen Live/Held toggle (Phase 3.10.3)
  freezes a screen on its current content - it does not blank or reverse
  it, so it offers no undo path for a misclick already showing.
- No confirmation-dialog UI pattern exists anywhere in this codebase
  today (`window.confirm`, a custom dialog component, an "Are you sure"
  string - none of these appear in `apps/desktop/src/`). Whichever
  direction this phase takes, it is the first of its kind here.

## The real design tension

Two genuinely different, both-legitimate responses to "an operator's
misclick could put the wrong verse on a real projector":

1. **Confirm before it happens** (a lightweight in-place confirm, not a
   blocking modal - e.g. the Display button itself asks for a second
   click within a short window) - trades away some of Phase 3.8.7.8's
   own one-click speed gain for a guard rail before the mistake ever
   reaches the screen. Every Display, correct or not, now takes two
   actions.
2. **Make the existing Stop button impossible to miss right after a
   Display fires** (e.g. a brief, prominent "Displayed - Undo" surfaced
   the instant `handleUnifiedAction` resolves, calling the exact same
   `clear_presentation_display` command the Stop button already calls) -
   keeps every correct Display exactly as fast as it is today, and only
   costs the operator anything on the rare mistaken click. Recovery is
   "blank the screen," matching what Stop already does - not a true
   "put back what was there before," since that state isn't tracked
   anywhere today.

A third option - do both - is possible but doubles the new surface for
what is otherwise a small ergonomics fix, and a confirm-before-every-click
control most directly fights the speed goal Display exists for.

## What this audit does not resolve

Which of these (or a combination) the operator wants is a genuine,
comparably-sized fork - not a case where one option is obviously correct
- so it is being put to the operator directly rather than assumed.
