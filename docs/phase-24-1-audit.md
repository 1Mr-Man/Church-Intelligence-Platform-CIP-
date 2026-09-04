# Phase 24.1: Real Slides in the Queue Strip

## Trigger

Direct operator follow-up after Phase 24's dual Live/Preview panel: "Continue
there next" - closing the one honestly-deferred gap that phase's own audit
doc flagged: the queue strip's thumbnails approximated a slide from raw
`PresentationContent` text rather than showing the actual `RenderedSlide`.

## What was actually missing

Phase 24 could show the **Live** panel's real slide (`activeSlide` was
already being fetched and discarded) and the **Preview** panel's real slide
(`previews` already held one from the existing Preview button) with zero
new backend work. The queue strip's *prepared* items had no such data
available anywhere in the frontend - a `PresentationItem` carries `content:
PresentationContent` (raw reference/text), never a `RenderedSlide` (the
word-wrapped, template-selected render). Nothing had ever computed one for
a prepared item outside the moment it becomes `Active`
(`display_presentation`) or is manually previewed pre-approval
(`preview_presentation`/`preview_scripture`).

## Design decision: a thin, read-only render command, reused everywhere else

`cip_presentation_renderer::render_content(&PresentationContent) ->
RenderedSlide` is already this codebase's single rendering system - every
other slide (the real display window, the Live panel, the Preview panel)
is built from it, never a second implementation. The fix adds one new
Tauri command, `get_prepared_item_slide(item_id) -> RenderedSlide`
(`apps/desktop/src-tauri/src/commands.rs`), that:

1. looks up the already-persisted `PresentationItem` by id
   (`persistence::get_presentation_item` - the same lookup
   `get_presentation_item` already exposes), and
2. calls the same `render_content` `display_presentation` and
   `get_presentation_display_state` already call.

It never mutates status and works for an item in any status (`Prepared`,
`Active`, `Stopped`) - it is a render, not a lifecycle transition, so it
carries none of `preview_presentation`'s approval-gating (that gate exists
to control what may be *displayed*/*prepared*, not to control who may
re-render already-persisted content for their own screen).

An alternative considered and rejected: extending
`list_prepared_presentations` to return `(PresentationItem, RenderedSlide)`
pairs directly, avoiding the extra round trip. Rejected because it would
have changed a widely-consumed return type (`PresentationItem[]`) touching
every existing caller - `PresentationCard`, both frontend test suites, and
both operator screens - for a queue strip whose item count is always small
in practice (never more than a handful of prepared-but-not-yet-displayed
items in a real service). The one-command-per-item cost is negligible at
that scale and keeps this phase's diff purely additive.

## Frontend wiring

`apps/desktop/src/lib/commands.ts` gained `getPreparedItemSlide(itemId)`.
`LiveChurchBrain.tsx` and `ServiceReplay.tsx` each gained a `queueSlides:
Record<string, RenderedSlide>` state and a `useEffect` that fetches a
slide for any `preparedItems` entry not already in `queueSlides` (a failed
fetch is swallowed - that item simply keeps `LivePreviewStage`'s existing
raw-text fallback, never a broken thumbnail). `LivePreviewStage.tsx`'s
queue-item rendering now prefers `queueSlides[item.id]`'s real
heading/body/footer over the old `queueHeading`/`queueSnippet` helpers,
which remain only as that fallback.

Stale `queueSlides` entries for items no longer `preparedItems` (cancelled,
displayed, or otherwise removed) are never explicitly pruned - the queue
strip only ever renders `queueSlides[item.id]` for an `item` still present
in `preparedItems`, so an orphaned entry is simply unreachable, and a real
service's prepared-item count over its lifetime is nowhere near large
enough for the small `RenderedSlide` objects involved to be a meaningful
memory concern. An earlier draft explicitly pruned stale entries in the
same effect; it was dropped because the synchronous `setState` call it
required to do so is exactly the pattern this codebase's own lint rule
(`react(set-state-in-effect)`) flags, and pruning added no correctness
value the unreachability argument above doesn't already cover for free.

## Real-browser verification

Re-ran the same headless-Chromium + mocked-Tauri-bridge technique Phase 23
and Phase 24 established, extending the existing fixture with a
`get_prepared_item_slide` mock response keyed by item id (returning a real
`RenderedSlide` for the two prepared items already in the fixture, "John
3:16" and "Psalm 23:1"). Confirmed on both Live Service and Service Replay
that the queue-strip cards now render the actual word-wrapped body text
and translation footer from the mocked `RenderedSlide`, not the
single-line raw-text snippet Phase 24 shipped.

## Why this doesn't disturb anything downstream

- `PresentationCard`'s own rendering/state/event handling is completely
  untouched - this phase only added a second reader of `preparedItems`,
  never a writer.
- `get_presentation_item` (the existing single-item-by-id command) is
  untouched; `get_prepared_item_slide` is a new, separate command, not a
  change to that one's contract.
- The queue strip's click behavior (send live immediately, per the
  existing explicit-activation safety model - see Phase 24's own audit
  doc) is unchanged; only what the card displays before that click
  changed.
- `PresentationDisplay.tsx` (the real display window) is untouched.

## Testing boundary

Consistent with this project's established convention (see Phase 24's own
audit doc, and `presentation.rs`'s own module docs on why command
*functions* stay untested thin wrappers with no `tauri::test` harness in
this project): `get_prepared_item_slide` introduces no new pure logic of
its own - `parse_uuid`, `persistence::get_presentation_item`, and
`render_content` are each already covered by existing tests
(`persistence.rs`'s presentation-item round-trip tests and
`presentation/renderer/src/lib.rs`'s render tests respectively). No new
Rust test was added for the command wrapper itself, matching
`commands::get_presentation_item`'s own precedent (also untested at the
command-wrapper level for the same reason).

## Full regression result

Rust: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D
warnings` clean in both default and `--features whisper` configs,
`cargo test --workspace` 365/365 passing in both configs (unchanged count
- no new pure logic was added to test). Frontend: `npm run typecheck` 0
errors, `npm run lint` the same 4 pre-existing warnings (unchanged - an
earlier draft of the new `useEffect` briefly introduced 4 new warnings, 2
per file, from a synchronous prune `setState` call and a resulting missing-
dependency warning; both were eliminated by dropping the explicit prune,
see above), `npm run test -- --run` 303/303 (unchanged), `npm run build`
clean.

## Known limitations (honest, not deferred silently)

- **A failed `get_prepared_item_slide` fetch is silently swallowed.** The
  affected queue card falls back to the raw-text approximation rather than
  showing an error - consistent with this being a display convenience, not
  a safety-relevant path (the actual Display click still sends the item's
  real persisted content, regardless of whether its thumbnail rendered).
- **This exact change has not been confirmed on real Windows hardware
  running the actual compiled desktop app** - verification used a mocked
  Tauri bridge in headless Chromium. The decisive test is a real operator
  preparing several items and confirming the queue strip's cards show the
  same wrapped text/footer the real Display action would show.

## Final gate

Environment A (typecheck/lint/test/build, plus real-browser-engine visual
verification of both screens' queue-strip thumbnails via headless
Chromium with a mocked Tauri bridge): PASS. Environment C (a real operator
confirming the queue strip's thumbnails match real prepared-item content
during an actual service): not yet performed.
